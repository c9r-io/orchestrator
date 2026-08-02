use agent_orchestrator::config::{ExecutionMode, TaskExecutionStep, TaskRuntimeContext};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::prehook::evaluate_step_prehook;
use agent_orchestrator::state::InnerState;
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::super::RunningTask;
use super::accumulator::StepExecutionAccumulator;
use super::apply::apply_step_results;
use super::dispatch_builtin::{
    BuiltinStepContext, BuiltinStepOutcome, execute_builtin_step, execute_builtin_step_dispatch,
};
use super::dispatch_dynamic::execute_dynamic_steps;
use super::finalize::finalize_item_execution;
use super::spill::spill_large_var;

pub struct ProcessItemRequest<'a> {
    pub task_id: &'a str,
    pub item: &'a agent_orchestrator::dto::TaskItemRow,
    pub task_item_paths: &'a [String],
    pub task_ctx: &'a TaskRuntimeContext,
    pub runtime: &'a RunningTask,
    pub step_filter: Option<&'a HashSet<String>>,
    pub run_dynamic_steps: bool,
}

/// Owned variant of ProcessItemRequest for tokio::spawn (requires 'static).
pub struct OwnedProcessItemRequest {
    pub task_id: String,
    pub item: agent_orchestrator::dto::TaskItemRow,
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

/// Processes one task item through all configured steps and finalizes its outcome.
pub async fn process_item(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
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
        |step_id: &str| -> bool { step_filter.is_none_or(|f| f.contains(step_id)) };
    acc.merge_task_pipeline_vars(&task_ctx.pipeline_vars);

    // Inject dynamic item variables (from generate_items) into pipeline vars
    if let Some(ref label) = item.label {
        acc.pipeline_vars
            .vars
            .insert("item_label".to_string(), label.clone());
    }
    if let Some(ref vars_json) = item.dynamic_vars_json
        && let Ok(vars) =
            serde_json::from_str::<std::collections::HashMap<String, String>>(vars_json)
    {
        for (k, v) in vars {
            acc.pipeline_vars.vars.insert(k, v);
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
        // Skip steps that already completed before a self_restart in this cycle.
        if task_ctx.restart_completed_steps.contains(&step.id) {
            insert_event(
                state,
                task_id,
                Some(&item.id),
                "step_skipped",
                json!({"step": step.id, "reason": "already_completed_before_restart"}),
            )
            .await?;
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

// ── Internal types and helpers ───────────────────────────────────────

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
    item: &'a agent_orchestrator::dto::TaskItemRow,
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

// apply_step_vars_overlay / restore_step_vars_overlay lived here until FR-156.
// They were the only readers of WorkflowStepConfig::step_vars, which apply now
// rejects; their three PipelineVariables signatures are the whole of that FR's
// reachable source-ratchet reduction.

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

fn synthetic_chain_result(
    step: &TaskExecutionStep,
    success: bool,
) -> agent_orchestrator::dto::RunResult {
    let output = agent_orchestrator::collab::AgentOutput::new(
        uuid::Uuid::new_v4(),
        "chain".to_string(),
        step.id.clone(),
        if success { 0 } else { 1 },
        String::new(),
        String::new(),
    );
    agent_orchestrator::dto::RunResult {
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

// ── Core step dispatcher ─────────────────────────────────────────────

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

        // Built-in safety: skip self-referential-unsafe docs globally,
        // unless they have scenario-level safe annotations.
        // Only applies to item-scoped steps — task-scoped steps operate on the
        // whole workspace and should not be gated by the anchor item's QA file.
        if task_ctx.self_referential
            && step.resolved_scope() == agent_orchestrator::config::StepScope::Item
            && !agent_orchestrator::ticket::is_self_referential_safe(
                &task_ctx.workspace_root,
                &item.qa_file_path,
                true,
            )
            && agent_orchestrator::ticket::get_self_referential_safe_scenarios(
                &task_ctx.workspace_root,
                &item.qa_file_path,
                true,
            )
            .is_empty()
        {
            acc.step_skipped.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                build_step_skipped_payload(step, "self_referential_unsafe", event_ctx.parent_step),
            )
            .await?;
            return Ok(StepExecutionOutcome::Completed { success: true });
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
                return Err(super::super::safety::RestartRequestedError { binary_path }.into());
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

// ── Agent/chain step execution ───────────────────────────────────────

/// Outcome of executing a chain or agent/generic step.
#[allow(clippy::large_enum_variant)]
enum AgentStepOutcome {
    /// The step requested an early return from the item execution loop.
    EarlyReturn,
    /// A RunResult was produced and needs post-processing via `apply_step_results`.
    Result(agent_orchestrator::dto::RunResult),
}

/// Execute chain steps or agent/generic steps, producing either a handled outcome
/// (for chains) or a RunResult for further processing.
#[allow(clippy::too_many_arguments)]
async fn execute_agent_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
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
                match execute_step(
                    state,
                    StepExecutionRequest {
                        task_id,
                        item,
                        task_item_paths,
                        event_ctx: StepEventContext::chain_child(&step.id),
                        step: chain_step,
                        task_ctx,
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
            // No step_vars overlay: FR-156 retired the field, and apply rejects
            // any manifest that still carries it.
            let effective_pipeline = acc.pipeline_vars.clone();

            let exec_result = execute_builtin_step(
                state,
                task_id,
                item.id.as_str(),
                step,
                BuiltinStepContext {
                    task_ctx,
                    pipeline_vars: &effective_pipeline,
                    runtime,
                    rel_path: &item.qa_file_path,
                    workspace_root: crate::scheduler::loop_engine::isolation::step_workspace_root(
                        task_ctx,
                        &effective_pipeline,
                        step.resolved_scope(),
                    ),
                },
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

            if let Some(ref output) = result.output
                && !output.stdout.is_empty()
            {
                let output_key = format!("{}_output", step.id);
                // Extract result text from stream-json output when available;
                // fall back to raw stdout for non-stream-json agents.
                let effective_output =
                    agent_orchestrator::json_extract::extract_stream_json_result(&output.stdout)
                        .unwrap_or_else(|| output.stdout.clone());
                spill_large_var(
                    &task_ctx.artifacts_dir,
                    task_id,
                    &output_key,
                    effective_output,
                    &mut acc.pipeline_vars,
                );
            }

            Ok(AgentStepOutcome::Result(result))
        }
    }
}
