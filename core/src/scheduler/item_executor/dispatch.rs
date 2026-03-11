use crate::config::{
    ExecutionMode, OnFailureAction, PipelineVariables, StoreInputConfig, TaskExecutionStep,
    TaskRuntimeContext,
};
use crate::dynamic_orchestration::{
    AdaptivePlanExecutor, AdaptivePlanSource, AdaptivePlanner, ExecutionHistoryRecord,
    StepExecutionRecord, StepPrehookContext as DynamicStepContext,
};
use crate::events::insert_event;
use crate::prehook::evaluate_step_prehook;
use crate::state::InnerState;
use crate::store::{StoreOp, StoreOpResult};
use crate::ticket::scan_active_tickets_for_task_items;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::warn;

use super::super::phase_runner::{
    run_phase, run_phase_with_rotation, run_phase_with_selected_agent, PhaseRunRequest,
    RotatingPhaseRunRequest, SelectedPhaseRunRequest,
};
use super::super::safety::{
    execute_self_restart_step, execute_self_test_step, RestartRequestedError, SelfRestartOutcome,
};
use super::super::RunningTask;
use super::accumulator::StepExecutionAccumulator;
use super::apply::apply_step_results;
use super::finalize::finalize_item_execution;
use super::spill::{spill_large_var, spill_to_file};

pub struct ProcessItemRequest<'a> {
    pub task_id: &'a str,
    pub item: &'a crate::dto::TaskItemRow,
    pub task_item_paths: &'a [String],
    pub task_ctx: &'a TaskRuntimeContext,
    pub runtime: &'a RunningTask,
    pub step_filter: Option<&'a HashSet<String>>,
    pub run_dynamic_steps: bool,
}

/// Owned variant of ProcessItemRequest for tokio::spawn (requires 'static).
pub struct OwnedProcessItemRequest {
    pub task_id: String,
    pub item: crate::dto::TaskItemRow,
    pub task_item_paths: Arc<Vec<String>>,
    pub task_ctx: Arc<TaskRuntimeContext>,
    pub runtime: RunningTask,
    pub step_filter: Option<Arc<HashSet<String>>>,
    pub run_dynamic_steps: bool,
}

/// Entry point for parallel item execution. Borrows from owned fields
/// and delegates to the existing process_item_filtered.
pub async fn process_item_filtered_owned(
    state: &Arc<InnerState>,
    request: OwnedProcessItemRequest,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    process_item_filtered(
        state,
        ProcessItemRequest {
            task_id: &request.task_id,
            item: &request.item,
            task_item_paths: &request.task_item_paths,
            task_ctx: &request.task_ctx,
            runtime: &request.runtime,
            step_filter: request.step_filter.as_deref(),
            run_dynamic_steps: request.run_dynamic_steps,
        },
        acc,
    )
    .await
}

pub async fn process_item(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_item_paths: &[String],
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<()> {
    let mut acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
    process_item_filtered(
        state,
        ProcessItemRequest {
            task_id,
            item,
            task_item_paths,
            task_ctx,
            runtime,
            step_filter: None,
            run_dynamic_steps: true,
        },
        &mut acc,
    )
    .await?;
    finalize_item_execution(state, task_id, item, task_ctx, &mut acc).await?;
    Ok(())
}

/// Process an item, optionally filtering to only run steps whose id is in `step_filter`.
/// When `step_filter` is `None`, all steps run.
/// Returns updated pipeline variables so callers can propagate task-scoped vars.
///
/// # Unified execution loop
/// Every step goes through the same path: prehook → execute → capture → status → post_actions.
/// Step-specific behaviors (on_failure, captures, post_actions) are declared as data in `StepBehavior`.
pub async fn process_item_filtered(
    state: &Arc<InnerState>,
    request: ProcessItemRequest<'_>,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let ProcessItemRequest {
        task_id,
        item,
        task_item_paths,
        task_ctx,
        runtime,
        step_filter,
        run_dynamic_steps,
    } = request;
    let should_run_step =
        |step_id: &str| -> bool { step_filter.map_or(true, |f| f.contains(step_id)) };
    acc.merge_task_pipeline_vars(&task_ctx.pipeline_vars);

    // Inject dynamic item variables (from generate_items) into pipeline vars
    if let Some(ref label) = item.label {
        acc.pipeline_vars
            .vars
            .insert("item_label".to_string(), label.clone());
    }
    if let Some(ref vars_json) = item.dynamic_vars_json {
        if let Ok(vars) =
            serde_json::from_str::<std::collections::HashMap<String, String>>(vars_json)
        {
            for (k, v) in vars {
                acc.pipeline_vars.vars.insert(k, v);
            }
        }
    }

    // ── Unified step loop ────────────────────────────────────────────

    for step in &task_ctx.execution_plan.steps {
        // Check for pause/stop between steps
        if runtime.stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if super::super::task_state::is_task_paused_in_db(state, task_id).await? {
            return Ok(());
        }
        if acc.terminal {
            return Ok(());
        }

        // Skip guards (handled separately in loop_engine), disabled, and filtered-out steps
        if step.is_guard || !step.enabled || !should_run_step(&step.id) {
            continue;
        }
        if !step.repeatable && task_ctx.current_cycle > 1 {
            continue;
        }

        match execute_step(
            state,
            StepExecutionRequest {
                task_id,
                item,
                task_item_paths,
                event_ctx: StepEventContext::top_level(),
                step,
                task_ctx,
                runtime,
            },
            acc,
        )
        .await?
        {
            StepExecutionOutcome::Completed { .. } => {}
            StepExecutionOutcome::EarlyReturn => return Ok(()),
        }
    }

    if run_dynamic_steps {
        execute_dynamic_steps(state, task_id, item, task_ctx, runtime, acc).await?;
    }

    Ok(())
}

// ── Extracted sub-functions for process_item_filtered ──────────────

/// Outcome of dispatching a builtin step (self_test, self_restart, ticket_scan).
enum BuiltinStepOutcome {
    /// The builtin was recognized and fully handled; caller should `continue`.
    Handled { success: bool },
    /// The builtin triggered an early return from the outer function.
    EarlyReturn,
    /// Not a recognized builtin dispatch; fall through to agent/generic execution.
    NotBuiltin,
    /// Self-restart succeeded; daemon should exec the new binary.
    RestartRequested { binary_path: std::path::PathBuf },
}

#[derive(Clone, Copy)]
struct StepEventContext<'a> {
    started_event_type: &'static str,
    finished_event_type: &'static str,
    parent_step: Option<&'a str>,
}

impl<'a> StepEventContext<'a> {
    fn top_level() -> Self {
        Self {
            started_event_type: "step_started",
            finished_event_type: "step_finished",
            parent_step: None,
        }
    }

    fn chain_child(parent_step: &'a str) -> Self {
        Self {
            started_event_type: "chain_step_started",
            finished_event_type: "chain_step_finished",
            parent_step: Some(parent_step),
        }
    }
}

struct StepExecutionRequest<'a> {
    task_id: &'a str,
    item: &'a crate::dto::TaskItemRow,
    task_item_paths: &'a [String],
    event_ctx: StepEventContext<'a>,
    step: &'a TaskExecutionStep,
    task_ctx: &'a TaskRuntimeContext,
    runtime: &'a RunningTask,
}

enum StepExecutionOutcome {
    Completed { success: bool },
    EarlyReturn,
}

fn build_step_event_payload(
    step: &TaskExecutionStep,
    cycle: u32,
    pipeline_var_keys: Vec<String>,
    parent_step: Option<&str>,
) -> Value {
    let mut payload = json!({
        "step": step.id,
        "step_id": step.id,
        "step_scope": step.resolved_scope(),
        "cycle": cycle,
        "pipeline_var_keys": pipeline_var_keys,
    });
    if let Some(parent_step) = parent_step {
        payload["parent_step"] = json!(parent_step);
    }
    payload
}

fn build_step_skipped_payload(
    step: &TaskExecutionStep,
    reason: &str,
    parent_step: Option<&str>,
) -> Value {
    let mut payload = json!({
        "step": step.id,
        "step_id": step.id,
        "step_scope": step.resolved_scope(),
        "reason": reason,
    });
    if let Some(parent_step) = parent_step {
        payload["parent_step"] = json!(parent_step);
    }
    payload
}

fn synthetic_chain_result(step: &TaskExecutionStep, success: bool) -> crate::dto::RunResult {
    let output = crate::collab::AgentOutput::new(
        uuid::Uuid::new_v4(),
        "chain".to_string(),
        step.id.clone(),
        if success { 0 } else { 1 },
        String::new(),
        String::new(),
    );
    crate::dto::RunResult {
        success,
        exit_code: if success { 0 } else { 1 },
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: None,
        output: Some(output),
        validation_status: "passed".to_string(),
        agent_id: "chain".to_string(),
        run_id: String::new(),
        execution_profile: "chain".to_string(),
        execution_mode: "chain".to_string(),
        sandbox_denied: false,
        sandbox_denial_reason: None,
        sandbox_violation_kind: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    }
}

fn execute_step<'a>(
    state: &'a Arc<InnerState>,
    request: StepExecutionRequest<'a>,
    acc: &'a mut StepExecutionAccumulator,
) -> Pin<Box<dyn Future<Output = Result<StepExecutionOutcome>> + Send + 'a>> {
    Box::pin(async move {
        let StepExecutionRequest {
            task_id,
            item,
            task_item_paths,
            event_ctx,
            step,
            task_ctx,
            runtime,
        } = request;
        let item_id = item.id.as_str();
        let phase = step.id.as_str();

        if runtime.stop_flag.load(std::sync::atomic::Ordering::SeqCst)
            || super::super::task_state::is_task_paused_in_db(state, task_id).await?
            || acc.terminal
        {
            return Ok(StepExecutionOutcome::EarlyReturn);
        }

        let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, &step.id);
        let should_run = evaluate_step_prehook(state, step.prehook.as_ref(), &prehook_ctx).await?;
        if !should_run {
            acc.step_skipped.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                build_step_skipped_payload(step, "prehook_false", event_ctx.parent_step),
            )
            .await?;
            return Ok(StepExecutionOutcome::Completed { success: true });
        }

        if !step.store_inputs.is_empty() {
            resolve_store_inputs(state, &task_ctx.project_id, &step.store_inputs, acc).await?;
        }

        if acc.step_ran.is_empty() {
            state.db_writer.mark_task_item_running(item_id).await?;
        }

        let pipeline_var_keys: Vec<String> = acc.pipeline_vars.vars.keys().cloned().collect();
        insert_event(
            state,
            task_id,
            Some(item_id),
            event_ctx.started_event_type,
            build_step_event_payload(
                step,
                task_ctx.current_cycle,
                pipeline_var_keys,
                event_ctx.parent_step,
            ),
        )
        .await?;

        let effective_execution = step.effective_execution_mode();
        let builtin_outcome = execute_builtin_step_dispatch(
            state,
            task_id,
            item_id,
            phase,
            step,
            &effective_execution,
            event_ctx.finished_event_type,
            event_ctx.parent_step,
            task_ctx,
            task_item_paths,
            &item.qa_file_path,
            acc,
        )
        .await?;

        match builtin_outcome {
            BuiltinStepOutcome::Handled { success } => {
                return Ok(StepExecutionOutcome::Completed { success });
            }
            BuiltinStepOutcome::EarlyReturn => return Ok(StepExecutionOutcome::EarlyReturn),
            BuiltinStepOutcome::NotBuiltin => {}
            BuiltinStepOutcome::RestartRequested { binary_path } => {
                return Err(RestartRequestedError { binary_path }.into());
            }
        }

        let agent_outcome = execute_agent_step(
            state,
            task_id,
            item,
            task_item_paths,
            step,
            &effective_execution,
            event_ctx,
            task_ctx,
            runtime,
            acc,
        )
        .await?;

        match agent_outcome {
            AgentStepOutcome::EarlyReturn => Ok(StepExecutionOutcome::EarlyReturn),
            AgentStepOutcome::Result(result) => {
                let should_return = apply_step_results(
                    state,
                    task_id,
                    item_id,
                    phase,
                    step,
                    event_ctx.finished_event_type,
                    event_ctx.parent_step,
                    task_ctx,
                    task_item_paths,
                    &item.qa_file_path,
                    &result,
                    acc,
                )
                .await?;
                if should_return {
                    return Ok(StepExecutionOutcome::EarlyReturn);
                }
                Ok(StepExecutionOutcome::Completed {
                    success: result.is_success(),
                })
            }
        }
    })
}

/// Dispatch self_test, self_restart, and ticket_scan builtin steps.
/// These builtins handle their own result capture and event emission.
#[allow(clippy::too_many_arguments)]
async fn execute_builtin_step_dispatch(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    step: &TaskExecutionStep,
    effective_execution: &ExecutionMode,
    finish_event_type: &str,
    parent_step: Option<&str>,
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
    qa_file_path: &str,
    acc: &mut StepExecutionAccumulator,
) -> Result<BuiltinStepOutcome> {
    match effective_execution {
        ExecutionMode::Builtin { name } if name == "self_test" => {
            // Self-test uses a specialized builtin
            let exit_code = execute_self_test_step(
                &task_ctx.workspace_root,
                state,
                task_id,
                item_id,
                Some(task_ctx.project_id.as_str()),
            )
            .await
            .unwrap_or(1);
            let passed = exit_code == 0;
            acc.pipeline_vars
                .vars
                .insert("self_test_exit_code".to_string(), exit_code.to_string());
            acc.pipeline_vars
                .vars
                .insert("self_test_passed".to_string(), passed.to_string());

            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "exit_code": exit_code,
                "success": passed
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

            // Apply behavior-driven status transitions for self_test
            if !passed {
                match &step.behavior.on_failure {
                    OnFailureAction::Continue => {}
                    OnFailureAction::SetStatus { status } => {
                        acc.item_status = status.clone();
                    }
                    OnFailureAction::EarlyReturn { status } => {
                        acc.item_status = status.clone();
                        acc.terminal = true;
                        return Ok(BuiltinStepOutcome::EarlyReturn);
                    }
                }
            }
            acc.step_ran.insert(step.id.clone(), true);
            acc.exit_codes.insert(step.id.clone(), exit_code as i64);
            // Apply captures
            let synth_result = crate::dto::RunResult {
                success: passed,
                exit_code: exit_code as i64,
                stdout_path: String::new(),
                stderr_path: String::new(),
                timed_out: false,
                duration_ms: None,
                output: None,
                validation_status: "passed".to_string(),
                agent_id: "builtin".to_string(),
                run_id: String::new(),
                execution_profile: "host".to_string(),
                execution_mode: "host".to_string(),
                sandbox_denied: false,
                sandbox_denial_reason: None,
                sandbox_violation_kind: None,
                sandbox_resource_kind: None,
                sandbox_network_target: None,
            };
            acc.apply_captures(&step.behavior.captures, &step.id, &synth_result);
            Ok(BuiltinStepOutcome::Handled { success: passed })
        }

        ExecutionMode::Builtin { name } if name == "self_restart" => {
            // Self-restart builtin: rebuild, verify, snapshot, then signal restart
            let ws_root = std::path::Path::new(&task_ctx.workspace_root);
            let outcome = execute_self_restart_step(ws_root, state, task_id, item_id)
                .await
                .unwrap_or(SelfRestartOutcome::Failed(1));

            match outcome {
                SelfRestartOutcome::RestartReady { binary_path } => {
                    let exit_code: i64 = 75; // EXIT_RESTART for event/vars compat
                    acc.pipeline_vars
                        .vars
                        .insert("self_restart_exit_code".to_string(), exit_code.to_string());

                    let mut payload = json!({
                        "step": phase,
                        "step_id": step.id,
                        "step_scope": step.resolved_scope(),
                        "exit_code": exit_code,
                        "restart": true
                    });
                    if let Some(parent_step) = parent_step {
                        payload["parent_step"] = json!(parent_step);
                    }
                    insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

                    // Invariant checkpoint: before_restart
                    let inv_results = crate::scheduler::invariant::evaluate_invariants(
                        &task_ctx.pinned_invariants,
                        crate::config::InvariantCheckPoint::BeforeRestart,
                        &task_ctx.workspace_root,
                    );
                    if let Ok(ref results) = inv_results {
                        for r in results {
                            let event_type = if r.passed {
                                "invariant_passed"
                            } else {
                                "invariant_violated"
                            };
                            let _ = insert_event(
                                state, task_id, Some(item_id), event_type,
                                json!({"invariant": r.name, "checkpoint": "BeforeRestart", "passed": r.passed, "message": r.message}),
                            ).await;
                        }
                        if crate::scheduler::invariant::has_halting_violation(results) {
                            warn!("invariant halt at before_restart — aborting restart");
                            acc.step_ran.insert(step.id.clone(), true);
                            acc.exit_codes.insert(step.id.clone(), exit_code);
                            return Ok(BuiltinStepOutcome::EarlyReturn);
                        }
                    }

                    // Persist pipeline vars to DB before restart so the new process recovers them.
                    if let Ok(json) = serde_json::to_string(&acc.pipeline_vars) {
                        if let Err(e) = state
                            .db_writer
                            .update_task_pipeline_vars(task_id, &json)
                            .await
                        {
                            tracing::warn!("failed to persist pipeline_vars before restart: {e}");
                        }
                    }

                    acc.step_ran.insert(step.id.clone(), true);
                    acc.exit_codes.insert(step.id.clone(), exit_code);
                    // Signal restart up the call stack — daemon layer handles exec()
                    Ok(BuiltinStepOutcome::RestartRequested { binary_path })
                }
                SelfRestartOutcome::Failed(exit_code) => {
                    acc.pipeline_vars
                        .vars
                        .insert("self_restart_exit_code".to_string(), exit_code.to_string());

                    let mut payload = json!({
                        "step": phase,
                        "step_id": step.id,
                        "step_scope": step.resolved_scope(),
                        "exit_code": exit_code,
                        "restart": false
                    });
                    if let Some(parent_step) = parent_step {
                        payload["parent_step"] = json!(parent_step);
                    }
                    insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

                    // Build or verification failed — apply on_failure behavior
                    if exit_code != 0 {
                        match &step.behavior.on_failure {
                            OnFailureAction::Continue => {}
                            OnFailureAction::SetStatus { status } => {
                                acc.item_status = status.clone();
                            }
                            OnFailureAction::EarlyReturn { status } => {
                                acc.item_status = status.clone();
                                acc.terminal = true;
                                return Ok(BuiltinStepOutcome::EarlyReturn);
                            }
                        }
                    }
                    acc.step_ran.insert(step.id.clone(), true);
                    acc.exit_codes.insert(step.id.clone(), exit_code);
                    Ok(BuiltinStepOutcome::Handled {
                        success: exit_code == 0,
                    })
                }
            }
        }

        ExecutionMode::Builtin { name } if name == "ticket_scan" => {
            // Ticket scan builtin (step_started already emitted above)
            let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
            acc.active_tickets = tickets.get(qa_file_path).cloned().unwrap_or_default();
            acc.new_ticket_count = acc.active_tickets.len() as i64;
            acc.step_ran.insert(step.id.clone(), true);
            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "tickets": acc.active_tickets.len()
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;
            Ok(BuiltinStepOutcome::Handled { success: true })
        }

        ExecutionMode::Builtin { name } if name == "item_select" => {
            // Selection orchestrated at loop_engine level; this is a marker step
            acc.step_ran.insert(step.id.clone(), true);
            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "builtin": "item_select"
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;
            Ok(BuiltinStepOutcome::Handled { success: true })
        }

        _ => Ok(BuiltinStepOutcome::NotBuiltin),
    }
}

/// Outcome of executing a chain or agent/generic step.
#[allow(clippy::large_enum_variant)]
enum AgentStepOutcome {
    /// The step requested an early return from the item execution loop.
    EarlyReturn,
    /// A RunResult was produced and needs post-processing via `apply_step_results`.
    Result(crate::dto::RunResult),
}

/// Execute chain steps or agent/generic steps, producing either a handled outcome
/// (for chains) or a RunResult for further processing.
#[allow(clippy::too_many_arguments)]
async fn execute_agent_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_item_paths: &[String],
    step: &TaskExecutionStep,
    effective_execution: &ExecutionMode,
    event_ctx: StepEventContext<'_>,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
) -> Result<AgentStepOutcome> {
    match effective_execution {
        ExecutionMode::Chain => {
            let mut chain_passed = true;
            for chain_step in &step.chain_steps {
                let mut step_ctx = task_ctx.clone();
                step_ctx.pipeline_vars = acc.pipeline_vars.clone();
                match execute_step(
                    state,
                    StepExecutionRequest {
                        task_id,
                        item,
                        task_item_paths,
                        event_ctx: StepEventContext::chain_child(&step.id),
                        step: chain_step,
                        task_ctx: &step_ctx,
                        runtime,
                    },
                    acc,
                )
                .await?
                {
                    StepExecutionOutcome::Completed { success } => {
                        if !success {
                            chain_passed = false;
                            break;
                        }
                    }
                    StepExecutionOutcome::EarlyReturn => {
                        return Ok(AgentStepOutcome::EarlyReturn);
                    }
                }
            }
            Ok(AgentStepOutcome::Result(synthetic_chain_result(
                step,
                chain_passed,
            )))
        }

        // ExecutionMode::Agent or ExecutionMode::Builtin for generic builtins
        _ => {
            let mut step_ctx = task_ctx.clone();
            step_ctx.pipeline_vars = acc.pipeline_vars.clone();

            let exec_result = execute_builtin_step(
                state,
                task_id,
                item.id.as_str(),
                step,
                &step_ctx,
                runtime,
                &item.qa_file_path,
            )
            .await;

            let (result, new_pipeline) = match exec_result {
                Ok(val) => val,
                Err(e) => {
                    let mut payload = json!({
                        "step": step.id,
                        "step_id": step.id,
                        "step_scope": step.resolved_scope(),
                        "error": e.to_string(),
                        "success": false
                    });
                    if let Some(parent_step) = event_ctx.parent_step {
                        payload["parent_step"] = json!(parent_step);
                    }
                    let _ = insert_event(
                        state,
                        task_id,
                        Some(item.id.as_str()),
                        event_ctx.finished_event_type,
                        payload,
                    )
                    .await;
                    return Err(e);
                }
            };
            acc.pipeline_vars = new_pipeline;

            if let Some(ref output) = result.output {
                if !output.stdout.is_empty() {
                    let output_key = format!("{}_output", step.id);
                    spill_large_var(
                        &state.logs_dir,
                        task_id,
                        &output_key,
                        output.stdout.clone(),
                        &mut acc.pipeline_vars,
                    );
                }
            }

            Ok(AgentStepOutcome::Result(result))
        }
    }
}

pub async fn execute_builtin_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step: &TaskExecutionStep,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    rel_path: &str,
) -> Result<(crate::dto::RunResult, PipelineVariables)> {
    let phase = &step.id;

    let result = if let Some(ref command) = step.command {
        let ctx = crate::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            task_ctx.current_cycle,
            phase.to_string(),
            task_ctx.workspace_root.clone(),
            task_ctx.workspace_id.clone(),
        );
        let rendered_command =
            ctx.render_template_with_pipeline(command, Some(&task_ctx.pipeline_vars));

        run_phase(
            state,
            PhaseRunRequest {
                task_id,
                item_id,
                step_id: &step.id,
                phase,
                tty: step.tty,
                command: rendered_command,
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                agent_id: "builtin",
                runtime,
                step_timeout_secs: step.timeout_secs,
                step_scope: step.resolved_scope(),
                prompt_delivery: crate::config::PromptDelivery::Arg,
                prompt_payload: None,
                pipe_stdin: false,
                project_id: &task_ctx.project_id,
                execution_profile: None,
            },
        )
        .await?
    } else {
        let resolved_prompt = step.template.as_ref().and_then(|tmpl_name| {
            let cfg = crate::config_load::read_loaded_config(state).ok()?;
            cfg.config
                .default_project()?
                .step_templates
                .get(tmpl_name)
                .map(|t| t.prompt.clone())
        });
        run_phase_with_rotation(
            state,
            RotatingPhaseRunRequest {
                task_id,
                item_id,
                step_id: &step.id,
                phase,
                tty: step.tty,
                capability: step.required_capability.as_deref(),
                rel_path,
                ticket_paths: &[],
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: Some(&task_ctx.pipeline_vars),
                step_timeout_secs: step.timeout_secs.or(task_ctx.safety.step_timeout_secs),
                step_scope: step.resolved_scope(),
                step_template_prompt: resolved_prompt.as_deref(),
                project_id: &task_ctx.project_id,
                execution_profile: step.execution_profile.as_deref(),
            },
        )
        .await?
    };

    let mut pipeline = task_ctx.pipeline_vars.clone();
    if let Some(ref output) = result.output {
        pipeline.prev_stdout = output.stdout.clone();
        pipeline.prev_stderr = output.stderr.clone();
        if let Some((trunc, path)) = spill_to_file(
            &state.logs_dir,
            task_id,
            "prev_stdout",
            &pipeline.prev_stdout,
        ) {
            pipeline.prev_stdout = trunc;
            pipeline.vars.insert("prev_stdout_path".to_string(), path);
        }
        if let Some((trunc, path)) = spill_to_file(
            &state.logs_dir,
            task_id,
            "prev_stderr",
            &pipeline.prev_stderr,
        ) {
            pipeline.prev_stderr = trunc;
            pipeline.vars.insert("prev_stderr_path".to_string(), path);
        }
        pipeline.build_errors = output.build_errors.clone();
        pipeline.test_failures = output.test_failures.clone();

        let output_key = format!("{}_output", phase);
        if !output.stdout.is_empty() {
            spill_large_var(
                &state.logs_dir,
                task_id,
                &output_key,
                output.stdout.clone(),
                &mut pipeline,
            );
        }
    }

    if let Ok(diff_output) = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(&task_ctx.workspace_root)
        .output()
        .await
    {
        pipeline.diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
        if let Some((trunc, path)) = spill_to_file(&state.logs_dir, task_id, "diff", &pipeline.diff)
        {
            pipeline.diff = trunc;
            pipeline.vars.insert("diff_path".to_string(), path);
        }
    }

    Ok((result, pipeline))
}

pub(super) fn is_execution_hard_failure(result: &crate::dto::RunResult) -> bool {
    result.validation_status == "failed"
}

/// Execute dynamic steps from the dynamic step pool.
/// Only runs in full-cycle mode (not in segment-filtered mode).
async fn execute_dynamic_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    if let Some(adaptive_config) = task_ctx.adaptive.clone().filter(|cfg| cfg.enabled) {
        let history = build_adaptive_history(task_id, item.id.as_str(), task_ctx, acc);
        let mut planner = AdaptivePlanner::new(adaptive_config.clone());
        for record in history {
            planner.add_history(record);
        }
        let planner_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, "adaptive_plan");
        insert_event(
            state,
            task_id,
            Some(item.id.as_str()),
            "adaptive_plan_requested",
            json!({
                "planner_agent": adaptive_config.planner_agent,
                "cycle": task_ctx.current_cycle,
                "fallback_mode": adaptive_config.fallback_mode,
            }),
        )
        .await?;

        let executor = AgentBackedAdaptiveExecutor {
            state,
            task_id,
            item_id: item.id.as_str(),
            item,
            task_ctx,
            runtime,
        };
        match planner.generate_plan(&executor, &planner_ctx).await {
            Ok(outcome) => {
                let event_name = match outcome.metadata.source {
                    AdaptivePlanSource::Planner => "adaptive_plan_succeeded",
                    AdaptivePlanSource::DeterministicFallback => "adaptive_plan_fallback_used",
                };
                insert_event(
                    state,
                    task_id,
                    Some(item.id.as_str()),
                    event_name,
                    json!({
                        "planner_agent": adaptive_config.planner_agent,
                        "cycle": task_ctx.current_cycle,
                        "fallback_mode": adaptive_config.fallback_mode,
                        "error_class": outcome.metadata.error_class.map(crate::dynamic_orchestration::adaptive_failure_class_name),
                        "node_count": outcome.plan.nodes.len(),
                        "edge_count": outcome.plan.edges.len(),
                    }),
                )
                .await?;
                return execute_adaptive_plan(
                    state,
                    task_id,
                    item,
                    task_ctx,
                    runtime,
                    acc,
                    &outcome.plan,
                )
                .await;
            }
            Err(err) => {
                insert_event(
                    state,
                    task_id,
                    Some(item.id.as_str()),
                    "adaptive_plan_failed",
                    json!({
                        "planner_agent": adaptive_config.planner_agent,
                        "cycle": task_ctx.current_cycle,
                        "fallback_mode": adaptive_config.fallback_mode,
                        "error": err.to_string(),
                    }),
                )
                .await?;
                return Err(err);
            }
        }
    }

    if task_ctx.dynamic_steps.is_empty() {
        return Ok(());
    }

    let pool = {
        let mut p = crate::dynamic_orchestration::DynamicStepPool::new();
        for ds in &task_ctx.dynamic_steps {
            p.add_step(ds.clone());
        }
        p
    };
    let dyn_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, "dynamic");
    let matched: Vec<_> = pool
        .find_matching_steps(&dyn_ctx)
        .into_iter()
        .cloned()
        .collect();
    for ds in &matched {
        execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, ds).await?;
    }

    Ok(())
}

#[async_trait]
impl AdaptivePlanExecutor for AgentBackedAdaptiveExecutor<'_> {
    async fn execute(
        &self,
        prompt: &str,
        config: &crate::dynamic_orchestration::AdaptivePlannerConfig,
    ) -> Result<String> {
        let planner_agent = config
            .planner_agent
            .as_deref()
            .ok_or_else(|| anyhow!("adaptive planner agent is missing"))?;
        let (command, prompt_delivery) = {
            let active = crate::config_load::read_active_config(self.state)?;
            let agent = crate::selection::resolve_agent_by_id(
                &self.task_ctx.project_id,
                &active.config,
                planner_agent,
            )
            .ok_or_else(|| anyhow!("adaptive planner agent not found: {}", planner_agent))?;
            (agent.command.clone(), agent.prompt_delivery)
        };
        let result = run_phase_with_selected_agent(
            self.state,
            SelectedPhaseRunRequest {
                task_id: self.task_id,
                item_id: self.item_id,
                step_id: "adaptive_plan",
                phase: "adaptive_plan",
                tty: false,
                agent_id: planner_agent,
                command_template: &command,
                prompt_delivery,
                rel_path: &self.item.qa_file_path,
                ticket_paths: &[],
                workspace_root: &self.task_ctx.workspace_root,
                workspace_id: &self.task_ctx.workspace_id,
                cycle: self.task_ctx.current_cycle,
                runtime: self.runtime,
                pipeline_vars: None,
                step_timeout_secs: self.task_ctx.safety.step_timeout_secs,
                step_scope: crate::config::StepScope::Item,
                step_template_prompt: Some(prompt),
                project_id: &self.task_ctx.project_id,
                execution_profile: None,
            },
        )
        .await?;
        let output = result
            .output
            .ok_or_else(|| anyhow!("adaptive planner produced no structured output"))?;
        Ok(output.stdout)
    }
}

struct AgentBackedAdaptiveExecutor<'a> {
    state: &'a Arc<InnerState>,
    task_id: &'a str,
    item_id: &'a str,
    item: &'a crate::dto::TaskItemRow,
    task_ctx: &'a TaskRuntimeContext,
    runtime: &'a RunningTask,
}

fn build_dynamic_step_context(
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    acc: &StepExecutionAccumulator,
    step_id: &str,
) -> DynamicStepContext {
    let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, step_id);
    DynamicStepContext {
        task_id: prehook_ctx.task_id,
        task_item_id: prehook_ctx.task_item_id,
        cycle: prehook_ctx.cycle,
        step: prehook_ctx.step,
        qa_file_path: prehook_ctx.qa_file_path,
        item_status: prehook_ctx.item_status,
        task_status: prehook_ctx.task_status,
        qa_exit_code: prehook_ctx.qa_exit_code,
        fix_exit_code: prehook_ctx.fix_exit_code,
        retest_exit_code: prehook_ctx.retest_exit_code,
        active_ticket_count: prehook_ctx.active_ticket_count,
        new_ticket_count: prehook_ctx.new_ticket_count,
        qa_failed: prehook_ctx.qa_failed,
        fix_required: prehook_ctx.fix_required,
        qa_confidence: prehook_ctx.qa_confidence,
        qa_quality_score: prehook_ctx.qa_quality_score,
        fix_has_changes: prehook_ctx.fix_has_changes,
        upstream_artifacts: vec![],
        build_error_count: prehook_ctx.build_error_count,
        test_failure_count: prehook_ctx.test_failure_count,
        build_exit_code: prehook_ctx.build_exit_code,
        test_exit_code: prehook_ctx.test_exit_code,
        self_test_exit_code: prehook_ctx.self_test_exit_code,
        self_test_passed: prehook_ctx.self_test_passed,
        max_cycles: prehook_ctx.max_cycles,
        is_last_cycle: prehook_ctx.is_last_cycle,
        last_sandbox_denied: prehook_ctx.last_sandbox_denied,
        sandbox_denied_count: prehook_ctx.sandbox_denied_count,
        last_sandbox_denial_reason: prehook_ctx.last_sandbox_denial_reason,
        self_referential_safe: prehook_ctx.self_referential_safe,
    }
}

fn build_adaptive_history(
    task_id: &str,
    item_id: &str,
    task_ctx: &TaskRuntimeContext,
    acc: &StepExecutionAccumulator,
) -> Vec<ExecutionHistoryRecord> {
    let mut steps: Vec<StepExecutionRecord> = acc
        .exit_codes
        .iter()
        .map(|(step_id, exit_code)| StepExecutionRecord {
            step_id: step_id.clone(),
            step_type: step_id.clone(),
            exit_code: *exit_code,
            duration_ms: 0,
            confidence: if step_id.contains("qa") {
                acc.qa_confidence
            } else {
                acc.fix_confidence
            },
            quality_score: if step_id.contains("qa") {
                acc.qa_quality_score
            } else {
                acc.fix_quality_score
            },
            tickets_created: acc.new_ticket_count,
            tickets_resolved: 0,
        })
        .collect();
    steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));

    if steps.is_empty() {
        return Vec::new();
    }

    vec![ExecutionHistoryRecord {
        task_id: task_id.to_string(),
        item_id: item_id.to_string(),
        cycle: task_ctx.current_cycle,
        steps,
        final_status: acc.item_status.clone(),
        timestamp: chrono::Utc::now(),
    }]
}

async fn execute_adaptive_plan(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
    plan: &crate::dynamic_orchestration::DynamicExecutionPlan,
) -> Result<()> {
    let mut queue: VecDeque<String> = if let Some(entry) = plan.entry.clone() {
        VecDeque::from([entry])
    } else {
        let mut entries: Vec<String> = plan
            .get_entry_nodes()
            .into_iter()
            .map(|node| node.id.clone())
            .collect();
        entries.sort();
        entries.into()
    };
    let mut executed = HashSet::new();

    while let Some(node_id) = queue.pop_front() {
        if !executed.insert(node_id.clone()) {
            continue;
        }
        let node = plan
            .get_node(&node_id)
            .ok_or_else(|| anyhow!("adaptive plan node disappeared: {}", node_id))?;
        let dyn_step = crate::dynamic_orchestration::DynamicStepConfig {
            id: node.id.clone(),
            description: None,
            step_type: node.step_type.clone(),
            agent_id: node.agent_id.clone(),
            template: node.template.clone(),
            trigger: node.prehook.as_ref().map(|prehook| prehook.when.clone()),
            priority: 0,
            max_runs: None,
        };
        execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, &dyn_step)
            .await?;

        let dyn_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, &node_id);
        for next in plan.find_next_nodes(&node_id, &dyn_ctx) {
            if !executed.contains(&next) {
                queue.push_back(next);
            }
        }
    }

    Ok(())
}

pub(crate) async fn execute_dynamic_step_config(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
    ds: &crate::dynamic_orchestration::DynamicStepConfig,
) -> Result<()> {
    let item_id = item.id.as_str();
    insert_event(
        state,
        task_id,
        Some(item_id),
        "dynamic_step_started",
        json!({"step_id": ds.id, "step_type": ds.step_type, "step_scope": "item", "priority": ds.priority}),
    )
    .await?;
    let result = if let Some(agent_id) = ds.agent_id.as_deref() {
        let (command, prompt_delivery) = {
            let active = crate::config_load::read_active_config(state)?;
            let agent = crate::selection::resolve_agent_by_id(
                &task_ctx.project_id,
                &active.config,
                agent_id,
            )
            .ok_or_else(|| {
                anyhow!(
                    "dynamic step '{}' references unknown agent '{}'",
                    ds.id,
                    agent_id
                )
            })?;
            (
                ds.template.clone().unwrap_or_else(|| agent.command.clone()),
                agent.prompt_delivery,
            )
        };
        run_phase_with_selected_agent(
            state,
            SelectedPhaseRunRequest {
                task_id,
                item_id,
                step_id: &ds.id,
                phase: &ds.step_type,
                tty: false,
                agent_id,
                command_template: &command,
                prompt_delivery,
                rel_path: &item.qa_file_path,
                ticket_paths: &acc.active_tickets,
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: None,
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
                step_scope: crate::config::StepScope::Item,
                step_template_prompt: None,
                project_id: &task_ctx.project_id,
                execution_profile: None,
            },
        )
        .await?
    } else {
        let cap = Some(ds.step_type.as_str());
        run_phase_with_rotation(
            state,
            RotatingPhaseRunRequest {
                task_id,
                item_id,
                step_id: &ds.id,
                phase: &ds.step_type,
                tty: false,
                capability: cap,
                rel_path: &item.qa_file_path,
                ticket_paths: &acc.active_tickets,
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: None,
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
                step_scope: crate::config::StepScope::Item,
                step_template_prompt: ds.template.as_deref(),
                project_id: &task_ctx.project_id,
                execution_profile: None,
            },
        )
        .await?
    };
    insert_event(
        state,
        task_id,
        Some(item_id),
        "dynamic_step_finished",
        json!({
            "step_id": ds.id,
            "step_scope": "item",
            "exit_code": result.exit_code,
            "success": result.is_success(),
            "execution_profile": result.execution_profile,
            "execution_mode": result.execution_mode,
            "sandbox_denied": result.sandbox_denied,
            "sandbox_denial_reason": result.sandbox_denial_reason,
            "sandbox_violation_kind": result.sandbox_violation_kind,
            "sandbox_resource_kind": result.sandbox_resource_kind,
            "sandbox_network_target": result.sandbox_network_target,
        }),
    )
    .await?;
    acc.exit_codes.insert(ds.id.clone(), result.exit_code);
    acc.step_ran.insert(ds.id.clone(), true);
    acc.apply_run_diagnostics(&result);
    match ds.step_type.as_str() {
        "qa" => {
            acc.flags
                .insert("qa_failed".to_string(), !result.is_success());
            if let Some(output) = result.output.as_ref() {
                acc.qa_confidence = Some(output.confidence);
                acc.qa_quality_score = Some(output.quality_score);
            }
        }
        "fix" => {
            acc.flags
                .insert("fix_success".to_string(), result.is_success());
            if let Some(output) = result.output.as_ref() {
                acc.fix_confidence = Some(output.confidence);
                acc.fix_quality_score = Some(output.quality_score);
            }
        }
        "retest" => {
            acc.flags
                .insert("retest_success".to_string(), result.is_success());
        }
        _ => {}
    }
    Ok(())
}

/// Resolve store_inputs: read values from workflow stores and inject into pipeline vars.
async fn resolve_store_inputs(
    state: &Arc<InnerState>,
    project_id: &str,
    inputs: &[StoreInputConfig],
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let cr = state
        .active_config
        .read()
        .map_err(|e| anyhow::anyhow!("config lock: {}", e))?
        .config
        .custom_resources
        .clone();

    for input in inputs {
        let result = state
            .store_manager
            .execute(
                &cr,
                StoreOp::Get {
                    store_name: input.store.clone(),
                    project_id: project_id.to_string(),
                    key: input.key.clone(),
                },
            )
            .await;

        match result {
            Ok(StoreOpResult::Value(Some(val))) => {
                let val_str = match &val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                acc.pipeline_vars.vars.insert(input.as_var.clone(), val_str);
            }
            Ok(StoreOpResult::Value(None)) => {
                if input.required {
                    anyhow::bail!(
                        "store_input: required key '{}' not found in store '{}'",
                        input.key,
                        input.store
                    );
                }
            }
            Ok(_) => {
                warn!(
                    store = %input.store,
                    key = %input.key,
                    "store_input: unexpected result type"
                );
            }
            Err(e) => {
                if input.required {
                    anyhow::bail!(
                        "store_input: failed to read key '{}' from store '{}': {}",
                        input.key,
                        input.store,
                        e
                    );
                }
                warn!(
                    error = %e,
                    store = %input.store,
                    key = %input.key,
                    "store_input: read failed (non-required)"
                );
            }
        }
    }

    Ok(())
}
