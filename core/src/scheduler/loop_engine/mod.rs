mod continuation;
mod cycle_safety;
mod graph;
pub(crate) mod isolation;
mod segment;
#[cfg(test)]
mod tests;

pub use continuation::evaluate_loop_guard_rules;

use crate::config::{InvariantCheckPoint, StepScope};
use chrono::TimeZone;
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::item_executor::{process_item, StepExecutionAccumulator};
use super::task_state::query_recent_cycle_timestamps;
use super::phase_runner::{run_phase_with_rotation, RotatingPhaseRunRequest};
use super::runtime::load_task_runtime_context;
use super::safety::RestartRequestedError;
use super::item_executor::finalize_item_execution;
use super::task_state::{
    count_stale_pending_items, count_unresolved_items, find_completed_runs_for_pending_items,
    find_inflight_command_runs_for_task, first_task_item_id, is_task_paused_in_db,
    list_task_items_for_cycle, record_task_execution_metric, set_task_status,
    update_task_cycle_state,
};
use super::RunningTask;

/// Runs the main workflow loop for a task until completion, pause, or failure.
pub async fn run_task_loop(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    set_task_status(&state, task_id, "running", false).await?;
    let result = run_task_loop_core(state.clone(), task_id, runtime).await;
    if let Ok(task_ctx) = load_task_runtime_context(&state, task_id).await {
        let _ = isolation::cleanup_task_isolation(&state, task_id, &task_ctx).await;
    }
    if let Err(ref e) = result {
        // RestartRequestedError is not a failure — the self_restart step already
        // set the task to "restart_pending".  Propagate the error so the daemon
        // can exec() the new binary; do NOT overwrite the status to "failed".
        if e.downcast_ref::<RestartRequestedError>().is_none() {
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
            let _ =
                record_task_execution_metric(&state, task_id, "failed", 0, unresolved).await;
        }
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

        // FR-037: Proactive max_cycles enforcement — prevent cycle overflow from
        // dynamic items that bypass the post-cycle continuation check.
        {
            let proactive_max = proactive_max_cycles(&task_ctx.execution_plan.loop_policy);
            if task_ctx.current_cycle >= proactive_max {
                insert_event(
                    &state,
                    task_id,
                    None,
                    "max_cycles_enforced",
                    json!({
                        "current_cycle": task_ctx.current_cycle,
                        "max_cycles": proactive_max,
                    }),
                )
                .await?;
                state.emit_event(
                    task_id,
                    None,
                    "max_cycles_enforced",
                    json!({
                        "current_cycle": task_ctx.current_cycle,
                        "max_cycles": proactive_max,
                    }),
                );
                break;
            }
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

        // FR-035 L2: Rapid cycle detection — pause task if last 3 cycles were too fast
        if task_ctx.current_cycle >= 4 {
            if let Ok(true) = detect_rapid_cycles(
                &state,
                task_id,
                &task_ctx,
            )
            .await
            {
                insert_event(
                    &state,
                    task_id,
                    None,
                    "degenerate_cycle_detected",
                    json!({
                        "cycle": task_ctx.current_cycle,
                        "min_cycle_interval_secs": task_ctx.safety.min_cycle_interval_secs,
                    }),
                )
                .await?;
                state.emit_event(
                    task_id,
                    None,
                    "degenerate_cycle_detected",
                    json!({"cycle": task_ctx.current_cycle}),
                );
                set_task_status(&state, task_id, "paused", false).await?;
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
        }

        let outcome = match task_ctx.execution.mode {
            crate::config::WorkflowExecutionMode::StaticSegment => {
                execute_cycle_segments(&state, task_id, &mut task_ctx, &runtime).await?
            }
            crate::config::WorkflowExecutionMode::DynamicDag => {
                match graph::execute_cycle_graph(&state, task_id, &mut task_ctx, &runtime).await? {
                    graph::GraphCycleOutcome::Completed => CycleSegmentOutcome::Completed,
                    graph::GraphCycleOutcome::RestartCycle => CycleSegmentOutcome::RestartCycle,
                    graph::GraphCycleOutcome::FallbackToStaticSegment => {
                        execute_cycle_segments(&state, task_id, &mut task_ctx, &runtime).await?
                    }
                }
            }
        };
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

    // FR-038: Wait for in-flight command runs before deciding task fate.
    wait_for_inflight_runs(&state, task_id).await?;

    // FR-038: Compensate pending items whose runs completed during recovery.
    let compensated = compensate_pending_items(&state, task_id, &task_ctx).await?;
    if compensated > 0 {
        insert_event(
            &state,
            task_id,
            None,
            "items_compensated",
            json!({"count": compensated}),
        )
        .await?;
    }

    let unresolved = count_unresolved_items(&state, task_id).await?;
    let stale_pending = count_stale_pending_items(&state, task_id).await?;
    let effective_unresolved = unresolved + stale_pending;

    if is_task_paused_in_db(&state, task_id).await? {
        return Ok(());
    }

    if effective_unresolved > 0 {
        set_task_status(&state, task_id, "failed", true).await?;
        insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved, "stale_pending_items": stale_pending}),
        )
        .await?;
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved, "stale_pending_items": stale_pending}),
        );
        record_task_execution_metric(
            &state,
            task_id,
            "failed",
            task_ctx.current_cycle,
            effective_unresolved,
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
            effective_unresolved,
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
                    self_referential: task_ctx.self_referential,
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

    let all_items = list_task_items_for_cycle(state, task_id).await?;
    // When dynamic items exist (created by generate_items post-action, possibly
    // before a self_restart), narrow to only dynamic items so that item-scoped
    // segments (e.g. qa_testing) process only the selected subset.
    let has_dynamic = all_items.iter().any(|i| i.source == "dynamic");
    let mut items: Vec<_> = if has_dynamic {
        all_items
            .into_iter()
            .filter(|i| i.source == "dynamic")
            .collect()
    } else {
        all_items
    };
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

/// FR-035 L2: Checks if the last 3 cycle intervals were all shorter than
/// `safety.min_cycle_interval_secs`, indicating a degenerate loop.
/// Returns `Ok(true)` when rapid cycles are detected.
async fn detect_rapid_cycles(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
) -> Result<bool> {
    // Need at least 4 timestamps to compute 3 intervals
    let timestamps = query_recent_cycle_timestamps(state, task_id, 4).await?;
    if timestamps.len() < 4 {
        return Ok(false);
    }

    let min_interval = task_ctx.safety.min_cycle_interval_secs as i64;
    let parsed: Vec<_> = timestamps
        .iter()
        .filter_map(|ts| parse_cycle_timestamp(ts))
        .collect();
    if parsed.len() < 4 {
        return Ok(false);
    }

    // Timestamps are newest-first from DB; reverse to oldest-first for interval computation
    let mut sorted = parsed;
    sorted.reverse();

    for pair in sorted.windows(2) {
        let interval = pair[1].signed_duration_since(pair[0]).num_seconds().abs();
        if interval >= min_interval {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Parses a DB event timestamp into a chrono DateTime.
fn parse_cycle_timestamp(ts: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
        return Some(parsed);
    }
    let zero_offset = chrono::FixedOffset::east_opt(0)?;
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(ts, fmt) {
            return Some(zero_offset.from_utc_datetime(&naive));
        }
    }
    None
}

/// FR-037: Compute the proactive max-cycles limit for a given loop policy.
///
/// Returns the cycle ceiling: if `current_cycle >= proactive_max`, the loop
/// must NOT increment and should break immediately.
pub(crate) fn proactive_max_cycles(policy: &crate::config::WorkflowLoopConfig) -> u32 {
    match policy.mode {
        crate::config::LoopMode::Fixed => policy.guard.max_cycles.unwrap_or(1),
        crate::config::LoopMode::Infinite => policy.guard.max_cycles.unwrap_or(u32::MAX),
        _ => u32::MAX, // Once mode: handled by evaluate_loop_guard_rules
    }
}

/// FR-038: Wait for in-flight command runs to finish before deciding task fate.
///
/// Polls for up to 120 seconds (2-second intervals) checking whether any
/// command runs for this task still have `exit_code = -1` (active). If all
/// runs complete or their PIDs are dead, returns immediately.
async fn wait_for_inflight_runs(state: &Arc<InnerState>, task_id: &str) -> Result<()> {
    let inflight = find_inflight_command_runs_for_task(state, task_id).await?;
    if inflight.is_empty() {
        return Ok(());
    }

    let pids: Vec<i64> = inflight.iter().filter_map(|(_, _, _, pid)| *pid).collect();
    insert_event(
        state,
        task_id,
        None,
        "inflight_runs_detected",
        json!({ "count": inflight.len(), "pids": pids }),
    )
    .await?;
    state.emit_event(
        task_id,
        None,
        "inflight_runs_detected",
        json!({ "count": inflight.len(), "pids": pids }),
    );

    let timeout = std::time::Duration::from_secs(120);
    let poll_interval = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() >= timeout {
            insert_event(
                state,
                task_id,
                None,
                "inflight_wait_timeout",
                json!({ "elapsed_secs": start.elapsed().as_secs() }),
            )
            .await?;
            break;
        }

        tokio::time::sleep(poll_interval).await;

        let remaining = find_inflight_command_runs_for_task(state, task_id).await?;
        if remaining.is_empty() {
            break;
        }

        // Check if all known PIDs are dead
        let all_dead = remaining.iter().all(|(_, _, _, pid)| {
            pid.map_or(true, |p| unsafe { libc::kill(p as i32, 0) } != 0)
        });
        if all_dead {
            break;
        }
    }

    Ok(())
}

/// FR-038: Compensate pending items whose command runs completed after recovery.
///
/// Reconstructs a `StepExecutionAccumulator` from DB records and calls
/// `finalize_item_execution` to properly transition item status.
async fn compensate_pending_items(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
) -> Result<u32> {
    let completed_runs = find_completed_runs_for_pending_items(state, task_id).await?;
    if completed_runs.is_empty() {
        return Ok(0);
    }

    // Group by item_id
    let mut grouped: std::collections::BTreeMap<String, Vec<&crate::task_repository::CompletedRunRecord>> =
        std::collections::BTreeMap::new();
    for run in &completed_runs {
        grouped
            .entry(run.task_item_id.clone())
            .or_default()
            .push(run);
    }

    let all_items = list_task_items_for_cycle(state, task_id).await?;
    let mut compensated = 0u32;

    for (item_id, runs) in &grouped {
        let Some(item) = all_items.iter().find(|i| i.id == *item_id) else {
            continue;
        };

        let mut acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());

        for run in runs {
            acc.exit_codes
                .insert(run.phase.clone(), run.exit_code);
            acc.step_ran.insert(run.phase.clone(), true);

            // Populate qa-specific fields
            if run.phase == "qa_testing" || run.phase == "qa" {
                acc.qa_confidence = run.confidence.map(|v| v as f32);
                acc.qa_quality_score = run.quality_score.map(|v| v as f32);
                if run.exit_code != 0 {
                    acc.flags.insert("qa_failed".to_string(), true);
                }
            }
            if run.phase == "fix" || run.phase == "ticket_fix" {
                acc.fix_confidence = run.confidence.map(|v| v as f32);
                acc.fix_quality_score = run.quality_score.map(|v| v as f32);
                if run.exit_code == 0 {
                    acc.flags.insert("fix_success".to_string(), true);
                }
            }
            if run.phase == "retest" && run.exit_code == 0 {
                acc.flags.insert("retest_success".to_string(), true);
            }
        }

        finalize_item_execution(state, task_id, item, task_ctx, &mut acc).await?;

        insert_event(
            state,
            task_id,
            Some(item_id),
            "item_compensated",
            json!({
                "phases": runs.iter().map(|r| r.phase.as_str()).collect::<Vec<_>>(),
                "final_status": acc.item_status,
            }),
        )
        .await?;
        state.emit_event(
            task_id,
            Some(item_id),
            "item_compensated",
            json!({
                "phases": runs.iter().map(|r| r.phase.as_str()).collect::<Vec<_>>(),
                "final_status": acc.item_status,
            }),
        );

        compensated += 1;
    }

    Ok(compensated)
}
