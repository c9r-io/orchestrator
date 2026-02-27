use crate::config_load::now_ts;
use crate::events::insert_event;
use crate::state::InnerState;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::Result;
use serde_json::json;

fn persist_task_execution_metric(
    state: &InnerState,
    task_id: &str,
    status: &str,
    current_cycle: u32,
    unresolved_items: i64,
) -> Result<()> {
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let (total_items, _finished_items, failed_items) = repo.load_task_item_counts(task_id)?;
    let conn = crate::db::open_conn(&state.db_path)?;
    let command_runs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
        rusqlite::params![task_id],
        |row| row.get(0),
    )?;
    let metric = crate::db::TaskExecutionMetric {
        task_id: task_id.to_string(),
        status: status.to_string(),
        current_cycle,
        unresolved_items,
        total_items,
        failed_items,
        command_runs,
        created_at: now_ts(),
    };
    crate::db::insert_task_execution_metric(&state.db_path, &metric)
}

pub(crate) fn record_task_execution_metric(
    state: &InnerState,
    task_id: &str,
    status: &str,
    current_cycle: u32,
    unresolved_items: i64,
) -> Result<()> {
    persist_task_execution_metric(state, task_id, status, current_cycle, unresolved_items)
}

pub fn set_task_status(
    state: &InnerState,
    task_id: &str,
    status: &str,
    set_completed: bool,
) -> Result<()> {
    state
        .db_writer
        .set_task_status(task_id, status, set_completed)
}

pub fn prepare_task_for_start(state: &InnerState, task_id: &str) -> Result<()> {
    SqliteTaskRepository::new(state.db_path.clone()).prepare_task_for_start_batch(task_id)?;
    insert_event(
        state,
        task_id,
        None,
        "task_started",
        json!({"reason":"manual_or_resume"}),
    )?;
    Ok(())
}

pub fn find_latest_resumable_task_id(
    state: &InnerState,
    include_pending: bool,
) -> Result<Option<String>> {
    SqliteTaskRepository::new(state.db_path.clone()).find_latest_resumable_task_id(include_pending)
}

pub fn first_task_item_id(state: &InnerState, task_id: &str) -> Result<Option<String>> {
    SqliteTaskRepository::new(state.db_path.clone()).first_task_item_id(task_id)
}

pub fn count_unresolved_items(state: &InnerState, task_id: &str) -> Result<i64> {
    SqliteTaskRepository::new(state.db_path.clone()).count_unresolved_items(task_id)
}

pub fn list_task_items_for_cycle(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<crate::dto::TaskItemRow>> {
    SqliteTaskRepository::new(state.db_path.clone()).list_task_items_for_cycle(task_id)
}

pub fn update_task_cycle_state(
    state: &InnerState,
    task_id: &str,
    current_cycle: u32,
    init_done: bool,
) -> Result<()> {
    state
        .db_writer
        .update_task_cycle_state(task_id, current_cycle, init_done)
}

pub(crate) fn is_task_paused_in_db(state: &InnerState, task_id: &str) -> Result<bool> {
    let status = SqliteTaskRepository::new(state.db_path.clone()).load_task_status(task_id)?;
    Ok(matches!(status.as_deref(), Some("paused")))
}
