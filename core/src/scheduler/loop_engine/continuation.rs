use crate::config::{InvariantCheckPoint, LoopMode};
use crate::events::insert_event;
use crate::scheduler::item_executor::execute_guard_step;
use crate::scheduler::task_state::{
    count_unresolved_items, record_task_execution_metric, set_task_status,
};
use crate::scheduler::RunningTask;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

/// Execute guard/gate steps and check termination conditions.
/// Returns `true` if the task should terminate (guard triggered early completion).
pub(super) async fn execute_guard_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
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
            state.emit_event(task_id, None, "task_completed", json!({}));
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
    task_ctx: &crate::config::TaskRuntimeContext,
) -> Result<bool> {
    let unresolved = count_unresolved_items(state, task_id).await?;
    let loop_mode_check = evaluate_loop_guard_rules(
        &task_ctx.execution_plan.loop_policy,
        task_ctx.current_cycle,
        unresolved,
    );

    let should_continue = if let Some((continue_loop, _)) = loop_mode_check {
        continue_loop
    } else if task_ctx
        .execution_plan
        .loop_policy
        .guard
        .stop_when_no_unresolved
    {
        unresolved > 0
    } else {
        true
    };

    let reason = if let Some((_, reason)) = loop_mode_check {
        reason
    } else if !should_continue {
        "no_unresolved_items".to_string()
    } else {
        "continue".to_string()
    };
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
    loop_policy: &crate::config::WorkflowLoopConfig,
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
