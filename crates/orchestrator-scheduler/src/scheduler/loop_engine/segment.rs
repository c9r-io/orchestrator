use crate::scheduler::RunningTask;
use crate::scheduler::item_executor::{
    OwnedProcessItemRequest, ProcessItemRequest, StepExecutionAccumulator, finalize_item_execution,
    process_item_filtered, process_item_filtered_owned,
};
use crate::scheduler::task_state::{list_task_items_for_cycle, set_item_blocked, set_task_status};
use agent_orchestrator::config::StepScope;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use super::isolation;

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
    /// Delay in ms between successive parallel agent spawns (0 = no delay).
    pub stagger_delay_ms: u64,
}

/// Group execution plan steps into contiguous segments of the same scope.
/// Guard steps are excluded; they run separately after items.
pub(super) fn build_scope_segments(
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
) -> Vec<ScopeSegment> {
    let plan_default = task_ctx.execution_plan.max_parallel;
    let plan_stagger = task_ctx.execution_plan.stagger_delay_ms;
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
        // FR-055: Resolve stagger_delay_ms: step override > plan default > 0
        let stagger_delay_ms = if scope == StepScope::Item {
            step.stagger_delay_ms.or(plan_stagger).unwrap_or(0)
        } else {
            0
        };
        segments.push(ScopeSegment {
            scope,
            step_ids: ids,
            max_parallel,
            stagger_delay_ms,
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
    task_ctx: &mut agent_orchestrator::config::TaskRuntimeContext,
    runtime: &RunningTask,
    segment: &ScopeSegment,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &mut Vec<agent_orchestrator::dto::TaskItemRow>,
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<TaskSegmentOutcome> {
    let anchor_item = match items.first() {
        Some(item) => item,
        None => return Ok(TaskSegmentOutcome::Continue),
    };

    let mut task_acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
    let process_result = process_item_filtered(
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
    .await;

    // When self_restart fires, process_item_filtered returns RestartRequestedError
    // before we reach the generate_items logic below.  Flush deferred post-actions
    // (pipeline vars + generate_items) to the database NOW so they survive the
    // exec() restart and are available in the next cycle.
    if let Err(ref e) = process_result {
        if e.downcast_ref::<super::super::safety::RestartRequestedError>()
            .is_some()
        {
            // Persist pipeline vars so the new process can read qa_doc_gen_output
            task_ctx.pipeline_vars = task_acc.pipeline_vars.clone();
            if let Ok(json_str) = serde_json::to_string(&task_ctx.pipeline_vars) {
                let _ = state
                    .db_writer
                    .update_task_pipeline_vars(task_id, &json_str)
                    .await;
            }
            // Flush pending generate_items — creates dynamic items in the DB
            flush_pending_generate_items(state, task_id, &mut task_acc, items, task_item_paths)
                .await;
        }
    }
    process_result?;

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
    for item in items.iter() {
        let acc = item_state
            .entry(item.id.clone())
            .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
        acc.merge_task_pipeline_vars(&task_acc.pipeline_vars);
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
            agent_orchestrator::config::InvariantCheckPoint::AfterImplement,
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
    flush_pending_generate_items(state, task_id, &mut task_acc, items, task_item_paths).await;

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
    task_ctx: &mut agent_orchestrator::config::TaskRuntimeContext,
    runtime: &RunningTask,
    segment: &ScopeSegment,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &[agent_orchestrator::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &[String],
) -> Result<()> {
    let max_par = segment.max_parallel;
    let run_dynamic_steps = is_last_item_segment(segment_idx, segments);
    if max_par <= 1 {
        // === Sequential path ===
        let max_item_failures = task_ctx.safety.max_item_step_failures;
        for item in items {
            // FR-035 L1: Skip blocked items (any step at or above threshold)
            let is_blocked = task_ctx
                .item_step_failures
                .iter()
                .any(|((iid, _), &count)| iid == &item.id && count >= max_item_failures);
            if is_blocked {
                continue;
            }
            // FR-035 L1: Skip items in retry backoff
            if let Some(&retry_after) = task_ctx.item_retry_after.get(&item.id) {
                if std::time::Instant::now() < retry_after {
                    insert_event(
                        state,
                        task_id,
                        Some(&item.id),
                        "step_skipped",
                        json!({"reason": "retry_backoff"}),
                    )
                    .await?;
                    continue;
                }
            }
            let acc = item_state
                .entry(item.id.clone())
                .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
            isolation::ensure_item_isolation(state, task_id, item, task_ctx, acc).await?;
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

            // FR-054: Incremental finalize — write terminal status to DB
            // immediately when this is the last item-scope segment, so that
            // `Progress: X/N` updates in real-time.  The batch `finalize_items`
            // call later will re-evaluate the same item (idempotent).
            if run_dynamic_steps {
                finalize_item_execution(state, task_id, item, task_ctx, acc).await?;
            }

            // FR-035 L1: Track per-item per-step failures and apply circuit breaker
            let Some(acc) = item_state.get(&item.id) else {
                continue;
            };
            for (step_id, &exit_code) in &acc.exit_codes {
                if !segment.step_ids.contains(step_id) {
                    continue;
                }
                let key = (item.id.clone(), step_id.clone());
                if exit_code != 0 {
                    let count = task_ctx.item_step_failures.entry(key).or_insert(0);
                    *count += 1;
                    if *count >= max_item_failures {
                        set_item_blocked(state, task_id, &item.id).await?;
                        insert_event(
                            state,
                            task_id,
                            Some(&item.id),
                            "item_blocked_consecutive_failures",
                            json!({
                                "step_id": step_id,
                                "failure_count": *count,
                                "last_exit_code": exit_code,
                            }),
                        )
                        .await?;
                        state.emit_event(
                            task_id,
                            Some(&item.id),
                            "item_blocked_consecutive_failures",
                            json!({
                                "step_id": step_id,
                                "failure_count": *count,
                            }),
                        );
                    } else {
                        // Exponential backoff: 1 failure → 30s, 2 → 120s
                        let delay_secs = match *count {
                            1 => 30,
                            2 => 120,
                            _ => 120,
                        };
                        task_ctx.item_retry_after.insert(
                            item.id.clone(),
                            std::time::Instant::now() + std::time::Duration::from_secs(delay_secs),
                        );
                    }
                } else {
                    // Success: reset failure counter and retry_after
                    task_ctx.item_step_failures.remove(&key);
                    task_ctx.item_retry_after.remove(&item.id);
                }
            }
        }
    } else {
        // === Parallel path ===
        let semaphore = Arc::new(Semaphore::new(max_par));
        let shared_paths = Arc::new(task_item_paths.to_vec());
        let shared_ctx = Arc::new(task_ctx.clone());
        let shared_filter = Arc::new(segment.step_ids.clone());
        let mut join_set = JoinSet::new();
        let mut dispatched_count: usize = 0;

        for item in items {
            let mut seeded_acc = item_state
                .remove(&item.id)
                .unwrap_or_else(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
            isolation::ensure_item_isolation(state, task_id, item, task_ctx, &mut seeded_acc)
                .await?;
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| anyhow::anyhow!("semaphore closed: {}", e))?;
            let state = state.clone();
            let item_id = item.id.clone();
            let item = item.clone();
            let task_id = task_id.to_string();
            let paths = shared_paths.clone();
            let ctx = shared_ctx.clone();
            let filter = shared_filter.clone();
            let item_runtime = runtime.fork();
            let prior_acc = seeded_acc;

            join_set.spawn(async move {
                let _permit = permit;
                let mut acc = prior_acc;
                let result = process_item_filtered_owned(
                    &state,
                    OwnedProcessItemRequest {
                        task_id: task_id.clone(),
                        item: item.clone(),
                        task_item_paths: paths,
                        task_ctx: ctx.clone(),
                        runtime: item_runtime,
                        step_filter: Some(filter),
                        run_dynamic_steps,
                    },
                    &mut acc,
                )
                .await;
                // FR-054: Incremental finalize — write terminal status to DB
                // immediately so `Progress: X/N` updates in real-time.
                // The batch `finalize_items` call later will re-evaluate
                // the same item (idempotent).
                if run_dynamic_steps && result.is_ok() {
                    if let Err(e) =
                        finalize_item_execution(&state, &task_id, &item, &ctx, &mut acc).await
                    {
                        return (item_id, acc, Err(e));
                    }
                }
                (item_id, acc, result)
            });
            dispatched_count += 1;
            // FR-055: stagger delay between parallel spawns
            let stagger_ms = segment.stagger_delay_ms;
            if stagger_ms > 0 && dispatched_count < items.len() {
                tokio::time::sleep(std::time::Duration::from_millis(stagger_ms)).await;
            }
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

        // FR-053: Completeness check — ensure every item was dispatched into the
        // JoinSet.  If the for-loop exited early (cancellation, semaphore closure,
        // or any `?` propagation in the setup path), `dispatched_count` will be
        // less than `items.len()`.  Fail explicitly so the cycle does not proceed
        // as if all items were processed.
        let expected = items.len();
        if dispatched_count < expected {
            let msg = format!(
                "parallel item segment incomplete: dispatched {}/{} items",
                dispatched_count, expected
            );
            warn!(
                dispatched_count,
                expected, "FR-053 completeness check failed"
            );
            insert_event(
                state,
                task_id,
                None,
                "parallel_dispatch_incomplete",
                json!({
                    "dispatched": dispatched_count,
                    "expected": expected,
                }),
            )
            .await?;
            anyhow::bail!("{}", msg);
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
    task_ctx: &mut agent_orchestrator::config::TaskRuntimeContext,
    segment_idx: usize,
    segments: &[ScopeSegment],
    items: &mut Vec<agent_orchestrator::dto::TaskItemRow>,
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
            // Persist pipeline_vars for eliminated items (they won't reach finalize)
            for es in &eval_states {
                if result.eliminated_ids.contains(&es.item_id) {
                    let existing = items
                        .iter()
                        .find(|i| i.id == es.item_id)
                        .and_then(|i| i.dynamic_vars_json.as_deref());
                    super::super::item_executor::persist_item_pipeline_vars(
                        state,
                        &es.item_id,
                        existing,
                        &es.pipeline_vars,
                    )
                    .await;
                }
            }
            promote_winner_vars(&mut task_ctx.pipeline_vars, &result);
            isolation::apply_winner_if_needed(state, task_id, task_ctx).await?;
            persist_selection_to_store(state, task_ctx, task_id, &result, &config).await;
            // R4: Collect per-item scores for observability
            let scores: serde_json::Map<String, serde_json::Value> = eval_states
                .iter()
                .filter_map(|s| {
                    config.metric_var.as_ref().and_then(|mv| {
                        s.pipeline_vars
                            .get(mv)
                            .and_then(|v| v.parse::<f64>().ok())
                            .map(|score| (s.item_id.clone(), json!(score)))
                    })
                })
                .collect();
            insert_event(
                state,
                task_id,
                None,
                "item_selected",
                json!({
                    "winner": result.winner_id,
                    "eliminated": result.eliminated_ids,
                    "selection_succeeded": true,
                    "scores": scores,
                }),
            )
            .await?;
            // Filter out eliminated items
            items.retain(|item| !result.eliminated_ids.contains(&item.id));
            *task_item_paths = items.iter().map(|i| i.qa_file_path.clone()).collect();
        }
        Err(e) => {
            warn!(error = %e, "item_select failed");
            // R1: Emit item_select_failed event with diagnostic payload
            let item_vars: serde_json::Map<String, serde_json::Value> = eval_states
                .iter()
                .map(|s| {
                    let keys: Vec<String> = s.pipeline_vars.keys().cloned().collect();
                    (s.item_id.clone(), json!(keys))
                })
                .collect();
            insert_event(
                state,
                task_id,
                None,
                "item_select_failed",
                json!({
                    "error": e.to_string(),
                    "metric_var": config.metric_var,
                    "item_count": eval_states.len(),
                    "item_vars": item_vars,
                }),
            )
            .await?;
            // Still apply winner via fallback (constraint: don't change control flow)
            isolation::apply_winner_if_needed(state, task_id, task_ctx).await?;
        }
    }

    Ok(())
}

/// Finalize all items at the end of segment-based execution.
pub(super) async fn finalize_items(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
    items: &[agent_orchestrator::dto::TaskItemRow],
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
    items: &[agent_orchestrator::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_acc: &StepExecutionAccumulator,
    task_pipeline_vars: &agent_orchestrator::config::PipelineVariables,
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
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
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
    items: &[agent_orchestrator::dto::TaskItemRow],
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
fn has_item_select_step(
    segment: &ScopeSegment,
    plan: &agent_orchestrator::config::TaskExecutionPlan,
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
#[cfg(test)]
pub(super) fn find_item_select_config_for_test(
    plan: &agent_orchestrator::config::TaskExecutionPlan,
) -> Option<agent_orchestrator::config::ItemSelectConfig> {
    find_item_select_config(plan)
}

fn find_item_select_config(
    plan: &agent_orchestrator::config::TaskExecutionPlan,
) -> Option<agent_orchestrator::config::ItemSelectConfig> {
    plan.steps.iter().find_map(|s| s.item_select_config.clone())
}

/// Collect item evaluation states from item_state accumulators.
pub(super) fn collect_item_eval_states(
    items: &[agent_orchestrator::dto::TaskItemRow],
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
    pipeline_vars: &mut agent_orchestrator::config::PipelineVariables,
    result: &agent_orchestrator::config::SelectionResult,
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
    task_ctx: &agent_orchestrator::config::TaskRuntimeContext,
    task_id: &str,
    result: &agent_orchestrator::config::SelectionResult,
    config: &agent_orchestrator::config::ItemSelectConfig,
) {
    if let Some(ref store_target) = config.store_result {
        let value = serde_json::json!({
            "winner_id": result.winner_id,
            "eliminated_ids": result.eliminated_ids,
            "winner_vars": result.winner_vars,
        });
        let cr = match agent_orchestrator::config_load::read_loaded_config(state) {
            Ok(cfg) => cfg.config.custom_resources.clone(),
            Err(error) => {
                warn!(%error, "failed to read active config while persisting item_select result");
                return;
            }
        };
        let op = agent_orchestrator::store::StoreOp::Put {
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

/// Flush any pending `generate_items` post-action from the accumulator.
///
/// Extracts dynamic items from pipeline variables, creates them in the database,
/// and refreshes the in-memory items list.  Called both on the normal path (after
/// the task segment completes) and on the restart path (before `RestartRequestedError`
/// propagates up to exec()) so that dynamic items survive a process restart.
async fn flush_pending_generate_items(
    state: &Arc<InnerState>,
    task_id: &str,
    task_acc: &mut StepExecutionAccumulator,
    items: &mut Vec<agent_orchestrator::dto::TaskItemRow>,
    task_item_paths: &mut Vec<String>,
) {
    let gen_action = match task_acc.pending_generate_items.take() {
        Some(a) => a,
        None => return,
    };

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
                    if let Err(e) = insert_event(
                        state,
                        task_id,
                        None,
                        "items_generated",
                        json!({"count": new_items.len(), "replace": gen_action.replace}),
                    )
                    .await
                    {
                        warn!(error = %e, "failed to emit items_generated event");
                    }
                    // Refresh items list — when dynamic items exist,
                    // subsequent item-scoped steps target only dynamic items.
                    // Static items excluded by dynamic replacement are marked
                    // "replaced" in the DB so they don't count as unresolved.
                    match list_task_items_for_cycle(state, task_id).await {
                        Ok(all_items) => {
                            // Filter out terminal items (preserved from prior run).
                            const TERMINAL_STATUSES: &[&str] = &[
                                "qa_passed",
                                "skipped",
                                "fixed",
                                "verified",
                                "eliminated",
                                "replaced",
                            ];
                            let all_items: Vec<_> = all_items
                                .into_iter()
                                .filter(|i| !TERMINAL_STATUSES.contains(&i.status.as_str()))
                                .collect();

                            let has_dynamic = all_items.iter().any(|i| i.source == "dynamic");
                            if has_dynamic {
                                // Mark static items as "replaced" so they don't
                                // appear unresolved at task completion.
                                for item in all_items.iter().filter(|i| i.source != "dynamic") {
                                    let _ = state
                                        .db_writer
                                        .update_task_item_status(&item.id, "replaced")
                                        .await;
                                }
                                *items = all_items
                                    .into_iter()
                                    .filter(|i| i.source == "dynamic")
                                    .collect();
                            } else {
                                *items = all_items;
                            }
                            *task_item_paths =
                                items.iter().map(|i| i.qa_file_path.clone()).collect();
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to refresh items after generate_items");
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "failed to create dynamic items");
                    let _ = insert_event(
                        state,
                        task_id,
                        None,
                        "items_generation_failed",
                        json!({
                            "error": e.to_string(),
                            "stage": "db_create",
                            "fallback": "static_items_retained",
                        }),
                    )
                    .await;
                }
            }
        }
        Ok(_) => {} // empty items, no-op
        Err(e) => {
            warn!(error = %e, "failed to extract dynamic items");
            let _ = insert_event(
                state,
                task_id,
                None,
                "items_generation_failed",
                json!({
                    "error": e.to_string(),
                    "from_var": gen_action.from_var,
                    "json_path": gen_action.json_path,
                    "fallback": "static_items_retained",
                }),
            )
            .await;
        }
    }
}
