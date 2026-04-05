use crate::scheduler::RunningTask;
use crate::scheduler::item_executor::execute_guard_step;
use crate::scheduler::task_state::{
    count_unresolved_items, record_task_execution_metric, set_task_status,
};
use agent_orchestrator::config::{ConvergenceContext, InvariantCheckPoint, LoopMode};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::prehook::evaluate_convergence_expression;
use agent_orchestrator::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

/// Execute guard/gate steps and check termination conditions.
/// Returns `true` if the task should terminate (guard triggered early completion).
pub(super) async fn execute_guard_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<bool> {
    for step in &task_ctx.execution_plan.steps {
        if !step.is_guard {
            continue;
        }
        if !step.repeatable && task_ctx.current_cycle > 1 {
            continue;
        }

        let guard_result = execute_guard_step(state, task_id, step, task_ctx, runtime).await?;
        if guard_result.should_stop {
            insert_event(
                state,
                task_id,
                None,
                "workflow_terminated",
                json!({
                    "cycle": task_ctx.current_cycle,
                    "guard_step": step.id,
                    "reason": guard_result.reason
                }),
            )
            .await?;
            state.emit_event(
                task_id,
                None,
                "workflow_terminated",
                json!({"guard_step": step.id}),
            );

            // Run before_complete invariant check before finalizing
            if let Some(action) = super::cycle_safety::check_invariants(
                state,
                task_id,
                task_ctx,
                InvariantCheckPoint::BeforeComplete,
            )
            .await?
            {
                if action == "halt" {
                    set_task_status(state, task_id, "failed", false).await?;
                    insert_event(
                        state,
                        task_id,
                        None,
                        "task_failed",
                        json!({"reason": "invariant_halt_before_complete"}),
                    )
                    .await?;
                    let unresolved = count_unresolved_items(state, task_id).await?;
                    record_task_execution_metric(
                        state,
                        task_id,
                        "failed",
                        task_ctx.current_cycle,
                        unresolved,
                    )
                    .await?;
                    return Ok(true);
                }
            }

            set_task_status(state, task_id, "completed", true).await?;
            insert_event(state, task_id, None, "task_completed", json!({})).await?;
            state.emit_event_with_project(
                task_id,
                None,
                "task_completed",
                json!({}),
                Some(task_ctx.project_id.clone()),
            );
            let unresolved = count_unresolved_items(state, task_id).await?;
            record_task_execution_metric(
                state,
                task_id,
                "completed",
                task_ctx.current_cycle,
                unresolved,
            )
            .await?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Evaluate the loop continuation strategy (Fixed/Infinite/Once), emit the
/// loop_guard_decision event, and return whether the loop should continue.
pub(super) async fn evaluate_loop_continuation(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
) -> Result<bool> {
    let unresolved = count_unresolved_items(state, task_id).await?;
    let loop_mode_check = evaluate_loop_guard_rules(
        &task_ctx.execution_plan.loop_policy,
        task_ctx.current_cycle,
        unresolved,
    );

    let (mut should_continue, mut reason) = if let Some((cont, r)) = loop_mode_check {
        (cont, r)
    } else if task_ctx
        .execution_plan
        .loop_policy
        .guard
        .stop_when_no_unresolved
        && unresolved == 0
    {
        (false, "no_unresolved_items".to_string())
    } else {
        (true, "continue".to_string())
    };

    // FR-043: Evaluate convergence expressions when the loop would otherwise continue.
    if should_continue {
        if let Some(exprs) = &task_ctx.execution_plan.loop_policy.convergence_expr {
            let conv_ctx = ConvergenceContext {
                cycle: task_ctx.current_cycle,
                active_ticket_count: unresolved,
                self_test_passed: task_ctx
                    .pipeline_vars
                    .vars
                    .get("self_test_passed")
                    .map(|v| v == "true")
                    .unwrap_or(false),
                max_cycles: task_ctx
                    .execution_plan
                    .loop_policy
                    .guard
                    .max_cycles
                    .unwrap_or(0),
                vars: task_ctx.pipeline_vars.vars.clone(),
            };
            for entry in exprs {
                match evaluate_convergence_expression(entry.when.trim(), &conv_ctx) {
                    Ok(true) => {
                        should_continue = false;
                        reason = entry
                            .reason
                            .clone()
                            .unwrap_or_else(|| "convergence_expr".to_string());
                        break;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        tracing::warn!(
                            task_id,
                            cycle = task_ctx.current_cycle,
                            expr = entry.when.as_str(),
                            "convergence_expr evaluation error: {}",
                            e
                        );
                    }
                }
            }
        }
    }
    insert_event(
        state,
        task_id,
        None,
        "loop_guard_decision",
        json!({
            "cycle": task_ctx.current_cycle,
            "continue": should_continue,
            "reason": reason,
            "unresolved_items": unresolved
        }),
    )
    .await?;
    state.emit_event(
        task_id,
        None,
        "loop_guard_decision",
        json!({
            "cycle": task_ctx.current_cycle,
            "continue": should_continue,
            "reason": reason,
            "unresolved_items": unresolved
        }),
    );

    Ok(should_continue)
}

/// Evaluates loop-guard policy and returns the next continue/stop decision when resolvable.
pub fn evaluate_loop_guard_rules(
    loop_policy: &agent_orchestrator::config::WorkflowLoopConfig,
    current_cycle: u32,
    _unresolved: i64,
) -> Option<(bool, String)> {
    match loop_policy.mode {
        LoopMode::Once => Some((false, "once_mode".to_string())),
        LoopMode::Fixed => {
            let max = loop_policy.guard.max_cycles.unwrap_or(1);
            if current_cycle >= max {
                Some((false, "fixed_cycles_complete".to_string()))
            } else {
                Some((true, "fixed_cycle_continue".to_string()))
            }
        }
        LoopMode::Infinite => {
            if let Some(max_cycles) = loop_policy.guard.max_cycles {
                if current_cycle >= max_cycles {
                    return Some((false, "max_cycles_reached".to_string()));
                }
            }
            if !loop_policy.guard.enabled {
                return Some((true, "guard_disabled".to_string()));
            }
            None
        }
    }
}
