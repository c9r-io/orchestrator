use agent_orchestrator::config_load::now_ts;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::state::InnerState;

use anyhow::Result;
use serde_json::json;

async fn persist_task_execution_metric(
    state: &InnerState,
    task_id: &str,
    status: &str,
    current_cycle: u32,
    unresolved_items: i64,
) -> Result<()> {
    let (total_items, _finished_items, failed_items) =
        state.task_repo.load_task_item_counts(task_id).await?;
    agent_orchestrator::scheduler_state::record_task_execution_metric(
        &state.async_database,
        agent_orchestrator::scheduler_state::TaskExecutionMetricInput {
            task_id: task_id.to_owned(),
            status: status.to_owned(),
            current_cycle: current_cycle as i64,
            unresolved_items,
            total_items,
            failed_items,
            created_at: now_ts(),
        },
    )
    .await
}

pub(crate) async fn record_task_execution_metric(
    state: &InnerState,
    task_id: &str,
    status: &str,
    current_cycle: u32,
    unresolved_items: i64,
) -> Result<()> {
    persist_task_execution_metric(state, task_id, status, current_cycle, unresolved_items).await
}

/// Updates the persisted task status and optionally stamps completion fields.
pub async fn set_task_status(
    state: &InnerState,
    task_id: &str,
    status: &str,
    set_completed: bool,
) -> Result<()> {
    state
        .db_writer
        .set_task_status(task_id, status, set_completed)
        .await
}

/// Prepares a task for execution and records a `task_started` event.
pub async fn prepare_task_for_start(state: &InnerState, task_id: &str) -> Result<()> {
    state
        .task_repo
        .prepare_task_for_start_batch(task_id)
        .await?;
    insert_event(
        state,
        task_id,
        None,
        "task_started",
        json!({"reason":"manual_or_resume"}),
    )
    .await?;
    Ok(())
}

/// Finds the latest resumable task identifier.
pub async fn find_latest_resumable_task_id(
    state: &InnerState,
    include_pending: bool,
) -> Result<Option<String>> {
    state
        .task_repo
        .find_latest_resumable_task_id(include_pending)
        .await
}

/// Returns the first task item identifier for a task, if any.
pub async fn first_task_item_id(state: &InnerState, task_id: &str) -> Result<Option<String>> {
    state.task_repo.first_task_item_id(task_id).await
}

/// Counts unresolved task items for a task.
pub async fn count_unresolved_items(state: &InnerState, task_id: &str) -> Result<i64> {
    state.task_repo.count_unresolved_items(task_id).await
}

/// Counts stale pending items (FR-038).
pub async fn count_stale_pending_items(state: &InnerState, task_id: &str) -> Result<i64> {
    state.task_repo.count_stale_pending_items(task_id).await
}

/// Returns in-flight command runs for a task (FR-038).
pub async fn find_inflight_command_runs_for_task(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<agent_orchestrator::task_repository::InflightRunRecord>> {
    state
        .task_repo
        .find_inflight_command_runs_for_task(task_id)
        .await
}

/// Returns completed runs whose parent items are still `pending` (FR-038).
pub async fn find_completed_runs_for_pending_items(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<agent_orchestrator::task_repository::CompletedRunRecord>> {
    state
        .task_repo
        .find_completed_runs_for_pending_items(task_id)
        .await
}

/// Lists task items for the current cycle.
pub async fn list_task_items_for_cycle(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<agent_orchestrator::dto::TaskItemRow>> {
    state.task_repo.list_task_items_for_cycle(task_id).await
}

/// Persists the current cycle number and init state for a task.
pub async fn update_task_cycle_state(
    state: &InnerState,
    task_id: &str,
    current_cycle: u32,
    init_done: bool,
) -> Result<()> {
    state
        .db_writer
        .update_task_cycle_state(task_id, current_cycle, init_done)
        .await
}

pub(crate) async fn is_task_paused_in_db(state: &InnerState, task_id: &str) -> Result<bool> {
    let status = state.task_repo.load_task_status(task_id).await?;
    Ok(matches!(status.as_deref(), Some("paused")))
}

/// FR-035: Marks a task item as blocked (circuit-breaker tripped).
pub async fn set_item_blocked(state: &InnerState, task_id: &str, item_id: &str) -> Result<()> {
    let task_id = task_id.to_owned();
    let item_id = item_id.to_owned();
    agent_orchestrator::scheduler_state::mark_item_blocked(&state.async_database, task_id, item_id)
        .await
}

/// FR-035: Resets all blocked items back to unresolved for a task. Returns the count reset.
pub async fn reset_blocked_items(state: &InnerState, task_id: &str) -> Result<u64> {
    let task_id = task_id.to_owned();
    agent_orchestrator::scheduler_state::reset_blocked_items(&state.async_database, task_id).await
}

/// FR-035: Queries recent cycle_started event timestamps from DB (newest first).
pub async fn query_recent_cycle_timestamps(
    state: &InnerState,
    task_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let task_id = task_id.to_owned();
    agent_orchestrator::scheduler_state::recent_cycle_timestamps(
        &state.async_database,
        task_id,
        limit,
    )
    .await
}

/// Detect whether this task is resuming from a self_restart.
///
/// Returns `true` if a `self_restart_ready` event exists that has not been
/// acknowledged by a subsequent `restart_resumed` event.
pub async fn detect_restart_resume(state: &InnerState, task_id: &str) -> Result<bool> {
    let task_id = task_id.to_owned();
    agent_orchestrator::scheduler_state::has_unacked_self_restart(&state.async_database, task_id)
        .await
}

/// Query step IDs that already finished in a given cycle for this task.
///
/// Used after restart to avoid re-running steps that completed before the
/// restart was triggered.
pub async fn query_completed_steps_in_cycle(
    state: &InnerState,
    task_id: &str,
    cycle: u32,
) -> Result<std::collections::HashSet<String>> {
    let task_id = task_id.to_owned();
    agent_orchestrator::scheduler_state::completed_steps_in_cycle(
        &state.async_database,
        task_id,
        cycle,
    )
    .await
}

/// FR-052: Counts recent heartbeat events for specified item IDs since cutoff.
pub async fn count_recent_heartbeats_for_items(
    state: &InnerState,
    task_id: &str,
    item_ids: &[String],
    cutoff_ts: &str,
) -> Result<i64> {
    state
        .task_repo
        .count_recent_heartbeats_for_items(task_id, item_ids, cutoff_ts)
        .await
}

/// Mark a command run as killed by the system after inflight_wait_timeout.
pub async fn mark_command_run_killed(state: &InnerState, run_id: &str) -> Result<()> {
    let run_id = run_id.to_owned();
    let now = agent_orchestrator::config_load::now_ts();
    agent_orchestrator::scheduler_state::mark_command_run_killed(&state.async_database, run_id, now)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::dto::CreateTaskPayload;
    use agent_orchestrator::task_ops::create_task_impl;
    use agent_orchestrator::test_utils::TestState;
    use orchestrator_persistence::test_support::open_conn;

    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/task_state_test.md");
        std::fs::write(&qa_file, "# task state test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-state-test".to_string()),
                goal: Some("exercise task_state wrappers".to_string()),
                ..Default::default()
            },
        )
        .expect("create task");
        (state, created.id)
    }

    #[tokio::test]
    async fn task_state_wrappers_delegate_to_repository_and_writer() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        prepare_task_for_start(&state, &task_id)
            .await
            .expect("prepare task");
        let resumable = find_latest_resumable_task_id(&state, true)
            .await
            .expect("find resumable task");
        let first_item = first_task_item_id(&state, &task_id)
            .await
            .expect("load first item");
        let items = list_task_items_for_cycle(&state, &task_id)
            .await
            .expect("list task items");

        assert_eq!(resumable.as_deref(), Some(task_id.as_str()));
        assert_eq!(items.len(), 1);
        assert_eq!(first_item.as_deref(), Some(items[0].id.as_str()));
        assert_eq!(
            count_unresolved_items(&state, &task_id)
                .await
                .expect("count unresolved items"),
            0
        );

        update_task_cycle_state(&state, &task_id, 2, true)
            .await
            .expect("update cycle state");
        record_task_execution_metric(&state, &task_id, "running", 2, 0)
            .await
            .expect("record task metric");
        set_task_status(&state, &task_id, "paused", false)
            .await
            .expect("pause task");
        assert!(
            is_task_paused_in_db(&state, &task_id)
                .await
                .expect("check paused status")
        );

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let metric_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_execution_metrics WHERE task_id = ?1",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .expect("count task metrics");
        assert_eq!(metric_rows, 1);
    }
}
