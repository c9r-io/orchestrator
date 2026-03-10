mod continuation;
mod cycle_safety;
mod segment;
#[cfg(test)]
mod tests;

pub use continuation::evaluate_loop_guard_rules;

use crate::config::{InvariantCheckPoint, StepScope};
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::item_executor::{process_item, StepExecutionAccumulator};
use super::phase_runner::{run_phase_with_rotation, RotatingPhaseRunRequest};
use super::runtime::load_task_runtime_context;
use super::task_state::{
    count_unresolved_items, first_task_item_id, is_task_paused_in_db, list_task_items_for_cycle,
    record_task_execution_metric, set_task_status, update_task_cycle_state,
};
use super::RunningTask;

pub async fn run_task_loop(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    set_task_status(&state, task_id, "running", false).await?;
    let result = run_task_loop_core(state.clone(), task_id, runtime).await;
    if let Err(ref e) = result {
        let _ = set_task_status(&state, task_id, "failed", false).await;
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        )
        .await;
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        );
        let unresolved = count_unresolved_items(&state, task_id).await.unwrap_or(0);
        let _ = record_task_execution_metric(&state, task_id, "failed", 0, unresolved).await;
    }
    result
}

/// Signal returned by `execute_cycle_segments` to indicate whether the caller
/// should restart the cycle loop (equivalent to the old `continue 'cycle`).
enum CycleSegmentOutcome {
    /// All segments completed normally; proceed to post-cycle logic.
    Completed,
    /// A stop/pause condition was detected mid-segment; restart the cycle loop.
    RestartCycle,
}

async fn run_task_loop_core(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    let mut task_ctx = load_task_runtime_context(&state, task_id).await?;

    run_init_once_if_needed(&state, task_id, &mut task_ctx, &runtime).await?;

    'cycle: loop {
        if is_task_paused_in_db(&state, task_id).await? {
            let unresolved = count_unresolved_items(&state, task_id).await?;
            record_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )
            .await?;
            return Ok(());
        }

        if runtime.stop_flag.load(Ordering::SeqCst) {
            set_task_status(&state, task_id, "paused", false).await?;
            insert_event(
                &state,
                task_id,
                None,
                "task_paused",
                json!({"reason":"stop_flag"}),
            )
            .await?;
            state.emit_event(task_id, None, "task_paused", json!({}));
            let unresolved = count_unresolved_items(&state, task_id).await?;
            record_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )
            .await?;
            return Ok(());
        }

        task_ctx.current_cycle += 1;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, task_ctx.init_done)
            .await?;
        let max_cycles = task_ctx.execution_plan.loop_policy.guard.max_cycles;
        insert_event(
            &state,
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle, "max_cycles": max_cycles}),
        )
        .await?;
        state.emit_event(
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle, "max_cycles": max_cycles}),
        );

        let outcome = execute_cycle_segments(&state, task_id, &mut task_ctx, &runtime).await?;
        if matches!(outcome, CycleSegmentOutcome::RestartCycle) {
            continue 'cycle;
        }

        let cycle_unresolved = count_unresolved_items(&state, task_id).await?;
        if cycle_unresolved > 0 {
            task_ctx.consecutive_failures += 1;
        } else {
            task_ctx.consecutive_failures = 0;
        }

        cycle_safety::apply_auto_rollback_if_needed(&state, task_id, &mut task_ctx).await?;

        if continuation::execute_guard_steps(&state, task_id, &task_ctx, &runtime).await? {
            return Ok(());
        }

        if !continuation::evaluate_loop_continuation(&state, task_id, &task_ctx).await? {
            break;
        }
    }

    // Invariant checkpoint: before_complete
    if let Some(action) = cycle_safety::check_invariants(
        &state,
        task_id,
        &task_ctx,
        InvariantCheckPoint::BeforeComplete,
    )
    .await?
    {
        if action == "halt" {
            set_task_status(&state, task_id, "failed", false).await?;
            insert_event(
                &state,
                task_id,
                None,
                "task_failed",
                json!({"reason": "invariant_halt_before_complete"}),
            )
            .await?;
            let unresolved = count_unresolved_items(&state, task_id).await?;
            record_task_execution_metric(
                &state,
                task_id,
                "failed",
                task_ctx.current_cycle,
                unresolved,
            )
            .await?;
            return Ok(());
        }
        // rollback at before_complete is treated as warn-only
    }

    let unresolved = count_unresolved_items(&state, task_id).await?;
    if is_task_paused_in_db(&state, task_id).await? {
        return Ok(());
    }

    if unresolved > 0 {
        set_task_status(&state, task_id, "failed", true).await?;
        insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        )
        .await?;
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        );
        record_task_execution_metric(
            &state,
            task_id,
            "failed",
            task_ctx.current_cycle,
            unresolved,
        )
        .await?;
    } else {
        set_task_status(&state, task_id, "completed", true).await?;
        insert_event(&state, task_id, None, "task_completed", json!({})).await?;
        state.emit_event(task_id, None, "task_completed", json!({}));
        record_task_execution_metric(
            &state,
            task_id,
            "completed",
            task_ctx.current_cycle,
            unresolved,
        )
        .await?;
    }

    Ok(())
}

/// Execute the init_once step if it has not been run yet.
async fn run_init_once_if_needed(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<()> {
    if task_ctx.init_done {
        return Ok(());
    }

    if let Some(step) = task_ctx.execution_plan.step_by_id("init_once") {
        if let Some(anchor_item_id) = first_task_item_id(state, task_id).await? {
            insert_event(
                state,
                task_id,
                Some(&anchor_item_id),
                "step_started",
                json!({"step":"init_once", "step_scope": "task"}),
            )
            .await?;
            let init_result = run_phase_with_rotation(
                state,
                RotatingPhaseRunRequest {
                    task_id,
                    item_id: &anchor_item_id,
                    step_id: &step.id,
                    phase: "init_once",
                    tty: step.tty,
                    capability: step.required_capability.as_deref(),
                    rel_path: ".",
                    ticket_paths: &[],
                    workspace_root: &task_ctx.workspace_root,
                    workspace_id: &task_ctx.workspace_id,
                    cycle: task_ctx.current_cycle,
                    runtime,
                    pipeline_vars: None,
                    step_timeout_secs: task_ctx.safety.step_timeout_secs,
                    step_scope: StepScope::Task,
                    step_template_prompt: None,
                    project_id: &task_ctx.project_id,
                    execution_profile: step.execution_profile.as_deref(),
                },
            )
            .await?;
            if !init_result.is_success() {
                anyhow::bail!("init_once failed: exit={}", init_result.exit_code);
            }
            insert_event(
                state,
                task_id,
                Some(&anchor_item_id),
                "step_finished",
                json!({"step":"init_once","step_scope":"task","exit_code":init_result.exit_code}),
            )
            .await?;
        }
    }
    task_ctx.init_done = true;
    update_task_cycle_state(state, task_id, task_ctx.current_cycle, true).await?;

    Ok(())
}

/// Create checkpoint, dispatch segment-based execution (task-scoped and item-scoped steps),
/// and finalize item execution. Returns `CycleSegmentOutcome::RestartCycle` when a
/// stop/pause condition is detected mid-segment.
async fn execute_cycle_segments(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<CycleSegmentOutcome> {
    cycle_safety::create_cycle_checkpoint(state, task_id, task_ctx).await?;

    // Invariant checkpoint: before_cycle
    if let Some(action) =
        cycle_safety::check_invariants(state, task_id, task_ctx, InvariantCheckPoint::BeforeCycle)
            .await?
    {
        match action {
            "halt" => {
                set_task_status(state, task_id, "failed", false).await?;
                anyhow::bail!("invariant halt at before_cycle checkpoint");
            }
            _ => {
                // rollback → restart cycle
                return Ok(CycleSegmentOutcome::RestartCycle);
            }
        }
    }

    let mut items = list_task_items_for_cycle(state, task_id).await?;
    let mut task_item_paths: Vec<String> =
        items.iter().map(|item| item.qa_file_path.clone()).collect();

    let segments = segment::build_scope_segments(task_ctx);
    if segments.is_empty() {
        // Fallback: no steps in execution plan, run the whole-cycle path.
        for item in &items {
            process_item(state, task_id, item, &task_item_paths, task_ctx, runtime).await?;
            if runtime.stop_flag.load(Ordering::SeqCst)
                || is_task_paused_in_db(state, task_id).await?
            {
                return Ok(CycleSegmentOutcome::RestartCycle);
            }
        }
    } else {
        let mut item_state: HashMap<String, StepExecutionAccumulator> = HashMap::new();
        let mut halt_after_task_segment = false;
        for (segment_idx, seg) in segments.iter().enumerate() {
            match seg.scope {
                StepScope::Task => {
                    let outcome = segment::execute_task_segment(
                        state,
                        task_id,
                        task_ctx,
                        runtime,
                        seg,
                        segment_idx,
                        &segments,
                        &mut items,
                        &mut item_state,
                        &mut task_item_paths,
                    )
                    .await?;
                    match outcome {
                        segment::TaskSegmentOutcome::HaltAfterSegment => {
                            halt_after_task_segment = true;
                        }
                        segment::TaskSegmentOutcome::InvariantRollback => {
                            return Ok(CycleSegmentOutcome::Completed);
                        }
                        segment::TaskSegmentOutcome::Continue => {}
                    }
                }
                StepScope::Item => {
                    segment::execute_item_segment(
                        state,
                        task_id,
                        task_ctx,
                        runtime,
                        seg,
                        segment_idx,
                        &segments,
                        &items,
                        &mut item_state,
                        &task_item_paths,
                    )
                    .await?;

                    segment::try_item_selection(
                        state,
                        task_id,
                        task_ctx,
                        segment_idx,
                        &segments,
                        &mut items,
                        &item_state,
                        &mut task_item_paths,
                    )
                    .await?;
                }
            }
            if halt_after_task_segment {
                break;
            }
            if runtime.stop_flag.load(Ordering::SeqCst)
                || is_task_paused_in_db(state, task_id).await?
            {
                return Ok(CycleSegmentOutcome::RestartCycle);
            }
        }

        segment::finalize_items(state, task_id, task_ctx, &items, &mut item_state).await?;
    }

    Ok(CycleSegmentOutcome::Completed)
}
