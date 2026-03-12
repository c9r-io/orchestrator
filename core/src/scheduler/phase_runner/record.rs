use crate::collab;
use crate::config_load::now_ts;
use crate::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use crate::metrics::MetricsCollector;
use crate::state::InnerState;
use crate::task_repository::NewCommandRun;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use super::types::{PhaseSetup, ValidatedOutput};

/// Stage 5: Construct results, publish to message bus, insert events, update metrics.
#[allow(clippy::too_many_arguments)]
pub(super) async fn record_phase_results(
    state: &Arc<InnerState>,
    setup: &PhaseSetup,
    validated: &ValidatedOutput,
    session_id: &Option<String>,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    step_scope: crate::config::StepScope,
    tty: bool,
    workspace_root: &Path,
    workspace_id: &str,
    agent_id: &str,
    duration: std::time::Duration,
) -> Result<()> {
    let writer = state.db_writer.clone();
    let task_id_owned = task_id.to_string();
    let item_id_owned = item_id.to_string();
    let insert_payload = NewCommandRun {
        id: setup.run_id.clone(),
        task_item_id: item_id.to_string(),
        phase: phase.to_string(),
        command: setup.command.clone(),
        cwd: workspace_root.to_string_lossy().to_string(),
        workspace_id: workspace_id.to_string(),
        agent_id: agent_id.to_string(),
        exit_code: validated.final_exit_code,
        stdout_path: setup.stdout_path.to_string_lossy().to_string(),
        stderr_path: setup.stderr_path.to_string_lossy().to_string(),
        started_at: setup.now.clone(),
        ended_at: now_ts(),
        interrupted: 0,
        output_json: serde_json::to_string(&validated.redacted_output)?,
        artifacts_json: serde_json::to_string(&validated.redacted_output.artifacts)?,
        confidence: Some(validated.redacted_output.confidence),
        quality_score: Some(validated.redacted_output.quality_score),
        validation_status: validated.validation_status.to_string(),
        session_id: session_id.clone(),
        machine_output_source: if tty {
            "output_json_path".to_string()
        } else {
            "stdout".to_string()
        },
        output_json_path: session_id
            .as_ref()
            .map(|sid| {
                state
                    .logs_dir
                    .join("sessions")
                    .join(sid)
                    .join("output.json")
            })
            .map(|p| p.to_string_lossy().to_string()),
    };
    let sender = collab::AgentEndpoint::for_task_item(agent_id, task_id, item_id);
    let msg = collab::AgentMessage::publish(
        sender,
        collab::MessagePayload::ExecutionResult(collab::ExecutionResult {
            run_id: setup.run_uuid,
            output: validated.redacted_output.clone(),
            success: validated.success,
            error: validated.validation_error.clone(),
        }),
    );
    let (publish_event_type, publish_event_payload_json) =
        if let Err(err) = state.message_bus.publish(msg).await {
            (
                "bus_publish_failed",
                serde_json::to_string(
                    &json!({"phase":phase,"run_id":setup.run_id,"error":err.to_string()}),
                )?,
            )
        } else {
            (
                "phase_output_published",
                serde_json::to_string(&json!({"phase":phase,"run_id":setup.run_id}))?,
            )
        };

    let validation_event_payload_json = validated.validation_event_payload_json.clone();
    {
        let mut events = Vec::with_capacity(3);
        if let Some(payload_json) = validation_event_payload_json {
            events.push(crate::db_write::DbEventRecord {
                task_id: task_id_owned.clone(),
                task_item_id: Some(item_id_owned.clone()),
                event_type: "output_validation_failed".to_string(),
                payload_json,
            });
        }
        if validated.sandbox_denied {
            let event_type = validated.sandbox_event_type.unwrap_or("sandbox_denied");
            events.push(crate::db_write::DbEventRecord {
                task_id: task_id_owned.clone(),
                task_item_id: Some(item_id_owned.clone()),
                event_type: event_type.to_string(),
                payload_json: serde_json::to_string(&json!({
                    "step": phase,
                    "step_id": step_id,
                    "step_scope": match step_scope {
                        crate::config::StepScope::Task => "task",
                        crate::config::StepScope::Item => "item",
                    },
                    "agent_id": agent_id,
                    "run_id": setup.run_id,
                    "execution_profile": setup.execution_profile.name,
                    "execution_mode": match setup.execution_profile.mode {
                        crate::config::ExecutionProfileMode::Host => "host",
                        crate::config::ExecutionProfileMode::Sandbox => "sandbox",
                    },
                    "reason_code": validated.sandbox_reason_code,
                    "reason": validated.sandbox_denial_reason,
                    "resource_kind": validated
                        .sandbox_resource_kind
                        .as_ref()
                        .map(|value| value.as_str()),
                    "network_target": validated.sandbox_network_target,
                    "stderr_excerpt": validated.sandbox_denial_stderr_excerpt,
                    "backend": crate::runner::sandbox_backend_label(&setup.execution_profile),
                }))?,
            });
        }
        events.push(crate::db_write::DbEventRecord {
            task_id: task_id_owned,
            task_item_id: Some(item_id_owned),
            event_type: publish_event_type.to_string(),
            payload_json: publish_event_payload_json,
        });
        writer
            .update_command_run_with_owned_events(insert_payload, events)
            .await?;
    }

    update_capability_health(state, agent_id, Some(phase), validated.success).await;

    let duration_ms = duration.as_millis() as u64;
    {
        let mut metrics_map = state.agent_metrics.write().await;
        let metrics = metrics_map
            .entry(agent_id.to_string())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        if validated.success {
            MetricsCollector::record_success(metrics, duration_ms);
        } else {
            MetricsCollector::record_failure(metrics);
        }
        MetricsCollector::decrement_load(metrics);
    }

    crate::agent_lifecycle::decrement_in_flight_and_check(state, agent_id).await;

    if !validated.success {
        let errors = increment_consecutive_errors(state, agent_id).await;
        if errors >= 2 {
            mark_agent_diseased(state, agent_id).await;
        }
    } else {
        reset_consecutive_errors(state, agent_id).await;
    }
    if let Some(sid) = session_id.as_deref() {
        let _ = state
            .session_store
            .update_session_state(sid, "closed", Some(validated.final_exit_code), true)
            .await;
    }

    Ok(())
}
