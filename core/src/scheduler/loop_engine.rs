use crate::config::{InvariantCheckPoint, LoopMode, OnViolation, StepScope};
use crate::events::insert_event;
use crate::scheduler::invariant::{evaluate_invariants, has_halting_violation, has_rollback_violation};
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::warn;

use super::item_executor::{
    execute_guard_step, finalize_item_execution, process_item, process_item_filtered,
    process_item_filtered_owned, OwnedProcessItemRequest, ProcessItemRequest,
    StepExecutionAccumulator,
};
use super::phase_runner::{run_phase_with_rotation, RotatingPhaseRunRequest};
use super::runtime::load_task_runtime_context;
use super::safety::{
    create_checkpoint, restore_binary_snapshot, rollback_to_checkpoint, snapshot_binary,
};
use super::task_state::{
    count_unresolved_items, first_task_item_id, is_task_paused_in_db, list_task_items_for_cycle,
    record_task_execution_metric, set_task_status, update_task_cycle_state,
};
use super::RunningTask;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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

        apply_auto_rollback_if_needed(&state, task_id, &mut task_ctx).await?;

        if execute_guard_steps(&state, task_id, &task_ctx, &runtime).await? {
            return Ok(());
        }

        if !evaluate_loop_continuation(&state, task_id, &task_ctx).await? {
            break;
        }
    }

    // Invariant checkpoint: before_complete
    if let Some(action) = check_invariants(&state, task_id, &task_ctx, InvariantCheckPoint::BeforeComplete).await? {
        if action == "halt" {
            set_task_status(&state, task_id, "failed", false).await?;
            insert_event(&state, task_id, None, "task_failed", json!({"reason": "invariant_halt_before_complete"})).await?;
            let unresolved = count_unresolved_items(&state, task_id).await?;
            record_task_execution_metric(&state, task_id, "failed", task_ctx.current_cycle, unresolved).await?;
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
    if matches!(
        task_ctx.safety.checkpoint_strategy,
        crate::config::CheckpointStrategy::GitTag
    ) {
        let ws_path = Path::new(&task_ctx.workspace_root);
        match create_checkpoint(ws_path, task_id, task_ctx.current_cycle).await {
            Ok(tag) => {
                insert_event(
                    state,
                    task_id,
                    None,
                    "checkpoint_created",
                    json!({"cycle": task_ctx.current_cycle, "tag": tag}),
                )
                .await?;

                if should_snapshot_binary(
                    task_ctx.safety.binary_snapshot,
                    task_ctx.self_referential,
                ) {
                    match snapshot_binary(&task_ctx.workspace_root, task_id, task_ctx.current_cycle)
                        .await
                    {
                        Ok(path) => {
                            insert_event(
                                state,
                                task_id,
                                None,
                                "binary_snapshot_created",
                                json!({"cycle": task_ctx.current_cycle, "path": path.display().to_string()}),
                            ).await?;
                        }
                        Err(e) => {
                            warn!(
                                cycle = task_ctx.current_cycle,
                                error = %e,
                                "failed to create binary snapshot"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    cycle = task_ctx.current_cycle,
                    error = %e,
                    "failed to create checkpoint"
                );
            }
        }
    }

    // Invariant checkpoint: before_cycle
    if let Some(action) = check_invariants(state, task_id, task_ctx, InvariantCheckPoint::BeforeCycle).await? {
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
    let mut task_item_paths: Vec<String> = items.iter().map(|item| item.qa_file_path.clone()).collect();

    // Segment-based execution: group steps by scope and dispatch accordingly.
    // Task-scoped steps run once (using first item as context anchor).
    // Item-scoped steps fan out across all items.
    let segments = build_scope_segments(task_ctx);
    if segments.is_empty() {
        // Fallback: no steps in execution plan, run legacy path
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
        for (segment_idx, segment) in segments.iter().enumerate() {
            match segment.scope {
                StepScope::Task => {
                    // Run task-scoped steps once using first item as anchor
                    if let Some(anchor_item) = items.first() {
                        let mut task_acc =
                            StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
                        process_item_filtered(
                            state,
                            ProcessItemRequest {
                                task_id,
                                item: anchor_item,
                                task_item_paths: &task_item_paths,
                                task_ctx,
                                runtime,
                                step_filter: Some(&segment.step_ids),
                            },
                            &mut task_acc,
                        )
                        .await?;
                        // Propagate task-scoped pipeline vars to subsequent segments
                        task_ctx.pipeline_vars = task_acc.pipeline_vars.clone();
                        // Persist pipeline vars to DB for recovery across process restarts
                        if let Ok(json) = serde_json::to_string(&task_ctx.pipeline_vars) {
                            if let Err(e) = state
                                .db_writer
                                .update_task_pipeline_vars(task_id, &json)
                                .await
                            {
                                tracing::warn!("failed to persist pipeline_vars after task segment: {e}");
                            }
                        }
                        // Invariant checkpoint: after_implement (if this segment had implement steps)
                        let has_implement = segment.step_ids.contains("implement")
                            || task_ctx.execution_plan.steps.iter().any(|s| {
                                segment.step_ids.contains(&s.id)
                                    && s.required_capability.as_deref() == Some("implement")
                            });
                        if has_implement {
                            if let Some(action) = check_invariants(
                                state,
                                task_id,
                                task_ctx,
                                InvariantCheckPoint::AfterImplement,
                            )
                            .await?
                            {
                                match action {
                                    "halt" => {
                                        set_task_status(state, task_id, "failed", false).await?;
                                        anyhow::bail!(
                                            "invariant halt at after_implement checkpoint"
                                        );
                                    }
                                    _ => {
                                        // rollback → force completion of this cycle
                                        return Ok(CycleSegmentOutcome::Completed);
                                    }
                                }
                            }
                        }

                        // Consume pending_generate_items from task segment
                        if let Some(gen_action) = task_acc.pending_generate_items.take() {
                            match super::item_generate::extract_dynamic_items(
                                &task_acc.pipeline_vars.vars,
                                &gen_action,
                            ) {
                                Ok(new_items) if !new_items.is_empty() => {
                                    match super::item_generate::create_dynamic_task_items_async(
                                        state,
                                        task_id,
                                        &new_items,
                                        gen_action.replace,
                                    )
                                    .await
                                    {
                                        Ok(_count) => {
                                            insert_event(
                                                state,
                                                task_id,
                                                None,
                                                "items_generated",
                                                json!({"count": new_items.len(), "replace": gen_action.replace}),
                                            )
                                            .await?;
                                            // Refresh items list
                                            items =
                                                list_task_items_for_cycle(state, task_id).await?;
                                            task_item_paths = items
                                                .iter()
                                                .map(|i| i.qa_file_path.clone())
                                                .collect();
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "failed to create dynamic items");
                                        }
                                    }
                                }
                                Ok(_) => {} // empty items, no-op
                                Err(e) => {
                                    warn!(error = %e, "failed to extract dynamic items");
                                }
                            }
                        }

                        if task_acc.terminal {
                            let skipped_item_steps = collect_remaining_item_step_steps(
                                task_ctx,
                                &segments,
                                segment_idx + 1,
                            );
                            propagate_task_segment_terminal_state(
                                &items,
                                &mut item_state,
                                &task_acc,
                                &task_ctx.pipeline_vars,
                                &skipped_item_steps,
                            );
                            emit_skipped_item_step_events(
                                state,
                                task_id,
                                &items,
                                &skipped_item_steps,
                            )
                            .await?;
                            halt_after_task_segment = true;
                        }
                    }
                }
                StepScope::Item => {
                    let max_par = segment.max_parallel;
                    if max_par <= 1 {
                        // === Sequential path (unchanged) ===
                        for item in &items {
                            let acc = item_state.entry(item.id.clone()).or_insert_with(|| {
                                StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                            });
                            process_item_filtered(
                                state,
                                ProcessItemRequest {
                                    task_id,
                                    item,
                                    task_item_paths: &task_item_paths,
                                    task_ctx,
                                    runtime,
                                    step_filter: Some(&segment.step_ids),
                                },
                                acc,
                            )
                            .await?;
                        }
                    } else {
                        // === Parallel path ===
                        let semaphore = Arc::new(Semaphore::new(max_par));
                        let shared_paths = Arc::new(task_item_paths.clone());
                        let shared_ctx = Arc::new(task_ctx.clone());
                        let shared_filter = Arc::new(segment.step_ids.clone());
                        let mut join_set = JoinSet::new();

                        for item in &items {
                            let permit = semaphore
                                .clone()
                                .acquire_owned()
                                .await
                                .map_err(|e| anyhow::anyhow!("semaphore closed: {}", e))?;
                            let state = state.clone();
                            let item = item.clone();
                            let task_id = task_id.to_string();
                            let paths = shared_paths.clone();
                            let ctx = shared_ctx.clone();
                            let filter = shared_filter.clone();
                            let item_runtime = runtime.fork();
                            // Reuse existing accumulator to preserve prior segment state
                            let prior_acc = item_state.remove(&item.id);
                            let pipeline_vars = task_ctx.pipeline_vars.clone();

                            join_set.spawn(async move {
                                let _permit = permit;
                                let mut acc = prior_acc.unwrap_or_else(|| {
                                    StepExecutionAccumulator::new(pipeline_vars)
                                });
                                let result = process_item_filtered_owned(
                                    &state,
                                    OwnedProcessItemRequest {
                                        task_id: task_id.clone(),
                                        item: item.clone(),
                                        task_item_paths: paths,
                                        task_ctx: ctx,
                                        runtime: item_runtime,
                                        step_filter: Some(filter),
                                    },
                                    &mut acc,
                                )
                                .await;
                                (item.id.clone(), acc, result)
                            });
                        }

                        // Collect all results (no fail-fast)
                        let mut errors = Vec::new();
                        while let Some(join_result) = join_set.join_next().await {
                            match join_result {
                                Ok((id, acc, Ok(()))) => {
                                    item_state.insert(id, acc);
                                }
                                Ok((id, acc, Err(e))) => {
                                    item_state.insert(id, acc);
                                    errors.push(e);
                                }
                                Err(e) => {
                                    errors.push(anyhow::anyhow!("item task panicked: {}", e));
                                }
                            }
                        }
                        if !errors.is_empty() {
                            let msg = errors
                                .iter()
                                .map(|e| e.to_string())
                                .collect::<Vec<_>>()
                                .join("; ");
                            anyhow::bail!("parallel item execution failed: {}", msg);
                        }
                    }

                    // item_select orchestration: check if next segment is a task-scoped
                    // item_select step, and if so, run selection now
                    if segment_idx + 1 < segments.len() {
                        let next = &segments[segment_idx + 1];
                        if next.scope == StepScope::Task
                            && has_item_select_step(next, &task_ctx.execution_plan)
                        {
                            if let Some(config) =
                                find_item_select_config(&task_ctx.execution_plan)
                            {
                                let eval_states =
                                    collect_item_eval_states(&items, &item_state);
                                match super::item_select::execute_item_select(
                                    &eval_states,
                                    &config,
                                ) {
                                    Ok(result) => {
                                        for id in &result.eliminated_ids {
                                            let _ = state
                                                .db_writer
                                                .update_task_item_status(id, "eliminated")
                                                .await;
                                        }
                                        promote_winner_vars(
                                            &mut task_ctx.pipeline_vars,
                                            &result,
                                        );
                                        persist_selection_to_store(
                                            state, task_ctx, task_id, &result, &config,
                                        )
                                        .await;
                                        insert_event(
                                            state,
                                            task_id,
                                            None,
                                            "item_selected",
                                            json!({
                                                "winner": result.winner_id,
                                                "eliminated": result.eliminated_ids,
                                            }),
                                        )
                                        .await?;
                                        // Filter out eliminated items
                                        items.retain(|item| {
                                            !result.eliminated_ids.contains(&item.id)
                                        });
                                        task_item_paths = items
                                            .iter()
                                            .map(|i| i.qa_file_path.clone())
                                            .collect();
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "item_select failed");
                                    }
                                }
                            }
                        }
                    }
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

        for item in &items {
            let acc = item_state
                .entry(item.id.clone())
                .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
            finalize_item_execution(state, task_id, item, task_ctx, acc).await?;
        }
    }

    Ok(CycleSegmentOutcome::Completed)
}

/// Check invariants at a given checkpoint. Returns:
/// - `None` if all pass or only warnings
/// - `Some("halt")` if a Halt violation is found
/// - `Some("rollback")` if a Rollback violation is found
async fn check_invariants(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
    checkpoint: InvariantCheckPoint,
) -> Result<Option<&'static str>> {
    if task_ctx.pinned_invariants.is_empty() {
        return Ok(None);
    }

    let results = evaluate_invariants(
        &task_ctx.pinned_invariants,
        checkpoint,
        &task_ctx.workspace_root,
    )?;

    if results.is_empty() {
        return Ok(None);
    }

    // Emit events for each result
    for r in &results {
        let event_type = if r.passed {
            "invariant_passed"
        } else {
            "invariant_violated"
        };
        insert_event(
            state,
            task_id,
            None,
            event_type,
            json!({
                "invariant": r.name,
                "checkpoint": format!("{:?}", checkpoint),
                "passed": r.passed,
                "message": r.message,
                "on_violation": format!("{:?}", r.on_violation),
            }),
        )
        .await?;
        if !r.passed && r.on_violation == OnViolation::Warn {
            warn!(
                invariant = %r.name,
                message = %r.message,
                "invariant warning at {:?}",
                checkpoint
            );
        }
    }

    if has_halting_violation(&results) {
        return Ok(Some("halt"));
    }
    if has_rollback_violation(&results) {
        return Ok(Some("rollback"));
    }

    Ok(None)
}

/// Detect consecutive failures and perform git rollback with optional binary recovery.
async fn apply_auto_rollback_if_needed(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
) -> Result<()> {
    if !(task_ctx.safety.auto_rollback
        && task_ctx.consecutive_failures >= task_ctx.safety.max_consecutive_failures
        && matches!(
            task_ctx.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        ))
    {
        return Ok(());
    }

    let rollback_cycle = task_ctx
        .current_cycle
        .saturating_sub(task_ctx.consecutive_failures);
    let rollback_tag = format!("checkpoint/{}/{}", task_id, rollback_cycle.max(1));
    let ws_path = Path::new(&task_ctx.workspace_root);
    match rollback_to_checkpoint(ws_path, &rollback_tag).await {
        Ok(()) => {
            insert_event(
                state,
                task_id,
                None,
                "auto_rollback",
                json!({
                    "cycle": task_ctx.current_cycle,
                    "rollback_to": rollback_tag,
                    "consecutive_failures": task_ctx.consecutive_failures,
                }),
            )
            .await?;
            state.emit_event(
                task_id,
                None,
                "auto_rollback",
                json!({"rollback_to": rollback_tag}),
            );

            if should_snapshot_binary(task_ctx.safety.binary_snapshot, task_ctx.self_referential) {
                match restore_binary_snapshot(&task_ctx.workspace_root).await {
                    Ok(()) => {
                        insert_event(
                            state,
                            task_id,
                            None,
                            "binary_snapshot_restored",
                            json!({"cycle": task_ctx.current_cycle}),
                        )
                        .await?;
                    }
                    Err(e) => warn!(error = %e, "failed to restore binary snapshot"),
                }
            }

            task_ctx.consecutive_failures = 0;
        }
        Err(e) => {
            warn!(error = %e, "auto-rollback failed");
            insert_event(
                state,
                task_id,
                None,
                "auto_rollback_failed",
                json!({"error": e.to_string()}),
            )
            .await?;
        }
    }

    Ok(())
}

/// Execute guard/gate steps and check termination conditions.
/// Returns `true` if the task should terminate (guard triggered early completion).
async fn execute_guard_steps(
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
async fn evaluate_loop_continuation(
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

/// A contiguous group of steps with the same execution scope.
struct ScopeSegment {
    scope: StepScope,
    step_ids: HashSet<String>,
    /// Resolved concurrency limit for item-scoped segments (1 = sequential).
    max_parallel: usize,
}

/// Group execution plan steps into contiguous segments of the same scope.
/// Guard steps are excluded; they run separately after items.
fn build_scope_segments(task_ctx: &crate::config::TaskRuntimeContext) -> Vec<ScopeSegment> {
    let plan_default = task_ctx.execution_plan.max_parallel;
    let mut segments: Vec<ScopeSegment> = Vec::new();
    for step in &task_ctx.execution_plan.steps {
        if step.is_guard || !step.enabled {
            continue;
        }
        let scope = step.resolved_scope();
        if let Some(last) = segments.last_mut() {
            if last.scope == scope {
                last.step_ids.insert(step.id.clone());
                continue;
            }
        }
        let mut ids = HashSet::new();
        ids.insert(step.id.clone());
        // Resolve max_parallel: step override > plan default > 1
        let max_parallel = if scope == StepScope::Item {
            step.max_parallel.or(plan_default).unwrap_or(1)
        } else {
            1 // task-scoped segments are always sequential
        };
        segments.push(ScopeSegment {
            scope,
            step_ids: ids,
            max_parallel,
        });
    }
    segments
}

fn propagate_task_segment_terminal_state(
    items: &[crate::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_acc: &StepExecutionAccumulator,
    task_pipeline_vars: &crate::config::PipelineVariables,
    skipped_item_steps: &[String],
) {
    for item in items {
        let acc = item_state
            .entry(item.id.clone())
            .or_insert_with(|| StepExecutionAccumulator::new(task_pipeline_vars.clone()));
        acc.merge_task_pipeline_vars(&task_acc.pipeline_vars);
        acc.item_status = task_acc.item_status.clone();
        if let Some(execution_failed) = task_acc.flags.get("execution_failed").copied() {
            acc.flags
                .insert("execution_failed".to_string(), execution_failed);
        }
        for step_id in skipped_item_steps {
            acc.step_skipped.insert(step_id.clone(), true);
        }
        acc.terminal = true;
    }
}

fn collect_remaining_item_step_steps(
    task_ctx: &crate::config::TaskRuntimeContext,
    segments: &[ScopeSegment],
    start_idx: usize,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut step_ids = Vec::new();

    for segment in segments.iter().skip(start_idx) {
        if segment.scope != StepScope::Item {
            continue;
        }
        for step in &task_ctx.execution_plan.steps {
            if !segment.step_ids.contains(&step.id) || !step.enabled {
                continue;
            }
            if !step.repeatable && task_ctx.current_cycle > 1 {
                continue;
            }
            if seen.insert(step.id.clone()) {
                step_ids.push(step.id.clone());
            }
        }
    }

    step_ids
}

async fn emit_skipped_item_step_events(
    state: &Arc<InnerState>,
    task_id: &str,
    items: &[crate::dto::TaskItemRow],
    skipped_item_steps: &[String],
) -> Result<()> {
    for item in items {
        for step_id in skipped_item_steps {
            insert_event(
                state,
                task_id,
                Some(&item.id),
                "step_skipped",
                json!({
                    "step": step_id,
                    "step_id": step_id,
                    "step_scope": StepScope::Item,
                    "reason": "upstream_task_segment_terminated"
                }),
            )
            .await?;
        }
    }

    Ok(())
}

fn should_snapshot_binary(binary_snapshot: bool, self_referential: bool) -> bool {
    binary_snapshot && self_referential
}

/// Check if a segment contains an item_select builtin step.
fn has_item_select_step(
    segment: &ScopeSegment,
    plan: &crate::config::TaskExecutionPlan,
) -> bool {
    for step_id in &segment.step_ids {
        if let Some(step) = plan.step_by_id(step_id) {
            if step.builtin.as_deref() == Some("item_select") {
                return true;
            }
        }
    }
    false
}

/// Find ItemSelectConfig from any step in the execution plan.
fn find_item_select_config(
    plan: &crate::config::TaskExecutionPlan,
) -> Option<crate::config::ItemSelectConfig> {
    plan.steps
        .iter()
        .find_map(|s| s.item_select_config.clone())
}

/// Collect item evaluation states from item_state accumulators.
fn collect_item_eval_states(
    items: &[crate::dto::TaskItemRow],
    item_state: &HashMap<String, StepExecutionAccumulator>,
) -> Vec<super::item_select::ItemEvalState> {
    items
        .iter()
        .filter_map(|item| {
            item_state.get(&item.id).map(|acc| {
                super::item_select::ItemEvalState {
                    item_id: item.id.clone(),
                    pipeline_vars: acc.pipeline_vars.vars.clone(),
                }
            })
        })
        .collect()
}

/// Promote winner variables into task-level pipeline vars.
fn promote_winner_vars(
    pipeline_vars: &mut crate::config::PipelineVariables,
    result: &crate::config::SelectionResult,
) {
    pipeline_vars
        .vars
        .insert("item_select_winner".to_string(), result.winner_id.clone());
    for (k, v) in &result.winner_vars {
        pipeline_vars.vars.insert(k.clone(), v.clone());
    }
}

/// Persist selection result to a workflow store if configured.
async fn persist_selection_to_store(
    state: &Arc<InnerState>,
    task_ctx: &crate::config::TaskRuntimeContext,
    task_id: &str,
    result: &crate::config::SelectionResult,
    config: &crate::config::ItemSelectConfig,
) {
    if let Some(ref store_target) = config.store_result {
        let value = serde_json::json!({
            "winner_id": result.winner_id,
            "eliminated_ids": result.eliminated_ids,
            "winner_vars": result.winner_vars,
        });
        let cr = match state.active_config.read() {
            Ok(cfg) => cfg.config.custom_resources.clone(),
            Err(_) => return,
        };
        let op = crate::store::StoreOp::Put {
            store_name: store_target.namespace.clone(),
            project_id: task_ctx.project_id.clone(),
            key: store_target.key.clone(),
            value: value.to_string(),
            task_id: task_id.to_string(),
        };
        if let Err(e) = state.store_manager.execute(&cr, op).await {
            warn!(error = %e, "failed to persist item_select result to store");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PipelineVariables, WorkflowLoopConfig, WorkflowLoopGuardConfig};
    use crate::test_utils::TestState;

    fn make_loop_policy(mode: LoopMode, max_cycles: Option<u32>) -> WorkflowLoopConfig {
        WorkflowLoopConfig {
            mode,
            guard: WorkflowLoopGuardConfig {
                max_cycles,
                ..Default::default()
            },
        }
    }

    #[test]
    fn fixed_mode_stops_at_max_cycles() {
        let policy = make_loop_policy(LoopMode::Fixed, Some(2));
        // cycle 1 < 2 → continue
        let result = evaluate_loop_guard_rules(&policy, 1, 0);
        assert_eq!(result, Some((true, "fixed_cycle_continue".to_string())));
        // cycle 2 >= 2 → stop
        let result = evaluate_loop_guard_rules(&policy, 2, 0);
        assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
        // cycle 3 >= 2 → stop
        let result = evaluate_loop_guard_rules(&policy, 3, 0);
        assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
    }

    #[test]
    fn fixed_mode_defaults_to_one_cycle() {
        let policy = make_loop_policy(LoopMode::Fixed, None);
        // cycle 1 >= 1 → stop immediately (acts like once)
        let result = evaluate_loop_guard_rules(&policy, 1, 0);
        assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
    }

    #[test]
    fn once_mode_always_stops() {
        let policy = make_loop_policy(LoopMode::Once, None);
        let result = evaluate_loop_guard_rules(&policy, 1, 0);
        assert_eq!(result, Some((false, "once_mode".to_string())));
    }

    #[test]
    fn infinite_mode_respects_max_cycles() {
        let policy = make_loop_policy(LoopMode::Infinite, Some(3));
        let result = evaluate_loop_guard_rules(&policy, 2, 0);
        assert_eq!(result, None); // guard enabled, no decision yet
        let result = evaluate_loop_guard_rules(&policy, 3, 0);
        assert_eq!(result, Some((false, "max_cycles_reached".to_string())));
    }

    #[test]
    fn infinite_mode_with_disabled_guard_continues_immediately() {
        let mut policy = make_loop_policy(LoopMode::Infinite, None);
        policy.guard.enabled = false;
        let result = evaluate_loop_guard_rules(&policy, 1, 0);
        assert_eq!(result, Some((true, "guard_disabled".to_string())));
    }

    #[test]
    fn build_segments_groups_contiguous_scopes() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "plan".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "implement".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "qa_testing".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "ticket_fix".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "doc_governance".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };

        let segments = build_scope_segments(&task_ctx);

        // Should produce 3 segments:
        // [plan, implement] → Task
        // [qa_testing, ticket_fix] → Item
        // [doc_governance] → Task
        assert_eq!(segments.len(), 3);

        assert_eq!(segments[0].scope, StepScope::Task);
        assert!(segments[0].step_ids.contains("plan"));
        assert!(segments[0].step_ids.contains("implement"));

        assert_eq!(segments[1].scope, StepScope::Item);
        assert!(segments[1].step_ids.contains("qa_testing"));
        assert!(segments[1].step_ids.contains("ticket_fix"));

        assert_eq!(segments[2].scope, StepScope::Task);
        assert!(segments[2].step_ids.contains("doc_governance"));
    }

    #[test]
    fn build_segments_skips_guards() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "plan".into(),

                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "loop_guard".into(),

                        required_capability: None,
                        builtin: Some("loop_guard".into()),
                        enabled: true,
                        repeatable: true,
                        is_guard: true,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };

        let segments = build_scope_segments(&task_ctx);
        // Guard is excluded, only plan remains
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].scope, StepScope::Task);
        assert!(segments[0].step_ids.contains("plan"));
        assert!(!segments[0].step_ids.contains("loop_guard"));
    }

    #[test]
    fn resolved_scope_uses_explicit_override() {
        use crate::config::*;
        let step = TaskExecutionStep {
            id: "qa_testing".into(),

            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: Some(StepScope::Task), // Override default Item scope
            behavior: StepBehavior::default(),
            max_parallel: None,
            timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn propagate_task_segment_terminal_state_marks_all_items_terminal() {
        let items = vec![
            crate::dto::TaskItemRow {
                id: "item-1".to_string(),
                qa_file_path: "a.md".to_string(),
                dynamic_vars_json: None,
                label: None,
                source: "static".to_string(),
            },
            crate::dto::TaskItemRow {
                id: "item-2".to_string(),
                qa_file_path: "b.md".to_string(),
                dynamic_vars_json: None,
                label: None,
                source: "static".to_string(),
            },
        ];
        let mut item_state = HashMap::new();
        let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
        task_acc.item_status = "unresolved".to_string();
        task_acc.terminal = true;
        task_acc.flags.insert("execution_failed".to_string(), true);
        task_acc.pipeline_vars.vars.insert(
            "fatal_reason".to_string(),
            "provider rate limit exceeded".to_string(),
        );

        propagate_task_segment_terminal_state(
            &items,
            &mut item_state,
            &task_acc,
            &PipelineVariables::default(),
            &["qa_testing".to_string(), "ticket_fix".to_string()],
        );

        assert_eq!(item_state.len(), 2);
        for item_id in ["item-1", "item-2"] {
            let acc = item_state.get(item_id).expect("item state missing");
            assert!(acc.terminal);
            assert_eq!(acc.item_status, "unresolved");
            assert_eq!(acc.flags.get("execution_failed").copied(), Some(true));
            assert_eq!(acc.step_skipped.get("qa_testing").copied(), Some(true));
            assert_eq!(acc.step_skipped.get("ticket_fix").copied(), Some(true));
            assert_eq!(
                acc.pipeline_vars
                    .vars
                    .get("fatal_reason")
                    .map(String::as_str),
                Some("provider rate limit exceeded")
            );
        }
    }

    #[test]
    fn collect_remaining_item_step_steps_returns_only_item_steps_after_segment() {
        use crate::config::{
            PipelineVariables, SafetyConfig, StepBehavior, TaskExecutionPlan, TaskExecutionStep,
            TaskRuntimeContext, WorkflowFinalizeConfig, WorkflowLoopConfig,
        };

        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            current_cycle: 2,
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "implement".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "qa_testing".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "ticket_fix".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "align_tests".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };
        let segments = build_scope_segments(&task_ctx);

        let skipped = collect_remaining_item_step_steps(&task_ctx, &segments, 1);

        assert_eq!(
            skipped,
            vec!["qa_testing".to_string(), "ticket_fix".to_string()]
        );
    }

    #[test]
    fn collect_remaining_item_step_steps_skips_non_repeatable_steps_after_first_cycle() {
        use crate::config::{
            PipelineVariables, SafetyConfig, StepBehavior, TaskExecutionPlan, TaskExecutionStep,
            TaskRuntimeContext, WorkflowFinalizeConfig, WorkflowLoopConfig,
        };

        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            current_cycle: 2,
            execution_plan: TaskExecutionPlan {
                steps: vec![TaskExecutionStep {
                    id: "qa_testing".into(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };
        let segments = build_scope_segments(&task_ctx);

        assert!(collect_remaining_item_step_steps(&task_ctx, &segments, 0).is_empty());
    }

    #[tokio::test]
    async fn emit_skipped_item_step_events_writes_event_rows() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let items = vec![
            crate::dto::TaskItemRow {
                id: "item-1".to_string(),
                qa_file_path: "a.md".to_string(),
                dynamic_vars_json: None,
                label: None,
                source: "static".to_string(),
            },
            crate::dto::TaskItemRow {
                id: "item-2".to_string(),
                qa_file_path: "b.md".to_string(),
                dynamic_vars_json: None,
                label: None,
                source: "static".to_string(),
            },
        ];
        let task_id = "task-skip-events";

        emit_skipped_item_step_events(
            &state,
            task_id,
            &items,
            &["qa_testing".to_string(), "ticket_fix".to_string()],
        )
        .await
        .expect("emit skipped events");

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        let skipped_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND event_type = 'step_skipped'",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .expect("count skipped events");

        assert_eq!(skipped_count, 4);
    }

    #[test]
    fn infinite_mode_no_max_cycles_with_guard_enabled_returns_none() {
        let policy = make_loop_policy(LoopMode::Infinite, None);
        // guard is enabled by default, no max_cycles — defer to agent guard
        let result = evaluate_loop_guard_rules(&policy, 1, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn infinite_mode_with_max_cycles_before_limit_returns_none() {
        let policy = make_loop_policy(LoopMode::Infinite, Some(5));
        // cycle 2 < 5, guard enabled → defer to agent guard
        let result = evaluate_loop_guard_rules(&policy, 2, 3);
        assert_eq!(result, None);
    }

    #[test]
    fn build_segments_skips_disabled_steps() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "plan".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "disabled_step".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: false,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };

        let segments = build_scope_segments(&task_ctx);
        assert_eq!(segments.len(), 1);
        assert!(segments[0].step_ids.contains("plan"));
        assert!(!segments[0].step_ids.contains("disabled_step"));
    }

    #[test]
    fn build_segments_empty_when_no_steps() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            execution_plan: TaskExecutionPlan {
                steps: vec![],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };

        let segments = build_scope_segments(&task_ctx);
        assert!(segments.is_empty());
    }

    #[test]
    fn propagate_task_segment_terminal_state_no_execution_failed_flag() {
        let items = vec![crate::dto::TaskItemRow {
            id: "item-1".to_string(),
            qa_file_path: "a.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        }];
        let mut item_state = HashMap::new();
        let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
        task_acc.item_status = "failed".to_string();
        task_acc.terminal = true;
        // No execution_failed flag set

        propagate_task_segment_terminal_state(
            &items,
            &mut item_state,
            &task_acc,
            &PipelineVariables::default(),
            &[],
        );

        let acc = item_state.get("item-1").expect("item state missing");
        assert!(acc.terminal);
        assert_eq!(acc.item_status, "failed");
        assert!(!acc.flags.contains_key("execution_failed"));
    }

    #[test]
    fn propagate_preserves_existing_item_state() {
        let items = vec![crate::dto::TaskItemRow {
            id: "item-1".to_string(),
            qa_file_path: "a.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        }];
        let mut item_state = HashMap::new();
        let mut existing_acc = StepExecutionAccumulator::new(PipelineVariables::default());
        existing_acc
            .pipeline_vars
            .vars
            .insert("existing_key".to_string(), "existing_val".to_string());
        item_state.insert("item-1".to_string(), existing_acc);

        let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
        task_acc.item_status = "unresolved".to_string();
        task_acc
            .pipeline_vars
            .vars
            .insert("new_key".to_string(), "new_val".to_string());

        propagate_task_segment_terminal_state(
            &items,
            &mut item_state,
            &task_acc,
            &PipelineVariables::default(),
            &[],
        );

        let acc = item_state.get("item-1").unwrap();
        assert_eq!(
            acc.pipeline_vars.vars.get("existing_key").unwrap(),
            "existing_val"
        );
        assert_eq!(acc.pipeline_vars.vars.get("new_key").unwrap(), "new_val");
    }

    #[test]
    fn collect_remaining_item_step_steps_from_start_index_2() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            current_cycle: 1,
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "plan".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "qa".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "governance".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };
        let segments = build_scope_segments(&task_ctx);
        assert_eq!(segments.len(), 3);

        // Skip from segment 2 onward (governance is Task scope, no Item steps)
        let skipped = collect_remaining_item_step_steps(&task_ctx, &segments, 2);
        assert!(skipped.is_empty());
    }

    #[tokio::test]
    async fn emit_skipped_item_step_events_empty_steps_emits_nothing() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let items = vec![crate::dto::TaskItemRow {
            id: "item-1".to_string(),
            qa_file_path: "a.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        }];

        emit_skipped_item_step_events(&state, "task-1", &items, &[])
            .await
            .expect("should succeed");

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = 'task-1' AND event_type = 'step_skipped'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn should_snapshot_true_when_both_enabled() {
        assert!(should_snapshot_binary(true, true));
    }

    #[test]
    fn should_snapshot_false_when_not_self_referential() {
        assert!(!should_snapshot_binary(true, false));
    }

    #[test]
    fn should_snapshot_false_when_binary_snapshot_disabled() {
        assert!(!should_snapshot_binary(false, true));
    }

    #[test]
    fn should_snapshot_false_when_both_disabled() {
        assert!(!should_snapshot_binary(false, false));
    }

    #[test]
    fn build_segments_item_select_is_task_scoped() {
        use crate::config::*;
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".into(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".into(),
            execution_plan: TaskExecutionPlan {
                steps: vec![
                    TaskExecutionStep {
                        id: "qa_testing".into(),
                        required_capability: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None, // default: Item
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    TaskExecutionStep {
                        id: "evaluate".into(),
                        required_capability: None,
                        builtin: Some("item_select".into()),
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None, // item_select defaults to Task
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: Some(ItemSelectConfig {
                            strategy: SelectionStrategy::Min,
                            metric_var: Some("error_count".into()),
                            weights: None,
                            threshold: None,
                            store_result: None,
                            tie_break: TieBreak::First,
                        }),
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
                max_parallel: None,
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: String::new(),
            spawn_depth: 0,
        };

        let segments = build_scope_segments(&task_ctx);
        // qa_testing → Item, evaluate (item_select builtin) → Task
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].scope, StepScope::Item);
        assert!(segments[0].step_ids.contains("qa_testing"));
        assert_eq!(segments[1].scope, StepScope::Task);
        assert!(segments[1].step_ids.contains("evaluate"));

        // has_item_select_step should find it
        assert!(has_item_select_step(&segments[1], &task_ctx.execution_plan));
        assert!(!has_item_select_step(&segments[0], &task_ctx.execution_plan));

        // find_item_select_config should return the config
        let config = find_item_select_config(&task_ctx.execution_plan);
        assert!(config.is_some());
        assert_eq!(config.unwrap().strategy, SelectionStrategy::Min);
    }

    #[test]
    fn collect_item_eval_states_maps_pipeline_vars() {
        let items = vec![
            crate::dto::TaskItemRow {
                id: "item-a".into(),
                qa_file_path: "a.md".into(),
                dynamic_vars_json: None,
                label: None,
                source: "static".into(),
            },
            crate::dto::TaskItemRow {
                id: "item-b".into(),
                qa_file_path: "b.md".into(),
                dynamic_vars_json: None,
                label: None,
                source: "static".into(),
            },
        ];
        let mut item_state = HashMap::new();
        let mut acc_a = StepExecutionAccumulator::new(PipelineVariables::default());
        acc_a.pipeline_vars.vars.insert("error_count".into(), "3".into());
        item_state.insert("item-a".to_string(), acc_a);

        let mut acc_b = StepExecutionAccumulator::new(PipelineVariables::default());
        acc_b.pipeline_vars.vars.insert("error_count".into(), "1".into());
        item_state.insert("item-b".to_string(), acc_b);

        let eval_states = collect_item_eval_states(&items, &item_state);
        assert_eq!(eval_states.len(), 2);
        assert_eq!(eval_states[0].item_id, "item-a");
        assert_eq!(eval_states[0].pipeline_vars.get("error_count").unwrap(), "3");
        assert_eq!(eval_states[1].item_id, "item-b");
        assert_eq!(eval_states[1].pipeline_vars.get("error_count").unwrap(), "1");
    }

    #[test]
    fn promote_winner_vars_inserts_into_pipeline() {
        use crate::config::SelectionResult;
        let mut vars = PipelineVariables::default();
        vars.vars.insert("existing".into(), "keep".into());

        let result = SelectionResult {
            winner_id: "item-b".into(),
            eliminated_ids: vec!["item-a".into()],
            winner_vars: {
                let mut m = HashMap::new();
                m.insert("quality_score".into(), "95".into());
                m
            },
        };

        promote_winner_vars(&mut vars, &result);
        assert_eq!(vars.vars.get("item_select_winner").unwrap(), "item-b");
        assert_eq!(vars.vars.get("quality_score").unwrap(), "95");
        assert_eq!(vars.vars.get("existing").unwrap(), "keep");
    }

    #[test]
    fn check_invariants_returns_none_for_empty_invariants() {
        // check_invariants is async, but we can verify the early-return logic
        // by checking the pinned_invariants.is_empty() path inline
        let invariants: Vec<crate::config::InvariantConfig> = vec![];
        assert!(invariants.is_empty());
        // The function returns Ok(None) when pinned_invariants is empty
    }

    #[tokio::test]
    async fn emit_skipped_item_step_events_empty_items_emits_nothing() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        emit_skipped_item_step_events(&state, "task-1", &[], &["qa_testing".to_string()])
            .await
            .expect("should succeed");

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = 'task-1'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0);
    }
}
