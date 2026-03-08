use crate::config::StepScope;
use crate::events::insert_event;
use crate::scheduler::item_executor::{
    finalize_item_execution, process_item_filtered, process_item_filtered_owned,
    OwnedProcessItemRequest, ProcessItemRequest, StepExecutionAccumulator,
};
use crate::scheduler::task_state::{list_task_items_for_cycle, set_task_status};
use crate::scheduler::RunningTask;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

/// Signal returned by task-scoped segment execution.
pub(super) enum TaskSegmentOutcome {
    /// Continue to next segment.
    Continue,
    /// A terminal condition in the task segment requires halting after this segment.
    HaltAfterSegment,
    /// An invariant check triggered a rollback (complete cycle early).
    InvariantRollback,
}

/// A contiguous group of steps with the same execution scope.
pub(super) struct ScopeSegment {
    pub scope: StepScope,
    pub step_ids: HashSet<String>,
    /// Resolved concurrency limit for item-scoped segments (1 = sequential).
    pub max_parallel: usize,
}

/// Group execution plan steps into contiguous segments of the same scope.
/// Guard steps are excluded; they run separately after items.
pub(super) fn build_scope_segments(
    task_ctx: &crate::config::TaskRuntimeContext,
) -> Vec<ScopeSegment> {
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

/// Execute a task-scoped segment: run steps on the anchor item, propagate vars,
/// handle dynamic item generation, invariant checks, and terminal state.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_task_segment(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
    runtime: &RunningTask,
    segment: &ScopeSegment,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &mut Vec<crate::dto::TaskItemRow>,
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<TaskSegmentOutcome> {
    let anchor_item = match items.first() {
        Some(item) => item,
        None => return Ok(TaskSegmentOutcome::Continue),
    };

    let mut task_acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
    process_item_filtered(
        state,
        ProcessItemRequest {
            task_id,
            item: anchor_item,
            task_item_paths,
            task_ctx,
            runtime,
            step_filter: Some(&segment.step_ids),
            run_dynamic_steps: false,
        },
        &mut task_acc,
    )
    .await?;

    // Propagate task-scoped pipeline vars to subsequent segments
    task_ctx.pipeline_vars = task_acc.pipeline_vars.clone();
    // Persist pipeline vars to DB for recovery across process restarts
    if let Ok(json_str) = serde_json::to_string(&task_ctx.pipeline_vars) {
        if let Err(e) = state
            .db_writer
            .update_task_pipeline_vars(task_id, &json_str)
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
        if let Some(action) = super::cycle_safety::check_invariants(
            state,
            task_id,
            task_ctx,
            crate::config::InvariantCheckPoint::AfterImplement,
        )
        .await?
        {
            match action {
                "halt" => {
                    set_task_status(state, task_id, "failed", false).await?;
                    anyhow::bail!("invariant halt at after_implement checkpoint");
                }
                _ => {
                    return Ok(TaskSegmentOutcome::InvariantRollback);
                }
            }
        }
    }

    // Consume pending_generate_items from task segment
    tracing::info!(
        segment_idx,
        has_pending = task_acc.pending_generate_items.is_some(),
        "checking pending_generate_items after task segment"
    );
    if let Some(gen_action) = task_acc.pending_generate_items.take() {
        match super::super::item_generate::extract_dynamic_items(
            &task_acc.pipeline_vars.vars,
            &gen_action,
        ) {
            Ok(new_items) if !new_items.is_empty() => {
                match super::super::item_generate::create_dynamic_task_items_async(
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
                        // Refresh items list — when dynamic items exist,
                        // subsequent item-scoped steps target only dynamic items
                        let all_items = list_task_items_for_cycle(state, task_id).await?;
                        let has_dynamic = all_items.iter().any(|i| i.source == "dynamic");
                        *items = if has_dynamic {
                            all_items
                                .into_iter()
                                .filter(|i| i.source == "dynamic")
                                .collect()
                        } else {
                            all_items
                        };
                        *task_item_paths = items.iter().map(|i| i.qa_file_path.clone()).collect();
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
        let skipped_item_steps =
            collect_remaining_item_step_steps(task_ctx, segments, segment_idx + 1);
        propagate_task_segment_terminal_state(
            items,
            item_state,
            &task_acc,
            &task_ctx.pipeline_vars,
            &skipped_item_steps,
        );
        emit_skipped_item_step_events(state, task_id, items, &skipped_item_steps).await?;
        return Ok(TaskSegmentOutcome::HaltAfterSegment);
    }

    Ok(TaskSegmentOutcome::Continue)
}

/// Execute an item-scoped segment: fan out across all items sequentially or in parallel.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_item_segment(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
    runtime: &RunningTask,
    segment: &ScopeSegment,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &[crate::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &[String],
) -> Result<()> {
    let max_par = segment.max_parallel;
    let run_dynamic_steps = is_last_item_segment(segment_idx, segments);
    if max_par <= 1 {
        // === Sequential path ===
        for item in items {
            let acc = item_state
                .entry(item.id.clone())
                .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
            process_item_filtered(
                state,
                ProcessItemRequest {
                    task_id,
                    item,
                    task_item_paths,
                    task_ctx,
                    runtime,
                    step_filter: Some(&segment.step_ids),
                    run_dynamic_steps,
                },
                acc,
            )
            .await?;
        }
    } else {
        // === Parallel path ===
        let semaphore = Arc::new(Semaphore::new(max_par));
        let shared_paths = Arc::new(task_item_paths.to_vec());
        let shared_ctx = Arc::new(task_ctx.clone());
        let shared_filter = Arc::new(segment.step_ids.clone());
        let mut join_set = JoinSet::new();

        for item in items {
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
                let mut acc =
                    prior_acc.unwrap_or_else(|| StepExecutionAccumulator::new(pipeline_vars));
                let result = process_item_filtered_owned(
                    &state,
                    OwnedProcessItemRequest {
                        task_id: task_id.clone(),
                        item: item.clone(),
                        task_item_paths: paths,
                        task_ctx: ctx,
                        runtime: item_runtime,
                        step_filter: Some(filter),
                        run_dynamic_steps,
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

    Ok(())
}

pub(super) fn is_last_item_segment(segment_idx: usize, segments: &[ScopeSegment]) -> bool {
    !segments
        .iter()
        .skip(segment_idx + 1)
        .any(|segment| segment.scope == StepScope::Item)
}

/// Check if next segment is a task-scoped item_select step, and if so, run selection.
#[allow(clippy::too_many_arguments)]
pub(super) async fn try_item_selection(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut crate::config::TaskRuntimeContext,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &mut Vec<crate::dto::TaskItemRow>,
    item_state: &HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<()> {
    if segment_idx + 1 >= segments.len() {
        return Ok(());
    }
    let next = &segments[segment_idx + 1];
    if next.scope != StepScope::Task || !has_item_select_step(next, &task_ctx.execution_plan) {
        return Ok(());
    }
    let config = match find_item_select_config(&task_ctx.execution_plan) {
        Some(c) => c,
        None => return Ok(()),
    };

    let eval_states = collect_item_eval_states(items, item_state);
    match super::super::item_select::execute_item_select(&eval_states, &config) {
        Ok(result) => {
            for id in &result.eliminated_ids {
                let _ = state
                    .db_writer
                    .update_task_item_status(id, "eliminated")
                    .await;
            }
            promote_winner_vars(&mut task_ctx.pipeline_vars, &result);
            persist_selection_to_store(state, task_ctx, task_id, &result, &config).await;
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
            items.retain(|item| !result.eliminated_ids.contains(&item.id));
            *task_item_paths = items.iter().map(|i| i.qa_file_path.clone()).collect();
        }
        Err(e) => {
            warn!(error = %e, "item_select failed");
        }
    }

    Ok(())
}

/// Finalize all items at the end of segment-based execution.
pub(super) async fn finalize_items(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &crate::config::TaskRuntimeContext,
    items: &[crate::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
) -> Result<()> {
    for item in items {
        let acc = item_state
            .entry(item.id.clone())
            .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
        finalize_item_execution(state, task_id, item, task_ctx, acc).await?;
    }
    Ok(())
}

pub(super) fn propagate_task_segment_terminal_state(
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

pub(super) fn collect_remaining_item_step_steps(
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

pub(super) async fn emit_skipped_item_step_events(
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

/// Check if a segment contains an item_select builtin step.
fn has_item_select_step(segment: &ScopeSegment, plan: &crate::config::TaskExecutionPlan) -> bool {
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
#[cfg(test)]
pub(super) fn find_item_select_config_for_test(
    plan: &crate::config::TaskExecutionPlan,
) -> Option<crate::config::ItemSelectConfig> {
    find_item_select_config(plan)
}

fn find_item_select_config(
    plan: &crate::config::TaskExecutionPlan,
) -> Option<crate::config::ItemSelectConfig> {
    plan.steps.iter().find_map(|s| s.item_select_config.clone())
}

/// Collect item evaluation states from item_state accumulators.
pub(super) fn collect_item_eval_states(
    items: &[crate::dto::TaskItemRow],
    item_state: &HashMap<String, StepExecutionAccumulator>,
) -> Vec<super::super::item_select::ItemEvalState> {
    items
        .iter()
        .filter_map(|item| {
            item_state
                .get(&item.id)
                .map(|acc| super::super::item_select::ItemEvalState {
                    item_id: item.id.clone(),
                    pipeline_vars: acc.pipeline_vars.vars.clone(),
                })
        })
        .collect()
}

/// Promote winner variables into task-level pipeline vars.
pub(super) fn promote_winner_vars(
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
