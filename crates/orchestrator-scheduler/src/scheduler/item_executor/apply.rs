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
use super::dispatch::is_execution_hard_failure;

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
        &state.logs_dir,
        task_id,
        &step.id,
        result,
    );
    acc.step_ran.insert(step.id.clone(), true);
    acc.apply_run_diagnostics(result);

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

    // 6. Collect artifacts
    if step.behavior.collect_artifacts {
        let step_artifacts = result
            .output
            .as_ref()
            .map(|o| o.artifacts.clone())
            .unwrap_or_default();
        if !step_artifacts.is_empty() {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "artifacts_parsed",
                json!({"step": phase, "count": step_artifacts.len()}),
            )
            .await?;
            acc.phase_artifacts.extend(step_artifacts);
        }
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
                .map(|idx| format!("artifact://ticket/{}", idx))
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
