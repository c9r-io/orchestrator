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
    let task_id_owned = task_id.to_owned();
    let status_owned = status.to_owned();
    state
        .async_database
        .writer()
        .call(move |conn| {
            let command_runs: i64 = conn.query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
                rusqlite::params![task_id_owned],
                |row| row.get(0),
            )?;
            let metric = agent_orchestrator::db::TaskExecutionMetric {
                task_id: task_id_owned.clone(),
                status: status_owned,
                current_cycle,
                unresolved_items,
                total_items,
                failed_items,
                command_runs,
                created_at: now_ts(),
            };
            conn.execute(
                "INSERT INTO task_execution_metrics (task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    metric.task_id,
                    metric.status,
                    metric.current_cycle as i64,
                    metric.unresolved_items,
                    metric.total_items,
                    metric.failed_items,
                    metric.command_runs,
                    metric.created_at
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
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
    state
        .async_database
        .writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE task_items SET status = 'blocked' WHERE id = ?1 AND task_id = ?2",
                rusqlite::params![item_id, task_id],
            )?;
            Ok(())
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
}

/// FR-035: Resets all blocked items back to unresolved for a task. Returns the count reset.
pub async fn reset_blocked_items(state: &InnerState, task_id: &str) -> Result<u64> {
    let task_id = task_id.to_owned();
    state
        .async_database
        .writer()
        .call(move |conn| {
            let count = conn.execute(
                "UPDATE task_items SET status = 'unresolved' WHERE task_id = ?1 AND status = 'blocked'",
                rusqlite::params![task_id],
            )?;
            Ok(count as u64)
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
}

/// FR-035: Queries recent cycle_started event timestamps from DB (newest first).
pub async fn query_recent_cycle_timestamps(
    state: &InnerState,
    task_id: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let task_id = task_id.to_owned();
    state
        .async_database
        .reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT created_at FROM events WHERE task_id = ?1 AND event_type = 'cycle_started' ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![task_id, limit], |row| row.get(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
}

/// Detect whether this task is resuming from a self_restart.
///
/// Returns `true` if a `self_restart_ready` event exists that has not been
/// acknowledged by a subsequent `restart_resumed` event.
pub async fn detect_restart_resume(state: &InnerState, task_id: &str) -> Result<bool> {
    let task_id = task_id.to_owned();
    state
        .async_database
        .reader()
        .call(move |conn| {
            let has_unacked_restart: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM events
                     WHERE task_id = ?1 AND event_type = 'self_restart_ready'
                     AND id > COALESCE(
                         (SELECT MAX(id) FROM events WHERE task_id = ?1 AND event_type = 'restart_resumed'),
                         0
                     )",
                    rusqlite::params![task_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            Ok(has_unacked_restart)
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
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
    state
        .async_database
        .reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT json_extract(payload_json, '$.step')
                 FROM events
                 WHERE task_id = ?1
                   AND event_type = 'step_finished'
                   AND cycle = ?2
                   AND json_extract(payload_json, '$.step') IS NOT NULL",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![task_id, cycle], |row| {
                    row.get::<_, String>(0)
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
}

/// Mark a command run as killed by the system after inflight_wait_timeout.
pub async fn mark_command_run_killed(state: &InnerState, run_id: &str) -> Result<()> {
    let run_id = run_id.to_owned();
    let now = agent_orchestrator::config_load::now_ts();
    state
        .async_database
        .writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE command_runs SET exit_code = -9, ended_at = ?2 WHERE id = ?1 AND exit_code = -1",
                rusqlite::params![run_id, now],
            )?;
            Ok(())
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::db::open_conn;
    use agent_orchestrator::dto::CreateTaskPayload;
    use agent_orchestrator::task_ops::create_task_impl;
    use agent_orchestrator::test_utils::TestState;

    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
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
        assert!(is_task_paused_in_db(&state, &task_id)
            .await
            .expect("check paused status"));

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
