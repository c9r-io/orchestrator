use crate::scheduler::spawn::{
    SpawnContext, execute_spawn_task, execute_spawn_tasks, validate_spawn_depth,
};
use agent_orchestrator::config::{
    OnFailureAction, OnSuccessAction, PostAction, TaskExecutionStep, TaskRuntimeContext,
};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::state::InnerState;
use agent_orchestrator::store::StoreOp;
use agent_orchestrator::ticket::{
    create_ticket_for_qa_failure, scan_active_tickets_for_task_items,
};
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};

use super::accumulator::StepExecutionAccumulator;
use super::dispatch_builtin::is_execution_hard_failure;

/// Apply step results: capture outputs, status transitions, post-actions,
/// artifact collection, confidence/quality scores, and event emission.
/// Returns `true` if the caller should return early (terminal state).
#[allow(clippy::too_many_arguments)]
pub(super) async fn apply_step_results(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    step: &TaskExecutionStep,
    finish_event_type: &str,
    parent_step: Option<&str>,
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
    qa_file_path: &str,
    result: &agent_orchestrator::dto::RunResult,
    acc: &mut StepExecutionAccumulator,
) -> Result<bool> {
    // 3. Capture outputs
    acc.exit_codes.insert(step.id.clone(), result.exit_code);
    let captures_missing = acc.apply_captures(
        &step.behavior.captures,
        &task_ctx.artifacts_dir,
        task_id,
        &step.id,
        result,
    );
    acc.step_ran.insert(step.id.clone(), true);
    acc.apply_run_diagnostics(result);
    apply_coordination_tool_effects(result, acc);

    // Inject streaming-run signals (tools_called, tool_error_count, run_cost_usd, …)
    // as typed pipeline vars so prehook / convergence / finalize CEL can drive
    // coordination from what the agent did. Empty for non-streaming runs.
    if let Some(output) = result.output.as_ref()
        && !uses_coordination_tool_model(&output.artifacts)
    {
        for (key, value) in agent_orchestrator::stream_json::stream_signal_vars(&output.artifacts) {
            acc.pipeline_vars.vars.insert(key, value);
        }
    }

    // 4. Status transitions
    if result.is_success() {
        if let OnSuccessAction::SetStatus { status } = &step.behavior.on_success {
            acc.item_status = status.clone();
        }
    } else {
        match &step.behavior.on_failure {
            OnFailureAction::Continue => {}
            OnFailureAction::SetStatus { status } => {
                acc.item_status = status.clone();
            }
            OnFailureAction::EarlyReturn { status } => {
                acc.item_status = status.clone();
                acc.terminal = true;
                let mut payload = json!({
                    "step": phase,
                    "step_id": step.id,
                    "step_scope": step.resolved_scope(),
                    "agent_id": result.agent_id,
                    "run_id": result.run_id,
                    "early_return": true,
                    "exit_code": result.exit_code,
                    "success": false,
                    "cycle": task_ctx.current_cycle,
                    "execution_profile": result.execution_profile,
                    "execution_mode": result.execution_mode,
                    "sandbox_denied": result.sandbox_denied,
                    "sandbox_denial_reason": result.sandbox_denial_reason,
                    "sandbox_violation_kind": result.sandbox_violation_kind,
                    "sandbox_resource_kind": result.sandbox_resource_kind,
                    "sandbox_network_target": result.sandbox_network_target,
                });
                if let Some(parent_step) = parent_step {
                    payload["parent_step"] = json!(parent_step);
                }
                insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;
                return Ok(true);
            }
        }
    }

    // 5. Post-actions
    for action in &step.behavior.post_actions {
        match action {
            PostAction::CreateTicket if !result.is_success() => {
                if let Some(exit_code) = acc.exit_codes.get(&step.id) {
                    let task_name = state
                        .task_repo
                        .load_task_name(task_id)
                        .await?
                        .unwrap_or_else(|| task_id.to_string());
                    match create_ticket_for_qa_failure(
                        &task_ctx.workspace_root,
                        &task_ctx.ticket_dir,
                        &task_name,
                        qa_file_path,
                        *exit_code,
                        &result.stdout_path,
                        &result.stderr_path,
                    ) {
                        Ok(Some(ticket_path)) => {
                            acc.created_ticket_files.push(ticket_path.clone());
                            acc.active_tickets.push(ticket_path.clone());
                            insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "ticket_created",
                                json!({"path": ticket_path, "qa_file": qa_file_path}),
                            )
                            .await?;
                        }
                        Ok(None) => {}
                        Err(e) => warn!(error = %e, "failed to auto-create ticket"),
                    }
                }
            }
            PostAction::ScanTickets => {
                let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                acc.active_tickets = tickets.get(qa_file_path).cloned().unwrap_or_default();
                acc.new_ticket_count = acc.active_tickets.len() as i64;
            }
            PostAction::SpawnTask(spawn_action) if result.is_success() => {
                if let Err(e) =
                    validate_spawn_depth(task_ctx.spawn_depth, task_ctx.safety.max_spawn_depth)
                {
                    warn!(error = %e, "spawn_task skipped: depth limit");
                } else {
                    let spawn_ctx = SpawnContext {
                        state,
                        parent_task_id: task_id,
                        parent_project_id: &task_ctx.project_id,
                        parent_workspace_id: &task_ctx.workspace_id,
                        parent_workflow_id: &task_ctx.workflow_id,
                        parent_spawn_depth: task_ctx.spawn_depth,
                        pipeline_vars: &acc.pipeline_vars.vars,
                    };
                    match execute_spawn_task(&spawn_ctx, spawn_action) {
                        Ok(child_id) => {
                            insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "task_spawned",
                                json!({"child_task_id": child_id}),
                            )
                            .await?;
                        }
                        Err(e) => warn!(error = %e, "spawn_task failed"),
                    }
                }
            }
            PostAction::SpawnTasks(spawn_action) if result.is_success() => {
                if let Err(e) =
                    validate_spawn_depth(task_ctx.spawn_depth, task_ctx.safety.max_spawn_depth)
                {
                    warn!(error = %e, "spawn_tasks skipped: depth limit");
                } else {
                    let spawn_ctx = SpawnContext {
                        state,
                        parent_task_id: task_id,
                        parent_project_id: &task_ctx.project_id,
                        parent_workspace_id: &task_ctx.workspace_id,
                        parent_workflow_id: &task_ctx.workflow_id,
                        parent_spawn_depth: task_ctx.spawn_depth,
                        pipeline_vars: &acc.pipeline_vars.vars,
                    };
                    match execute_spawn_tasks(&spawn_ctx, spawn_action) {
                        Ok(child_ids) => {
                            info!(count = child_ids.len(), "spawned batch tasks");
                            insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "tasks_spawned",
                                json!({"child_task_ids": child_ids}),
                            )
                            .await?;
                        }
                        Err(e) => warn!(error = %e, "spawn_tasks failed"),
                    }
                }
            }
            PostAction::GenerateItems(gen_action) => {
                // Buffer for application after segment completes
                tracing::info!(
                    from_var = %gen_action.from_var,
                    json_path = %gen_action.json_path,
                    replace = gen_action.replace,
                    "buffering GenerateItems post-action"
                );
                acc.pending_generate_items = Some(gen_action.clone());
            }
            PostAction::StorePut {
                store,
                key,
                from_var,
            } => {
                if let Some(value) = acc.pipeline_vars.vars.get(from_var).cloned() {
                    if let Err(e) =
                        execute_store_put(state, task_ctx, task_id, store, key, &value).await
                    {
                        warn!(error = %e, store = %store, key = %key, "StorePut post-action failed");
                    }
                } else {
                    warn!(from_var = %from_var, "StorePut: pipeline var not found");
                }
            }
            _ => {}
        }
    }

    // Process store_outputs declarations
    process_store_outputs(state, task_ctx, task_id, step, acc).await;

    // 6. Collect artifacts. Convergence signals are control-plane input and
    // must survive even when user-facing artifact collection is disabled.
    let step_artifacts = result
        .output
        .as_ref()
        .map(|o| {
            o.artifacts
                .iter()
                .filter(|artifact| {
                    step.behavior.collect_artifacts
                        || matches!(
                            &artifact.kind,
                            agent_orchestrator::collab::ArtifactKind::ToolCall { tool }
                                if tool == "mark_done" || tool.ends_with("__mark_done")
                        )
                        || matches!(
                            &artifact.kind,
                            agent_orchestrator::collab::ArtifactKind::Data { schema }
                                if schema == "driver_terminal"
                        )
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !step_artifacts.is_empty() {
        if step.behavior.collect_artifacts {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "artifacts_parsed",
                json!({"step": phase, "count": step_artifacts.len()}),
            )
            .await?;
        }
        acc.phase_artifacts.extend(step_artifacts);
    }

    // Also check for ticket artifacts that may seed active_tickets
    if acc.active_tickets.is_empty() {
        let ticket_artifact_count = acc
            .phase_artifacts
            .iter()
            .filter(|a| {
                matches!(
                    a.kind,
                    agent_orchestrator::collab::ArtifactKind::Ticket { .. }
                )
            })
            .count();
        if ticket_artifact_count > 0 {
            acc.active_tickets = (0..ticket_artifact_count)
                .map(|idx| format!("artifact://ticket/{idx}"))
                .collect();
            acc.new_ticket_count = acc.active_tickets.len() as i64;
        }
    }

    let confidence = result.output.as_ref().map(|o| o.confidence).unwrap_or(0.0);
    let quality = result
        .output
        .as_ref()
        .map(|o| o.quality_score)
        .unwrap_or(0.0);

    match phase {
        "qa" | "qa_testing" => {
            acc.qa_confidence = Some(confidence);
            acc.qa_quality_score = Some(quality);
        }
        "fix" | "ticket_fix" => {
            acc.fix_confidence = Some(confidence);
            acc.fix_quality_score = Some(quality);
        }
        _ => {}
    }

    let mut payload = json!({
        "step": phase,
        "step_id": step.id,
        "step_scope": step.resolved_scope(),
        "agent_id": result.agent_id,
        "run_id": result.run_id,
        "exit_code": result.exit_code,
        "success": result.is_success(),
        "timed_out": result.timed_out,
        "duration_ms": result.duration_ms,
        "cycle": task_ctx.current_cycle,
        "build_errors": acc.pipeline_vars.build_errors.len(),
        "test_failures": acc.pipeline_vars.test_failures.len(),
        "confidence": confidence,
        "quality_score": quality,
        "validation_status": result.validation_status,
        "execution_profile": result.execution_profile,
        "execution_mode": result.execution_mode,
        "sandbox_denied": result.sandbox_denied,
        "sandbox_denial_reason": result.sandbox_denial_reason,
        "sandbox_violation_kind": result.sandbox_violation_kind,
        "sandbox_resource_kind": result.sandbox_resource_kind,
        "sandbox_network_target": result.sandbox_network_target,
    });
    if let Some(parent_step) = parent_step {
        payload["parent_step"] = json!(parent_step);
    }
    if !captures_missing.is_empty() {
        payload["captures_missing"] = json!(captures_missing);
    }
    insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

    if is_execution_hard_failure(result) {
        acc.item_status = "unresolved".to_string();
        acc.flags.insert("execution_failed".to_string(), true);
        acc.terminal = true;
        return Ok(true);
    }

    Ok(false)
}

/// Folds authenticated coordination-tool receipts into the same accumulator
/// used by legacy post-actions. The daemon already performed or validated the
/// operation; this step keeps scheduler-local state aligned before finalization.
fn apply_coordination_tool_effects(
    result: &agent_orchestrator::dto::RunResult,
    acc: &mut StepExecutionAccumulator,
) {
    use agent_orchestrator::collab::ArtifactKind;
    use std::collections::HashMap;

    let Some(output) = result.output.as_ref() else {
        return;
    };
    let mut calls = HashMap::new();
    for artifact in &output.artifacts {
        if let ArtifactKind::ToolCall { tool } = &artifact.kind
            && let Some(call_id) = artifact
                .content
                .as_ref()
                .and_then(|content| content.get("call_id"))
                .and_then(serde_json::Value::as_str)
        {
            calls.insert(call_id.to_string(), bare_tool_name(tool));
        }
    }
    for artifact in &output.artifacts {
        let ArtifactKind::Data { schema } = &artifact.kind else {
            continue;
        };
        if schema != "driver_tool_result" {
            continue;
        }
        let Some(content) = artifact.content.as_ref() else {
            continue;
        };
        if content
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(call_id) = content.get("call_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(tool) = calls.get(call_id) else {
            continue;
        };
        let Some(receipt) = content.get("payload").and_then(parse_tool_result_payload) else {
            continue;
        };
        match tool.as_str() {
            "mark_item" | "mark_done"
                if receipt
                    .get("accepted")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false) =>
            {
                if let Some(status) = receipt.get("status").and_then(serde_json::Value::as_str) {
                    acc.item_status = status.to_string();
                }
            }
            "create_ticket" => {
                if let Some(path) = receipt.get("path").and_then(serde_json::Value::as_str) {
                    if !acc.created_ticket_files.iter().any(|value| value == path) {
                        acc.created_ticket_files.push(path.to_string());
                    }
                    if !acc.active_tickets.iter().any(|value| value == path) {
                        acc.active_tickets.push(path.to_string());
                    }
                    acc.new_ticket_count = acc.active_tickets.len() as i64;
                }
            }
            "scan_tickets" => {
                if let Some(tickets) = receipt.get("tickets").and_then(serde_json::Value::as_array)
                {
                    acc.active_tickets = tickets
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect();
                    acc.new_ticket_count = acc.active_tickets.len() as i64;
                }
            }
            _ => {}
        }
    }
}

fn bare_tool_name(tool: &str) -> String {
    tool.strip_prefix("mcp__orch__").unwrap_or(tool).to_string()
}

fn uses_coordination_tool_model(artifacts: &[agent_orchestrator::collab::Artifact]) -> bool {
    use agent_orchestrator::collab::ArtifactKind;
    artifacts.iter().any(|artifact| {
        let ArtifactKind::ToolCall { tool } = &artifact.kind else {
            return false;
        };
        matches!(
            bare_tool_name(tool).as_str(),
            "run_tests"
                | "mark_item"
                | "mark_done"
                | "create_ticket"
                | "scan_tickets"
                | "generate_items"
        )
    })
}

fn parse_tool_result_payload(payload: &serde_json::Value) -> Option<serde_json::Value> {
    match payload {
        serde_json::Value::String(value) => serde_json::from_str(value).ok(),
        serde_json::Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
                .collect::<String>();
            serde_json::from_str(&text).ok()
        }
        serde_json::Value::Object(_) => Some(payload.clone()),
        _ => None,
    }
}

/// Execute a single store put operation. Non-critical: logs on failure.
async fn execute_store_put(
    state: &Arc<InnerState>,
    task_ctx: &TaskRuntimeContext,
    task_id: &str,
    store: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    let cr = agent_orchestrator::config_load::read_loaded_config(state)?
        .config
        .custom_resources
        .clone();
    state
        .store_manager
        .execute(
            &cr,
            StoreOp::Put {
                store_name: store.to_string(),
                project_id: task_ctx.project_id.clone(),
                key: key.to_string(),
                value: value.to_string(),
                task_id: task_id.to_string(),
            },
        )
        .await?;
    Ok(())
}

/// Process store_outputs declarations on a step, writing pipeline vars to stores.
async fn process_store_outputs(
    state: &Arc<InnerState>,
    task_ctx: &TaskRuntimeContext,
    task_id: &str,
    step: &TaskExecutionStep,
    acc: &StepExecutionAccumulator,
) {
    for output in &step.store_outputs {
        if let Some(value) = acc.pipeline_vars.vars.get(&output.from_var) {
            if let Err(e) =
                execute_store_put(state, task_ctx, task_id, &output.store, &output.key, value).await
            {
                warn!(
                    error = %e,
                    store = %output.store,
                    key = %output.key,
                    "store_output write failed"
                );
            }
        } else {
            warn!(
                from_var = %output.from_var,
                store = %output.store,
                "store_output: pipeline var not found"
            );
        }
    }
}

#[cfg(test)]
mod coordination_effect_tests {
    use super::*;
    use agent_orchestrator::collab::{AgentOutput, Artifact, ArtifactKind};
    use agent_orchestrator::config::PipelineVariables;
    use uuid::Uuid;

    fn result_with_artifacts(artifacts: Vec<Artifact>) -> agent_orchestrator::dto::RunResult {
        agent_orchestrator::dto::RunResult {
            success: true,
            exit_code: 0,
            stdout_path: String::new(),
            stderr_path: String::new(),
            timed_out: false,
            duration_ms: Some(1),
            output: Some(
                AgentOutput::new(
                    Uuid::new_v4(),
                    "agent".to_string(),
                    "qa".to_string(),
                    0,
                    String::new(),
                    String::new(),
                )
                .with_artifacts(artifacts),
            ),
            validation_status: "passed".to_string(),
            agent_id: "agent".to_string(),
            run_id: "run".to_string(),
            execution_profile: "host".to_string(),
            execution_mode: "host".to_string(),
            sandbox_denied: false,
            sandbox_denial_reason: None,
            sandbox_violation_kind: None,
            sandbox_resource_kind: None,
            sandbox_network_target: None,
        }
    }

    #[test]
    fn mark_item_and_ticket_receipts_fold_into_accumulator() {
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__mark_item".to_string(),
            })
            .with_content(json!({"call_id":"mark-1","args":{"status":"qa_passed"}})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"mark-1",
                "payload":[{"type":"text","text":"{\"accepted\":true,\"status\":\"qa_passed\"}"}],
                "is_error":false
            })),
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__create_ticket".to_string(),
            })
            .with_content(json!({"call_id":"ticket-1","args":{}})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"ticket-1",
                "payload":"{\"created\":true,\"path\":\"docs/ticket/T-1.md\"}",
                "is_error":false
            })),
        ];
        let result = result_with_artifacts(artifacts);
        let mut accumulator = StepExecutionAccumulator::new(PipelineVariables::default());

        apply_coordination_tool_effects(&result, &mut accumulator);

        assert_eq!(accumulator.item_status, "qa_passed");
        assert_eq!(accumulator.created_ticket_files, vec!["docs/ticket/T-1.md"]);
        assert_eq!(accumulator.active_tickets, vec!["docs/ticket/T-1.md"]);
    }

    #[test]
    fn compatibility_alias_is_detected_and_object_receipt_is_applied() {
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mark_done".to_string(),
            })
            .with_content(json!({"call_id":"done-1","args":{}})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"done-1",
                "payload":{"accepted":true,"status":"verified"},
                "is_error":false
            })),
        ];
        assert!(uses_coordination_tool_model(&artifacts));
        let result = result_with_artifacts(artifacts);
        let mut accumulator = StepExecutionAccumulator::new(PipelineVariables::default());

        apply_coordination_tool_effects(&result, &mut accumulator);

        assert_eq!(accumulator.item_status, "verified");
    }

    #[test]
    fn errored_unpaired_and_malformed_receipts_are_ignored() {
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__mark_item".to_string(),
            })
            .with_content(json!({"call_id":"mark-1"})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"mark-1",
                "payload":{"accepted":true,"status":"qa_failed"},
                "is_error":true
            })),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"unpaired",
                "payload":{"accepted":true,"status":"qa_failed"},
                "is_error":false
            })),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({
                "call_id":"mark-1",
                "payload":[{"type":"image","data":"ignored"}],
                "is_error":false
            })),
        ];
        let result = result_with_artifacts(artifacts);
        let mut accumulator = StepExecutionAccumulator::new(PipelineVariables::default());
        accumulator.item_status = "unresolved".to_string();

        apply_coordination_tool_effects(&result, &mut accumulator);

        assert_eq!(accumulator.item_status, "unresolved");
    }
}
