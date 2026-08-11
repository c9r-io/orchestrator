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

/// Executes one allowlisted attention action through the shared service path
/// used by gRPC, GUI/CLI clients, and verified external source adapters.
pub async fn execute_allowlisted_action(
    state: &InnerState,
    attention_item_id: &str,
    expected_version: i64,
    idempotency_key: &str,
    actor: &str,
    action_id: &str,
    input: &Value,
) -> Result<agent_orchestrator::attention::AttentionItem> {
    let item = state
        .attention_repo
        .get(attention_item_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("attention item not found"))?;
    if !item.actions.iter().any(|action| action.id == action_id) {
        anyhow::bail!("action is not allowlisted for this item");
    }
    if !matches!(
        action_id,
        "retry_failed_item"
            | "resume_task"
            | "approve_decision"
            | "reject_decision"
            | "acknowledge"
    ) {
        anyhow::bail!("unsupported action");
    }

    let reservation = state
        .attention_repo
        .reserve_action(
            attention_item_id,
            expected_version,
            idempotency_key,
            actor,
            action_id,
            input,
        )
        .await?;
    if !reservation.should_execute {
        return Ok(reservation.item);
    }

    let execution = match action_id {
        "retry_failed_item" => {
            if let Some(item_id) = item.task_item_id.as_deref() {
                match super::task::retry_task_item(state, item_id) {
                    Ok(task_id) => super::task::enqueue_task(state, &task_id)
                        .await
                        .map_err(Into::into),
                    Err(error) => Err(error.into()),
                }
            } else {
                Err(anyhow::anyhow!("item has no failed task item"))
            }
        }
        "resume_task" => super::task::enqueue_task(state, &item.task_id)
            .await
            .map_err(Into::into),
        "approve_decision" | "reject_decision" | "acknowledge" => Ok(()),
        _ => unreachable!("supported action validated before reservation"),
    };
    let error_code = execution.as_ref().err().map(|error| error.to_string());
    let completed = state
        .attention_repo
        .complete_action(
            attention_item_id,
            idempotency_key,
            actor,
            action_id,
            error_code.as_deref(),
        )
        .await?;
    execution?;
    Ok(completed)
}

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

/// Evidence kinds that record something which already happened and awaits
/// human review; the task-completion sweep must not clear them (FR-162).
/// `resume_executed` still sweeps everything: resume is an operator action
/// typically taken from the evidence item itself.
const TASK_SWEEP_PRESERVED_KINDS: &[&str] = &["step_failed", "low_confidence", "task_spawn_failed"];

fn policy_operations(event: &AttentionSourceEvent) -> Vec<AttentionProjectionOp> {
    if matches!(
        event.event_type.as_str(),
        "task_completed" | "task_finished"
    ) {
        return vec![AttentionProjectionOp::ResolveTask {
            task_id: event.task_id.clone(),
            source_event_id: event.id.to_string(),
            preserve_kinds: TASK_SWEEP_PRESERVED_KINDS
                .iter()
                .map(|kind| kind.to_string())
                .collect(),
            reason: "task_completed".to_string(),
        }];
    }
    if event.event_type == "resume_executed" {
        return vec![AttentionProjectionOp::ResolveTask {
            task_id: event.task_id.clone(),
            source_event_id: event.id.to_string(),
            preserve_kinds: Vec::new(),
            reason: "condition_cleared".to_string(),
        }];
    }

    if is_successful_step(event) {
        let mut operations = Vec::new();
        if let Some(step_id) = step_id(event) {
            operations.push(AttentionProjectionOp::ResolveStep {
                task_id: event.task_id.clone(),
                task_item_id: event.task_item_id.clone(),
                step_id,
                source_event_id: event.id.to_string(),
            });
        }
        if is_low_confidence(event) {
            operations.push(AttentionProjectionOp::Upsert(Box::new(candidate(
                event,
                "low_confidence",
                AttentionSeverity::Attention,
                "Agent confidence needs review",
            ))));
        }
        return operations;
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
        "sandbox_denied" | "sandbox_network_blocked" | "sandbox_resource_exceeded" => Some((
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
        "degenerate_loop" | "degenerate_cycle" | "degenerate_cycle_detected" => Some((
            "degenerate_loop",
            AttentionSeverity::Intervention,
            "Workflow loop is not making progress",
        )),
        "step_failed" | "output_validation_failed" => Some((
            "step_failed",
            AttentionSeverity::Intervention,
            "Workflow step failed",
        )),
        "task_spawn_failed" => Some((
            "task_spawn_failed",
            AttentionSeverity::Intervention,
            "Child task creation failed",
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
        source_route_id: None,
        source_binding_name: None,
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
    fn completed_task_preserves_failure_evidence() {
        let operations = policy_operations(&event("task_completed", json!({})));
        let AttentionProjectionOp::ResolveTask {
            preserve_kinds,
            reason,
            ..
        } = &operations[0]
        else {
            panic!("expected resolve-task");
        };
        assert_eq!(
            preserve_kinds,
            &["step_failed", "low_confidence", "task_spawn_failed"]
        );
        assert_eq!(reason, "task_completed");
    }

    #[test]
    fn successful_low_confidence_step_resolves_stale_step_and_opens_review() {
        let operations = policy_operations(&event(
            "step_finished",
            json!({"success": true, "step_id": "prepare_reply", "confidence": 0.4}),
        ));
        assert!(matches!(
            operations.first(),
            Some(AttentionProjectionOp::ResolveStep { .. })
        ));
        let Some(AttentionProjectionOp::Upsert(candidate)) = operations.get(1) else {
            panic!("expected low-confidence review item");
        };
        assert_eq!(candidate.kind, "low_confidence");
    }

    #[test]
    fn executed_resume_resolves_only_after_durable_state_change_event() {
        assert!(matches!(
            policy_operations(&event("resume_executed", json!({})))[0],
            AttentionProjectionOp::ResolveTask { .. }
        ));
        assert!(policy_operations(&event("resume_planned", json!({}))).is_empty());
    }

    #[test]
    fn degenerate_cycle_detected_routes_as_emitted() {
        // The arm previously spelled only "degenerate_cycle" while the loop
        // engine emits "degenerate_cycle_detected"; it never fired (FR-162).
        let operations = policy_operations(&event("degenerate_cycle_detected", json!({})));
        let AttentionProjectionOp::Upsert(candidate) = &operations[0] else {
            panic!("expected upsert");
        };
        assert_eq!(candidate.kind, "degenerate_loop");
    }

    #[test]
    fn sandbox_siblings_route_to_sandbox_denied() {
        for event_type in ["sandbox_network_blocked", "sandbox_resource_exceeded"] {
            let operations = policy_operations(&event(event_type, json!({})));
            let AttentionProjectionOp::Upsert(candidate) = &operations[0] else {
                panic!("expected upsert for {event_type}");
            };
            assert_eq!(candidate.kind, "sandbox_denied");
        }
    }

    #[test]
    fn output_validation_failure_materializes_step_failed() {
        let operations = policy_operations(&event(
            "output_validation_failed",
            json!({"step_id": "build", "error": "token=secret"}),
        ));
        let AttentionProjectionOp::Upsert(candidate) = &operations[0] else {
            panic!("expected upsert");
        };
        assert_eq!(candidate.kind, "step_failed");
        assert_eq!(candidate.step_id.as_deref(), Some("build"));
        assert!(!candidate.summary.contains("secret"));
    }

    #[test]
    fn task_spawn_failure_routes_as_preserved_evidence() {
        let operations = policy_operations(&event(
            "task_spawn_failed",
            json!({"reason_code": "depth_limit"}),
        ));
        let AttentionProjectionOp::Upsert(candidate) = &operations[0] else {
            panic!("expected upsert");
        };
        assert_eq!(candidate.kind, "task_spawn_failed");
        assert!(TASK_SWEEP_PRESERVED_KINDS.contains(&candidate.kind.as_str()));
    }

    #[test]
    fn resumed_task_sweeps_evidence() {
        let operations = policy_operations(&event("resume_executed", json!({})));
        let AttentionProjectionOp::ResolveTask {
            preserve_kinds,
            reason,
            ..
        } = &operations[0]
        else {
            panic!("expected resolve-task");
        };
        assert!(preserve_kinds.is_empty());
        assert_eq!(reason, "condition_cleared");
    }
}
