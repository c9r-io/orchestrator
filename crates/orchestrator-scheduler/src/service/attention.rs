//! Built-in policies that project durable task events into the attention queue.

use agent_orchestrator::attention::{
    AttentionActionDescriptor, AttentionCandidate, AttentionProjectionOp, AttentionSeverity,
    AttentionSourceEvent,
};
use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::config_load::now_ts;
use agent_orchestrator::state::InnerState;
use anyhow::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BATCH_SIZE: usize = 500;

/// Projects one bounded event batch and wakes expired snoozes.
///
/// The event cursor advances in the same transaction as queue mutations, so a
/// daemon restart either replays the complete batch or observes all of it.
pub async fn reconcile_attention_once(state: &InnerState) -> Result<usize> {
    state.attention_repo.wake_expired_snoozes(&now_ts()).await?;
    let cursor = state.attention_repo.projector_cursor().await?;
    let events = state
        .attention_repo
        .load_source_events(cursor, BATCH_SIZE)
        .await?;
    let Some(last) = events.last() else {
        return Ok(0);
    };
    let last_event_id = last.id;
    let config = agent_orchestrator::config_load::read_loaded_config(state)?;
    let operations = events
        .iter()
        .filter(|event| {
            config
                .config
                .runtime_policy_for_project(&event.project_id)
                .attention_inbox_enabled
        })
        .flat_map(policy_operations)
        .collect();
    state
        .attention_repo
        .apply_projection_batch(operations, last_event_id)
        .await?;
    tracing::info!(
        source_events = events.len(),
        cursor = last_event_id,
        "attention inbox projection batch committed"
    );
    Ok(events.len())
}

fn policy_operations(event: &AttentionSourceEvent) -> Vec<AttentionProjectionOp> {
    if matches!(
        event.event_type.as_str(),
        "task_completed" | "task_finished"
    ) {
        return vec![AttentionProjectionOp::ResolveTask {
            task_id: event.task_id.clone(),
            source_event_id: event.id.to_string(),
        }];
    }

    if is_successful_step(event) {
        if let Some(step_id) = step_id(event) {
            return vec![AttentionProjectionOp::ResolveStep {
                task_id: event.task_id.clone(),
                task_item_id: event.task_item_id.clone(),
                step_id,
                source_event_id: event.id.to_string(),
            }];
        }
    }

    let policy = match event.event_type.as_str() {
        "approval_required" | "approval_requested" => Some((
            "approval_required",
            AttentionSeverity::Intervention,
            "Approval required",
        )),
        "agent_question" | "decision_required" => Some((
            "agent_question",
            AttentionSeverity::Intervention,
            "Agent needs a decision",
        )),
        "retry_exhausted" => Some((
            "retry_exhausted",
            AttentionSeverity::Intervention,
            "Automatic retries exhausted",
        )),
        "policy_blocked" => Some((
            "policy_blocked",
            AttentionSeverity::Intervention,
            "Execution blocked by policy",
        )),
        "sandbox_denied" => Some((
            "sandbox_denied",
            AttentionSeverity::Intervention,
            "Execution denied by sandbox",
        )),
        "budget_threshold" | "budget_exhausted" => Some((
            "budget_threshold",
            AttentionSeverity::Attention,
            "Budget threshold reached",
        )),
        "step_timeout" | "task_stalled" => Some((
            "stalled",
            AttentionSeverity::Intervention,
            "Execution appears stalled",
        )),
        "task_failed" => Some((
            "task_failed",
            AttentionSeverity::Intervention,
            "Task failed",
        )),
        "degenerate_loop" | "degenerate_cycle" => Some((
            "degenerate_loop",
            AttentionSeverity::Intervention,
            "Workflow loop is not making progress",
        )),
        "step_failed" => Some((
            "step_failed",
            AttentionSeverity::Intervention,
            "Workflow step failed",
        )),
        "step_finished" | "chain_step_finished" | "dynamic_step_finished"
            if event.payload.get("success").and_then(Value::as_bool) == Some(false) =>
        {
            Some((
                "step_failed",
                AttentionSeverity::Intervention,
                "Workflow step failed",
            ))
        }
        _ => None,
    };

    let mut result = Vec::new();
    if let Some((kind, severity, title)) = policy {
        result.push(AttentionProjectionOp::Upsert(Box::new(candidate(
            event, kind, severity, title,
        ))));
    }
    if is_low_confidence(event) {
        result.push(AttentionProjectionOp::Upsert(Box::new(candidate(
            event,
            "low_confidence",
            AttentionSeverity::Attention,
            "Agent confidence needs review",
        ))));
    }
    result
}

fn candidate(
    event: &AttentionSourceEvent,
    kind: &str,
    severity: AttentionSeverity,
    title: &str,
) -> AttentionCandidate {
    let step_id = step_id(event);
    let session_id = safe_identifier(event.payload.get("session_id"));
    let dedupe_key = format!(
        "{}:{}:{}:{}:{}",
        event.project_id,
        event.task_id,
        event.task_item_id.as_deref().unwrap_or("-"),
        step_id.as_deref().unwrap_or("-"),
        kind
    );
    let id = digest(&dedupe_key);
    AttentionCandidate {
        id: format!("attn-{}", &id[..20]),
        project_id: event.project_id.clone(),
        task_id: event.task_id.clone(),
        task_item_id: event.task_item_id.clone(),
        step_id: step_id.clone(),
        session_id,
        kind: kind.to_owned(),
        severity,
        title: title.to_owned(),
        summary: structured_summary(event, step_id.as_deref()),
        requested_decision: decision_schema(kind),
        actions: actions_for(kind, event.task_item_id.is_some()),
        dedupe_key,
        source_event_id: event.id.to_string(),
        occurred_at: event.created_at.clone(),
        sla_deadline: None,
    }
}

fn structured_summary(event: &AttentionSourceEvent, step_id: Option<&str>) -> String {
    // Deliberately exclude event error/message/output fields. They can contain
    // prompts, transcripts, credentials, or arbitrary runner output.
    match step_id {
        Some(step) => format!("Task {} requires review at step {}.", event.task_id, step),
        None => format!("Task {} requires operator review.", event.task_id),
    }
}

fn actions_for(kind: &str, has_item: bool) -> Vec<AttentionActionDescriptor> {
    let mut actions = match kind {
        "approval_required" | "agent_question" => vec![
            action("approve_decision", "Approve", "operator", "required"),
            action("reject_decision", "Reject", "operator", "required"),
        ],
        "step_failed" | "retry_exhausted" if has_item => vec![action(
            "retry_failed_item",
            "Retry failed item",
            "operator",
            "required",
        )],
        "stalled" | "task_failed" => {
            vec![action("resume_task", "Resume task", "operator", "required")]
        }
        _ => Vec::new(),
    };
    actions.push(action("acknowledge", "Acknowledge", "operator", "none"));
    actions
}

fn action(
    id: &str,
    label: &str,
    required_role: &str,
    confirmation: &str,
) -> AttentionActionDescriptor {
    AttentionActionDescriptor {
        id: id.to_owned(),
        label: label.to_owned(),
        required_role: required_role.to_owned(),
        confirmation: confirmation.to_owned(),
        input_schema: json!({"type": "object", "additionalProperties": false}),
    }
}

fn decision_schema(kind: &str) -> Option<Value> {
    matches!(kind, "approval_required" | "agent_question").then(|| {
        json!({
            "type": "object",
            "properties": {"decision": {"enum": ["approve", "reject"]}},
            "required": ["decision"],
            "additionalProperties": false
        })
    })
}

fn step_id(event: &AttentionSourceEvent) -> Option<String> {
    ["step_id", "step", "step_name"]
        .into_iter()
        .find_map(|key| safe_identifier(event.payload.get(key)))
}

fn safe_identifier(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str()?;
    (!value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:/".contains(character)))
    .then(|| value.to_owned())
}

fn is_successful_step(event: &AttentionSourceEvent) -> bool {
    matches!(
        event.event_type.as_str(),
        "step_finished" | "chain_step_finished" | "dynamic_step_finished"
    ) && event.payload.get("success").and_then(Value::as_bool) != Some(false)
}

fn is_low_confidence(event: &AttentionSourceEvent) -> bool {
    matches!(
        event.event_type.as_str(),
        "step_finished" | "chain_step_finished" | "dynamic_step_finished"
    ) && event
        .payload
        .get("confidence")
        .and_then(Value::as_f64)
        .is_some_and(|confidence| confidence < 0.5)
}

fn digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(event_type: &str, payload: Value) -> AttentionSourceEvent {
        AttentionSourceEvent {
            id: 3,
            project_id: "default".into(),
            task_id: "task-1".into(),
            task_item_id: Some("item-1".into()),
            event_type: event_type.into(),
            payload,
            created_at: "2026-07-12T00:00:00Z".into(),
        }
    }

    #[test]
    fn failure_projection_never_copies_raw_error() {
        let operations = policy_operations(&event(
            "step_finished",
            json!({"success": false, "step_id": "build", "error": "token=secret"}),
        ));
        let AttentionProjectionOp::Upsert(candidate) = &operations[0] else {
            panic!("expected upsert");
        };
        assert_eq!(candidate.kind, "step_failed");
        assert!(!candidate.summary.contains("secret"));
        assert_eq!(candidate.step_id.as_deref(), Some("build"));
    }

    #[test]
    fn completed_task_resolves_active_items() {
        assert!(matches!(
            policy_operations(&event("task_completed", json!({})))[0],
            AttentionProjectionOp::ResolveTask { .. }
        ));
    }
}
