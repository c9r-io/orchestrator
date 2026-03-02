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
    let repo = SqliteTaskRepository::new(state.database.clone());
    let (total_items, _finished_items, failed_items) = repo.load_task_item_counts(task_id)?;
    let conn = state.database.connection()?;
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
    let conn = state.database.connection()?;
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
    SqliteTaskRepository::new(state.database.clone()).prepare_task_for_start_batch(task_id)?;
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
    SqliteTaskRepository::new(state.database.clone()).find_latest_resumable_task_id(include_pending)
}

pub fn first_task_item_id(state: &InnerState, task_id: &str) -> Result<Option<String>> {
    SqliteTaskRepository::new(state.database.clone()).first_task_item_id(task_id)
}

pub fn count_unresolved_items(state: &InnerState, task_id: &str) -> Result<i64> {
    SqliteTaskRepository::new(state.database.clone()).count_unresolved_items(task_id)
}

pub fn list_task_items_for_cycle(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<crate::dto::TaskItemRow>> {
    SqliteTaskRepository::new(state.database.clone()).list_task_items_for_cycle(task_id)
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
    let status = SqliteTaskRepository::new(state.database.clone()).load_task_status(task_id)?;
    Ok(matches!(status.as_deref(), Some("paused")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

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

    #[test]
    fn task_state_wrappers_delegate_to_repository_and_writer() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        prepare_task_for_start(&state, &task_id).expect("prepare task");
        let resumable = find_latest_resumable_task_id(&state, true).expect("find resumable task");
        let first_item = first_task_item_id(&state, &task_id).expect("load first item");
        let items = list_task_items_for_cycle(&state, &task_id).expect("list task items");

        assert_eq!(resumable.as_deref(), Some(task_id.as_str()));
        assert_eq!(items.len(), 1);
        assert_eq!(first_item.as_deref(), Some(items[0].id.as_str()));
        assert_eq!(
            count_unresolved_items(&state, &task_id).expect("count unresolved items"),
            0
        );

        update_task_cycle_state(&state, &task_id, 2, true).expect("update cycle state");
        record_task_execution_metric(&state, &task_id, "running", 2, 0)
            .expect("record task metric");
        set_task_status(&state, &task_id, "paused", false).expect("pause task");
        assert!(is_task_paused_in_db(&state, &task_id).expect("check paused status"));

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
