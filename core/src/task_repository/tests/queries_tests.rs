use super::fixtures::seed_task;
use super::super::trait_def::TaskRepository;
use super::super::types::TaskRepositorySource;
use super::super::SqliteTaskRepository;
use crate::db::open_conn;
use crate::test_utils::TestState;
use rusqlite::params;

#[test]
fn resolve_task_id_supports_exact_and_prefix() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let exact = repo
        .resolve_task_id(&task_id)
        .expect("exact id must resolve");
    assert_eq!(exact, task_id);

    let prefix = &task_id[0..8];
    let by_prefix = repo
        .resolve_task_id(prefix)
        .expect("single prefix match must resolve");
    assert_eq!(by_prefix, task_id);
}

#[test]
fn load_task_summary_and_counts_are_consistent() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let summary = repo
        .load_task_summary(&task_id)
        .expect("summary should load from repo");
    assert_eq!(summary.id, task_id);
    assert!(!summary.created_at.is_empty());
    assert!(!summary.updated_at.is_empty());

    let (total, finished, failed) = repo
        .load_task_item_counts(&summary.id)
        .expect("item counts should load");
    assert!(total >= 1);
    assert_eq!(finished, 0);
    assert_eq!(failed, 0);
}

// ── find_latest_resumable_task_id ──────────────────────────────────

#[test]
fn find_latest_resumable_task_id_returns_running_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set running");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_interrupted_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='interrupted' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set interrupted");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_paused_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set paused");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_includes_pending_when_flag_set() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    // task is created with status 'pending' by default
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    // Without include_pending, pending task should NOT be found
    let without = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(without, None);

    // With include_pending, pending task SHOULD be found
    let with = repo
        .find_latest_resumable_task_id(true)
        .expect("query should succeed");
    assert_eq!(with, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_none_when_all_completed() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='completed' WHERE id = ?1",
        params![task_id],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, None);
}

#[test]
fn find_latest_resumable_task_id_returns_none_for_failed_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='failed' WHERE id = ?1",
        params![task_id],
    )
    .expect("set failed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, None);
}

// ── load_task_runtime_row ──────────────────────────────────────────

#[test]
fn load_task_runtime_row_returns_expected_fields() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let row = repo
        .load_task_runtime_row(&task_id)
        .expect("runtime row should load");
    assert!(!row.workspace_id.is_empty());
    assert!(!row.workflow_id.is_empty());
    assert!(!row.workspace_root_raw.is_empty());
    assert_eq!(row.goal, "repo-test-goal");
    assert_eq!(row.current_cycle, 0);
    assert_eq!(row.init_done, 0);
}

#[test]
fn load_task_runtime_row_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let result = repo.load_task_runtime_row("nonexistent-id");
    assert!(result.is_err());
}

// ── first_task_item_id ─────────────────────────────────────────────

#[test]
fn first_task_item_id_returns_item() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let item_id = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert!(item_id.is_some(), "seeded task must have at least one item");
}

#[test]
fn first_task_item_id_returns_none_for_empty_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "DELETE FROM task_items WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("delete items");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_id = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert_eq!(item_id, None);
}

#[test]
fn first_task_item_id_respects_order_no() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert a second item with a lower order_no so it should be returned first
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-low-order', ?1, -1, '/tmp/qa.md', 'pending', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert low order item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let first = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert_eq!(first, Some("item-low-order".to_string()));
}

// ── count_unresolved_items ─────────────────────────────────────────

#[test]
fn count_unresolved_items_zero_when_all_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert_eq!(count, 0);
}

#[test]
fn count_unresolved_items_counts_unresolved_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE task_items SET status='unresolved' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("set unresolved");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert!(count >= 1);
}

#[test]
fn count_unresolved_items_counts_qa_failed_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE task_items SET status='qa_failed' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("set qa_failed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert!(count >= 1);
}

#[test]
fn count_unresolved_items_mixed_statuses() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Add two more items with different statuses
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-unresolved', ?1, 10, '/tmp/qa1.md', 'unresolved', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert unresolved");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-qa-failed', ?1, 11, '/tmp/qa2.md', 'qa_failed', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert qa_failed");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-passed', ?1, 12, '/tmp/qa3.md', 'qa_passed', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert qa_passed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    // Only 'unresolved' and 'qa_failed' should be counted (original item is still 'pending')
    assert_eq!(count, 2);
}

// ── list_task_items_for_cycle ───────────────────────────────────────

#[test]
fn list_task_items_for_cycle_returns_items_in_order() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Add a second item with a higher order_no
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-second', ?1, 99, '/tmp/qa_second.md', 'pending', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert second item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let items = repo
        .list_task_items_for_cycle(&task_id)
        .expect("list should succeed");
    assert!(items.len() >= 2, "should have at least 2 items");
    // Last item should be our inserted one
    let last = items.last().expect("last item should exist");
    assert_eq!(last.id, "item-second");
    assert_eq!(last.qa_file_path, "/tmp/qa_second.md");
}

#[test]
fn list_task_items_for_cycle_returns_empty_for_unknown_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let items = repo
        .list_task_items_for_cycle("nonexistent-task-id")
        .expect("list should succeed even for unknown task");
    assert!(items.is_empty());
}

// ── load_task_name ─────────────────────────────────────────────────

#[test]
fn load_task_name_returns_name() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let name = repo
        .load_task_name(&task_id)
        .expect("load_task_name should succeed");
    assert_eq!(name, Some("repo-test".to_string()));
}

#[test]
fn load_task_name_returns_none_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let name = repo
        .load_task_name("nonexistent-id")
        .expect("load_task_name should succeed");
    assert_eq!(name, None);
}

// ── load_task_status ───────────────────────────────────────────────

#[test]
fn load_task_status_returns_current_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let status = repo
        .load_task_status(&task_id)
        .expect("load_task_status should succeed");
    assert_eq!(status, Some("pending".to_string()));
}

#[test]
fn load_task_status_reflects_status_change() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set running");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let status = repo
        .load_task_status(&task_id)
        .expect("load_task_status should succeed");
    assert_eq!(status, Some("running".to_string()));
}

#[test]
fn load_task_status_returns_none_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let status = repo
        .load_task_status("nonexistent-id")
        .expect("load_task_status should succeed");
    assert_eq!(status, None);
}
