use crate::config::{
    ExecutionMode, OnFailureAction, PipelineVariables, StoreInputConfig, TaskExecutionStep,
    TaskRuntimeContext,
};
use crate::events::insert_event;
use crate::prehook::evaluate_step_prehook;
use crate::state::InnerState;
use crate::store::{StoreOp, StoreOpResult};
use crate::ticket::scan_active_tickets_for_task_items;
use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::warn;

use super::super::phase_runner::{
    run_phase, run_phase_with_rotation, PhaseRunRequest, RotatingPhaseRunRequest,
};
use super::super::safety::{execute_self_restart_step, execute_self_test_step, EXIT_RESTART};
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
}

/// Owned variant of ProcessItemRequest for tokio::spawn (requires 'static).
pub struct OwnedProcessItemRequest {
    pub task_id: String,
    pub item: crate::dto::TaskItemRow,
    pub task_item_paths: Arc<Vec<String>>,
    pub task_ctx: Arc<TaskRuntimeContext>,
    pub runtime: RunningTask,
    pub step_filter: Option<Arc<HashSet<String>>>,
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
    } = request;
    let item_id = item.id.as_str();
    let should_run_step =
        |step_id: &str| -> bool { step_filter.map_or(true, |f| f.contains(step_id)) };
    acc.merge_task_pipeline_vars(&task_ctx.pipeline_vars);
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

        let phase = &step.id;

        // 1. Evaluate prehook
        let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, &step.id);
        let should_run = evaluate_step_prehook(state, step.prehook.as_ref(), &prehook_ctx).await?;
        if !should_run {
            acc.step_skipped.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                json!({"step": phase, "step_id": &step.id, "step_scope": step.resolved_scope(), "reason": "prehook_false"}),
            ).await?;
            continue;
        }

        // 1b. Resolve store_inputs
        if !step.store_inputs.is_empty() {
            resolve_store_inputs(state, &task_ctx.project_id, &step.store_inputs, acc).await?;
        }

        // 2. Execute
        if acc.step_ran.is_empty() {
            state.db_writer.mark_task_item_running(item_id).await?;
        }
        let pipeline_var_keys: Vec<&String> = acc.pipeline_vars.vars.keys().collect();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_started",
            json!({"step": phase, "step_id": &step.id, "step_scope": step.resolved_scope(), "cycle": task_ctx.current_cycle, "pipeline_var_keys": pipeline_var_keys}),
        ).await?;

        // Layer 2 defense: delegate to the consolidated method on TaskExecutionStep.
        // If `step.builtin` names a known builtin, the method returns Builtin regardless
        // of what `behavior.execution` says, making dispatch robust against stale JSON.
        let effective_execution = step.effective_execution_mode();

        // Dispatch builtin steps (self_test, self_restart, ticket_scan) which handle
        // their own result capture and use `continue` semantics.
        let builtin_outcome = execute_builtin_step_dispatch(
            state,
            task_id,
            item_id,
            phase,
            step,
            &effective_execution,
            task_ctx,
            task_item_paths,
            &item.qa_file_path,
            acc,
        )
        .await?;

        match builtin_outcome {
            BuiltinStepOutcome::Handled => continue,
            BuiltinStepOutcome::EarlyReturn => return Ok(()),
            BuiltinStepOutcome::NotBuiltin => {}
        }

        // Execute chain or agent/generic steps, producing a RunResult.
        let agent_outcome = execute_agent_step(
            state,
            task_id,
            item_id,
            phase,
            step,
            &effective_execution,
            task_ctx,
            runtime,
            &item.qa_file_path,
            acc,
        )
        .await?;

        match agent_outcome {
            AgentStepOutcome::Handled => continue,
            AgentStepOutcome::Result(result) => {
                // Apply step results: capture, status transitions, post-actions,
                // artifact collection, events, and hard-failure check.
                let should_return = apply_step_results(
                    state,
                    task_id,
                    item_id,
                    phase,
                    step,
                    task_ctx,
                    task_item_paths,
                    &item.qa_file_path,
                    &result,
                    acc,
                )
                .await?;
                if should_return {
                    return Ok(());
                }
            }
        }
    }

    // Dynamic steps (only in full/legacy mode, not in segment-filtered mode)
    execute_dynamic_steps(state, task_id, item, task_ctx, runtime, step_filter, acc).await?;

    Ok(())
}

// ── Extracted sub-functions for process_item_filtered ──────────────

/// Outcome of dispatching a builtin step (self_test, self_restart, ticket_scan).
enum BuiltinStepOutcome {
    /// The builtin was recognized and fully handled; caller should `continue`.
    Handled,
    /// The builtin triggered an early return from the outer function.
    EarlyReturn,
    /// Not a recognized builtin dispatch; fall through to agent/generic execution.
    NotBuiltin,
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
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
    qa_file_path: &str,
    acc: &mut StepExecutionAccumulator,
) -> Result<BuiltinStepOutcome> {
    match effective_execution {
        ExecutionMode::Builtin { name } if name == "self_test" => {
            // Self-test uses a specialized builtin
            let exit_code =
                execute_self_test_step(&task_ctx.workspace_root, state, task_id, item_id)
                    .await
                    .unwrap_or(1);
            let passed = exit_code == 0;
            acc.pipeline_vars
                .vars
                .insert("self_test_exit_code".to_string(), exit_code.to_string());
            acc.pipeline_vars
                .vars
                .insert("self_test_passed".to_string(), passed.to_string());

            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": phase, "step_scope": step.resolved_scope(), "exit_code": exit_code, "success": passed}),
            ).await?;

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
            };
            acc.apply_captures(&step.behavior.captures, &step.id, &synth_result);
            Ok(BuiltinStepOutcome::Handled)
        }

        ExecutionMode::Builtin { name } if name == "self_restart" => {
            // Self-restart builtin: rebuild, verify, snapshot, then exit for relaunch
            let ws_root = std::path::Path::new(&task_ctx.workspace_root);
            let exit_code = execute_self_restart_step(ws_root, state, task_id, item_id)
                .await
                .unwrap_or(1);

            acc.pipeline_vars
                .vars
                .insert("self_restart_exit_code".to_string(), exit_code.to_string());

            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": phase, "step_scope": step.resolved_scope(), "exit_code": exit_code, "restart": exit_code == EXIT_RESTART}),
            ).await?;

            if exit_code == EXIT_RESTART {
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
                        acc.exit_codes.insert(step.id.clone(), exit_code as i64);
                        return Ok(BuiltinStepOutcome::EarlyReturn);
                    }
                }

                // Persist pipeline vars to DB before exit so the relaunched process recovers them.
                if let Ok(json) = serde_json::to_string(&acc.pipeline_vars) {
                    if let Err(e) = state
                        .db_writer
                        .update_task_pipeline_vars(task_id, &json)
                        .await
                    {
                        tracing::warn!("failed to persist pipeline_vars before restart: {e}");
                    }
                }
                // All state is persisted (restart_pending set by execute_self_restart_step).
                // Exit process so the wrapper script (orchestrator.sh) relaunches the new binary.
                std::process::exit(EXIT_RESTART as i32);
            }

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
            acc.exit_codes.insert(step.id.clone(), exit_code as i64);
            Ok(BuiltinStepOutcome::Handled)
        }

        ExecutionMode::Builtin { name } if name == "ticket_scan" => {
            // Ticket scan builtin (step_started already emitted above)
            let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
            acc.active_tickets = tickets.get(qa_file_path).cloned().unwrap_or_default();
            acc.new_ticket_count = acc.active_tickets.len() as i64;
            acc.step_ran.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": "ticket_scan", "step_scope": step.resolved_scope(), "tickets": acc.active_tickets.len()}),
            ).await?;
            Ok(BuiltinStepOutcome::Handled)
        }

        ExecutionMode::Builtin { name } if name == "item_select" => {
            // Selection orchestrated at loop_engine level; this is a marker step
            acc.step_ran.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": phase, "step_scope": step.resolved_scope(), "builtin": "item_select"}),
            )
            .await?;
            Ok(BuiltinStepOutcome::Handled)
        }

        _ => Ok(BuiltinStepOutcome::NotBuiltin),
    }
}

/// Outcome of executing a chain or agent/generic step.
#[allow(clippy::large_enum_variant)]
enum AgentStepOutcome {
    /// Chain step was fully handled (including event emission); caller should `continue`.
    Handled,
    /// A RunResult was produced and needs post-processing via `apply_step_results`.
    Result(crate::dto::RunResult),
}

/// Execute chain steps or agent/generic steps, producing either a handled outcome
/// (for chains) or a RunResult for further processing.
#[allow(clippy::too_many_arguments)]
async fn execute_agent_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    step: &TaskExecutionStep,
    effective_execution: &ExecutionMode,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    qa_file_path: &str,
    acc: &mut StepExecutionAccumulator,
) -> Result<AgentStepOutcome> {
    match effective_execution {
        ExecutionMode::Chain => {
            // Chain execution: run sub-steps in sequence
            let mut chain_passed = true;
            for chain_step in &step.chain_steps {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "chain_step_started",
                    json!({"step": phase, "step_scope": step.resolved_scope(), "chain_step": chain_step.id}),
                ).await?;

                let mut step_ctx = task_ctx.clone();
                step_ctx.pipeline_vars = acc.pipeline_vars.clone();

                let chain_exec = execute_builtin_step(
                    state,
                    task_id,
                    item_id,
                    chain_step,
                    &step_ctx,
                    runtime,
                    qa_file_path,
                )
                .await;

                let (chain_result, new_pipeline) = match chain_exec {
                    Ok(val) => val,
                    Err(e) => {
                        let _ = insert_event(
                            state,
                            task_id,
                            Some(item_id),
                            "chain_step_finished",
                            json!({"step": phase, "step_scope": step.resolved_scope(), "chain_step": chain_step.id, "error": e.to_string(), "success": false}),
                        ).await;
                        let _ = insert_event(
                            state,
                            task_id,
                            Some(item_id),
                            "step_finished",
                            json!({"step": phase, "step_scope": step.resolved_scope(), "error": e.to_string(), "success": false}),
                        ).await;
                        return Err(e);
                    }
                };
                acc.pipeline_vars = new_pipeline;

                if let Some(ref output) = chain_result.output {
                    if !output.stdout.is_empty() {
                        spill_large_var(
                            &state.logs_dir,
                            task_id,
                            "plan_output",
                            output.stdout.clone(),
                            &mut acc.pipeline_vars,
                        );
                    }
                }

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "chain_step_finished",
                    json!({
                        "step": phase,
                        "step_scope": step.resolved_scope(),
                        "chain_step": chain_step.id,
                        "exit_code": chain_result.exit_code,
                        "success": chain_result.is_success()
                    }),
                )
                .await?;

                if !chain_result.is_success() {
                    chain_passed = false;
                    acc.item_status = format!("{}_failed", chain_step.id);
                    break;
                }
            }
            acc.step_ran.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": phase, "step_scope": step.resolved_scope(), "success": chain_passed}),
            ).await?;
            Ok(AgentStepOutcome::Handled)
        }

        // ExecutionMode::Agent or ExecutionMode::Builtin for generic builtins
        _ => {
            let mut step_ctx = task_ctx.clone();
            step_ctx.pipeline_vars = acc.pipeline_vars.clone();

            let exec_result = execute_builtin_step(
                state,
                task_id,
                item_id,
                step,
                &step_ctx,
                runtime,
                qa_file_path,
            )
            .await;

            let (result, new_pipeline) = match exec_result {
                Ok(val) => val,
                Err(e) => {
                    let _ = insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "step_finished",
                        json!({"step": phase, "step_id": step.id, "step_scope": step.resolved_scope(), "error": e.to_string(), "success": false}),
                    ).await;
                    return Err(e);
                }
            };
            acc.pipeline_vars = new_pipeline;

            if let Some(ref output) = result.output {
                if !output.stdout.is_empty() {
                    let output_key = format!("{}_output", phase);
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
            },
        )
        .await?
    } else {
        let resolved_prompt = step.template.as_ref().and_then(|tmpl_name| {
            let cfg = state.active_config.read().ok()?;
            cfg.config
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
/// Only runs in full/legacy mode (not in segment-filtered mode).
async fn execute_dynamic_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    step_filter: Option<&HashSet<String>>,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    if task_ctx.dynamic_steps.is_empty() || step_filter.is_some() {
        return Ok(());
    }

    let item_id = item.id.as_str();
    let pool = {
        let mut p = crate::dynamic_orchestration::DynamicStepPool::new();
        for ds in &task_ctx.dynamic_steps {
            p.add_step(ds.clone());
        }
        p
    };
    let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, "dynamic");
    let dyn_ctx = crate::dynamic_orchestration::StepPrehookContext {
        task_id: prehook_ctx.task_id,
        task_item_id: prehook_ctx.task_item_id,
        cycle: prehook_ctx.cycle,
        step: "dynamic".to_string(),
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
    };
    let matched = pool.find_matching_steps(&dyn_ctx);
    for ds in matched {
        insert_event(
            state,
            task_id,
            Some(item_id),
            "dynamic_step_started",
            json!({"step_id": ds.id, "step_type": ds.step_type, "step_scope": "item", "priority": ds.priority}),
        ).await?;
        let cap = Some(ds.step_type.as_str());
        let result = run_phase_with_rotation(
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
                step_template_prompt: None,
                project_id: &task_ctx.project_id,
            },
        )
        .await?;
        insert_event(
            state,
            task_id,
            Some(item_id),
            "dynamic_step_finished",
            json!({"step_id": ds.id, "step_scope": "item", "exit_code": result.exit_code, "success": result.is_success()}),
        ).await?;
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
