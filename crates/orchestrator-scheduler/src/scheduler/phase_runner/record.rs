use agent_orchestrator::config_load::now_ts;
use agent_orchestrator::driver::{DriverEvent, DriverOutcome};
use agent_orchestrator::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use agent_orchestrator::metrics::MetricsCollector;
use agent_orchestrator::state::InnerState;
use agent_orchestrator::task_repository::NewCommandRun;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use super::types::{PhaseSetup, ValidatedOutput};

/// Stage 5: Construct results, insert events, update metrics.
#[allow(clippy::too_many_arguments)]
pub(super) async fn record_phase_results(
    state: &Arc<InnerState>,
    setup: &PhaseSetup,
    validated: &ValidatedOutput,
    session_id: &Option<String>,
    driver_events: &[DriverEvent],
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    step_scope: agent_orchestrator::config::StepScope,
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
        command_template: setup.command_template.clone(),
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
        command_rule_index: None,
    };

    let validation_event_payload_json = validated.validation_event_payload_json.clone();
    {
        let mut events = Vec::with_capacity(2);
        if let Some(payload_json) = validation_event_payload_json {
            events.push(agent_orchestrator::db_write::DbEventRecord {
                task_id: task_id_owned.clone(),
                task_item_id: Some(item_id_owned.clone()),
                event_type: "output_validation_failed".to_string(),
                payload_json,
            });
        }
        if validated.sandbox_denied {
            let event_type = validated.sandbox_event_type.unwrap_or("sandbox_denied");
            events.push(agent_orchestrator::db_write::DbEventRecord {
                task_id: task_id_owned.clone(),
                task_item_id: Some(item_id_owned.clone()),
                event_type: event_type.to_string(),
                payload_json: serde_json::to_string(&json!({
                    "step": phase,
                    "step_id": step_id,
                    "step_scope": match step_scope {
                        agent_orchestrator::config::StepScope::Task => "task",
                        agent_orchestrator::config::StepScope::Item => "item",
                    },
                    "agent_id": agent_id,
                    "run_id": setup.run_id,
                    "execution_profile": setup.execution_profile.name,
                    "execution_mode": match setup.execution_profile.mode {
                        agent_orchestrator::config::ExecutionProfileMode::Host => "host",
                        agent_orchestrator::config::ExecutionProfileMode::Sandbox => "sandbox",
                    },
                    "reason_code": validated.sandbox_reason_code,
                    "reason": validated.sandbox_denial_reason,
                    "resource_kind": validated
                        .sandbox_resource_kind
                        .as_ref()
                        .map(|value| value.as_str()),
                    "network_target": validated.sandbox_network_target,
                    "stderr_excerpt": validated.sandbox_denial_stderr_excerpt,
                    "backend": agent_orchestrator::runner::sandbox_backend_label(&setup.execution_profile),
                }))?,
            });
        }
        // Project structured run records into events: one per tool call, plus a
        // run summary. Plain shell runs carry no such artifacts, so this is a
        // no-op for them.
        events.extend(project_stream_events(
            &validated.redacted_output.artifacts,
            task_id,
            item_id,
            phase,
            step_id,
            step_scope,
            agent_id,
            &setup.run_id,
        ));
        events.extend(project_driver_events(
            driver_events,
            task_id,
            item_id,
            phase,
            step_id,
            step_scope,
            agent_id,
            &setup.run_id,
            &setup.redaction_patterns,
        ));

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

    agent_orchestrator::agent_lifecycle::decrement_in_flight_and_check(state, agent_id).await;

    // Agent infrastructure failure — the agent itself could not function.
    // Distinct from "task conclusion is negative" (exit_code > 0) where
    // the agent completed its work correctly.
    let agent_infra_failed = validated.final_exit_code < 0
        || validated.sandbox_denied
        || validated.validation_status == "failed";

    if agent_infra_failed {
        // Resolve the agent's health policy: agent-level > workspace-level > global default.
        let health_policy = {
            let cfg_snap = state.config_runtime.load();
            let project = cfg_snap
                .active_config
                .config
                .projects
                .values()
                .find(|p| p.agents.contains_key(agent_id));
            let agent_policy = project
                .and_then(|p| p.agents.get(agent_id))
                .map(|a| &a.health_policy);
            if agent_policy.is_none_or(|p| p.is_default()) {
                // Agent has no explicit override — try workspace fallback.
                let ws_policy = project
                    .and_then(|p| p.workspaces.get(workspace_id))
                    .map(|ws| &ws.health_policy);
                ws_policy.cloned().unwrap_or_default()
            } else {
                agent_policy.cloned().unwrap_or_default()
            }
        };

        if health_policy.disease_duration_hours > 0 {
            let errors = increment_consecutive_errors(state, agent_id).await;
            if errors >= health_policy.disease_threshold {
                mark_agent_diseased(state, agent_id, &health_policy).await;
            }
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

/// Projects every normalized driver event into the canonical task event stream.
#[allow(clippy::too_many_arguments)]
pub(crate) fn project_driver_events(
    driver_events: &[DriverEvent],
    task_id: &str,
    item_id: &str,
    step: &str,
    step_id: &str,
    step_scope: agent_orchestrator::config::StepScope,
    agent_id: &str,
    run_id: &str,
    redaction_patterns: &[String],
) -> Vec<agent_orchestrator::db_write::DbEventRecord> {
    use agent_orchestrator::db_write::DbEventRecord;
    let step_scope = match step_scope {
        agent_orchestrator::config::StepScope::Task => "task",
        agent_orchestrator::config::StepScope::Item => "item",
    };
    driver_events
        .iter()
        .map(|event| {
            let (event_type, detail) = match event {
                DriverEvent::Started { session } => (
                    "driver_started",
                    json!({"session_available": session.is_some()}),
                ),
                DriverEvent::AssistantText(text) => (
                    "driver_assistant_text",
                    json!({"text": bounded_text(text, 16 * 1024)}),
                ),
                DriverEvent::ToolUse {
                    call_id,
                    name,
                    args,
                } => (
                    "driver_tool_use",
                    json!({"call_id": call_id, "name": name, "args": args}),
                ),
                DriverEvent::ToolResult {
                    call_id,
                    payload,
                    is_error,
                } => (
                    "driver_tool_result",
                    json!({"call_id": call_id, "payload": payload, "is_error": is_error}),
                ),
                DriverEvent::PermissionRequested { request_id, scope } => (
                    "approval_requested",
                    json!({
                        "request_id": request_id,
                        "permission_kind": scope.kind,
                        "scope": scope.detail,
                    }),
                ),
                DriverEvent::Usage { cost_usd, tokens } => (
                    "driver_usage",
                    json!({
                        "cost_usd": cost_usd,
                        "input_tokens": tokens.input,
                        "output_tokens": tokens.output,
                    }),
                ),
                DriverEvent::Finished { outcome, exit_code } => (
                    "driver_finished",
                    json!({
                        "outcome": match outcome {
                            DriverOutcome::Success => "success",
                            DriverOutcome::Failed => "failed",
                            DriverOutcome::Cancelled => "cancelled",
                        },
                        "exit_code": exit_code,
                    }),
                ),
            };
            let payload = json!({
                "step": step,
                "step_id": step_id,
                "step_scope": step_scope,
                "agent_id": agent_id,
                "run_id": run_id,
                "detail": detail,
            });
            DbEventRecord {
                task_id: task_id.to_string(),
                task_item_id: Some(item_id.to_string()),
                event_type: event_type.to_string(),
                payload_json: agent_orchestrator::runner::redact_text(
                    &payload.to_string(),
                    redaction_patterns,
                ),
            }
        })
        .collect()
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Projects stream-json structured artifacts (from the streaming agent runner)
/// into `events` rows: one `agent_tool_call` per tool call, an `agent_run_summary`,
/// and `stream_truncated` when the stream was cut off before its result event.
///
/// Non-streaming runs carry no such artifacts, so this returns an empty vec.
#[allow(clippy::too_many_arguments)]
pub(crate) fn project_stream_events(
    artifacts: &[agent_orchestrator::collab::Artifact],
    task_id: &str,
    item_id: &str,
    step: &str,
    step_id: &str,
    step_scope: agent_orchestrator::config::StepScope,
    agent_id: &str,
    run_id: &str,
) -> Vec<agent_orchestrator::db_write::DbEventRecord> {
    use agent_orchestrator::collab::ArtifactKind;
    use agent_orchestrator::db_write::DbEventRecord;

    let step_scope_str = match step_scope {
        agent_orchestrator::config::StepScope::Task => "task",
        agent_orchestrator::config::StepScope::Item => "item",
    };
    let mut events = Vec::new();

    for artifact in artifacts {
        match &artifact.kind {
            ArtifactKind::ToolCall { tool } => {
                events.push(DbEventRecord {
                    task_id: task_id.to_string(),
                    task_item_id: Some(item_id.to_string()),
                    event_type: "agent_tool_call".to_string(),
                    payload_json: json!({
                        "step": step,
                        "step_id": step_id,
                        "step_scope": step_scope_str,
                        "agent_id": agent_id,
                        "run_id": run_id,
                        "tool": tool,
                        "detail": artifact.content,
                    })
                    .to_string(),
                });
            }
            ArtifactKind::Data { schema } if schema.as_str() == "stream_run_summary" => {
                let summary = artifact.content.clone().unwrap_or(serde_json::Value::Null);
                events.push(DbEventRecord {
                    task_id: task_id.to_string(),
                    task_item_id: Some(item_id.to_string()),
                    event_type: "agent_run_summary".to_string(),
                    payload_json: json!({
                        "step": step,
                        "step_id": step_id,
                        "step_scope": step_scope_str,
                        "agent_id": agent_id,
                        "run_id": run_id,
                        "summary": summary,
                    })
                    .to_string(),
                });
                if summary.get("truncated").and_then(|v| v.as_bool()) == Some(true) {
                    events.push(DbEventRecord {
                        task_id: task_id.to_string(),
                        task_item_id: Some(item_id.to_string()),
                        event_type: "stream_truncated".to_string(),
                        payload_json: json!({
                            "step": step,
                            "step_id": step_id,
                            "run_id": run_id,
                        })
                        .to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::{project_driver_events, project_stream_events};
    use agent_orchestrator::config::StepScope;
    use agent_orchestrator::driver::{DriverEvent, DriverOutcome, PermissionScope, SessionRef};
    use agent_orchestrator::output_validation::validate_phase_output;
    use agent_orchestrator::test_utils::TestState;
    use orchestrator_persistence::test_support::open_conn;
    use rusqlite::params;

    // A compact stream-json run: the orchestrator MCP tool call + terminal result.
    const STREAM: &str = concat!(
        r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__orch__run_tests","input":{"target":"core"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"{\"failed\":1,\"failures\":[\"core::selection::picks_healthy_agent\"]}"}]}]}}"#,
        "\n",
        r#"{"type":"result","is_error":false,"result":"1 failed","num_turns":3,"total_cost_usd":0.02,"session_id":"s1"}"#,
        "\n",
    );

    #[tokio::test]
    async fn streaming_run_projects_tool_call_events_into_db() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // Real chain: parse the stream into artifacts, then project events.
        let outcome =
            validate_phase_output("implement", uuid::Uuid::new_v4(), "streamer", 0, STREAM, "")
                .expect("validate streaming output");
        let events = project_stream_events(
            &outcome.output.artifacts,
            "task-stream-events",
            "item-1",
            "implement",
            "implement",
            StepScope::Item,
            "streamer",
            "run-1",
        );

        // Persist through the real events-insert path (promotes step/scope/cycle).
        for event in &events {
            state
                .db_writer
                .insert_event(
                    &event.task_id,
                    event.task_item_id.as_deref(),
                    &event.event_type,
                    &event.payload_json,
                )
                .await
                .expect("insert projected event");
        }

        // Read back and assert on payload + the promoted `step` column.
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let rows: Vec<(String, String, Option<String>)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT event_type, payload_json, step FROM events
                     WHERE task_id = 'task-stream-events'
                       AND event_type IN ('agent_tool_call', 'agent_run_summary')
                     ORDER BY id",
                )
                .expect("prepare events query");
            stmt.query_map(params![], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .expect("query events")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect events")
        };

        let tool_call = rows
            .iter()
            .find(|(ty, _, _)| ty == "agent_tool_call")
            .expect("agent_tool_call row present");
        let payload: serde_json::Value =
            serde_json::from_str(&tool_call.1).expect("parse tool_call payload");
        assert_eq!(payload["tool"], "mcp__orch__run_tests");
        assert_eq!(
            tool_call.2.as_deref(),
            Some("implement"),
            "step should be promoted into its column"
        );

        let summary = rows
            .iter()
            .find(|(ty, _, _)| ty == "agent_run_summary")
            .expect("agent_run_summary row present");
        let summary_payload: serde_json::Value =
            serde_json::from_str(&summary.1).expect("parse summary payload");
        assert_eq!(summary_payload["summary"]["num_turns"], 3);
        assert_eq!(summary_payload["summary"]["num_tool_calls"], 1);
    }

    #[test]
    fn driver_projection_is_complete_redacted_and_session_opaque() {
        let secret = "provider-session-secret";
        let events = vec![
            DriverEvent::Started {
                session: Some(SessionRef::from_provider(secret.to_string()).expect("session")),
            },
            DriverEvent::ToolUse {
                call_id: "tool-1".to_string(),
                name: "run_tests".to_string(),
                args: serde_json::json!({"token":"sensitive-value"}),
            },
            DriverEvent::PermissionRequested {
                request_id: "permission-1".to_string(),
                scope: PermissionScope {
                    kind: "workspace_write".to_string(),
                    detail: serde_json::json!({"path":"src/lib.rs"}),
                },
            },
            DriverEvent::Finished {
                outcome: DriverOutcome::Success,
                exit_code: 0,
            },
        ];
        let projected = project_driver_events(
            &events,
            "task-1",
            "item-1",
            "implement",
            "implement",
            StepScope::Item,
            "driver-agent",
            "run-1",
            &["sensitive-value".to_string()],
        );
        assert_eq!(projected.len(), events.len());
        assert!(
            projected
                .iter()
                .any(|event| event.event_type == "approval_requested")
        );
        let joined = projected
            .iter()
            .map(|event| event.payload_json.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains(secret));
        assert!(!joined.contains("sensitive-value"));
        assert!(joined.contains("[REDACTED]"));
    }
}
