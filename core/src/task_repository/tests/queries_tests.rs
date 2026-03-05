use super::super::trait_def::TaskRepository;
use super::super::types::TaskRepositorySource;
use super::super::SqliteTaskRepository;
use super::fixtures::seed_task;
use crate::db::open_conn;
use crate::dto::CreateTaskPayload;
use crate::task_ops::create_task_impl;
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
fn resolve_task_id_errors_for_not_found() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let err = repo
        .resolve_task_id("nonexistent")
        .expect_err("should error for nonexistent task");
    assert!(err.to_string().contains("task not found"));
}

#[test]
fn resolve_task_id_errors_for_multiple_matches() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert two tasks with IDs that share a common prefix
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, created_at, updated_at) VALUES ('prefix-aaa-1', 'task1', 'pending', 'test', '[]', 'default', 'default', 'default', '/tmp', '[]', '/tmp', '{}', 'once', 0, 0, ?1, ?1)",
        params![ts],
    )
    .expect("insert task 1");
    conn.execute(
        "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, created_at, updated_at) VALUES ('prefix-aaa-2', 'task2', 'pending', 'test', '[]', 'default', 'default', 'default', '/tmp', '[]', '/tmp', '{}', 'once', 0, 0, ?1, ?1)",
        params![ts],
    )
    .expect("insert task 2");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let err = repo
        .resolve_task_id("prefix-aaa")
        .expect_err("should error for multiple matches");
    assert!(
        err.to_string().contains("multiple tasks match prefix"),
        "error should mention multiple matches, got: {}",
        err
    );
}

#[test]
fn load_task_summary_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let result = repo.load_task_summary("nonexistent-id");
    assert!(result.is_err());
}

// ── load_task_detail_rows ─────────────────────────────────────────────

#[test]
fn load_task_detail_rows_returns_items_runs_and_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let (items, runs, events) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    // Seeded task should have at least one item
    assert!(!items.is_empty(), "should have task items");
    assert!(runs.is_empty(), "should have no command runs initially");
    assert!(events.is_empty(), "should have no events initially");

    // Verify item fields
    let first_item = &items[0];
    assert!(!first_item.id.is_empty());
    assert_eq!(first_item.task_id, task_id);
    assert!(!first_item.qa_file_path.is_empty());
}

#[test]
fn load_task_detail_rows_includes_command_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task item exists");

    // Insert a command run
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES ('cr-1', ?1, 'qa', 'echo test', '/tmp', 'default', 'agent', 0, '/tmp/out.log', '/tmp/err.log', '{}', '[]', NULL, NULL, 'unknown', ?2, ?2, 0)",
        params![item_id, ts],
    )
    .expect("insert command run");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (items, runs, events) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    assert!(!items.is_empty());
    assert_eq!(runs.len(), 1, "should have one command run");
    assert!(events.is_empty());

    let run = &runs[0];
    assert_eq!(run.id, "cr-1");
    assert_eq!(run.phase, "qa");
    assert_eq!(run.command, "echo test");
    assert!(!run.interrupted);
}

#[test]
fn load_task_detail_rows_includes_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert an event
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO events (id, task_id, task_item_id, event_type, payload_json, created_at) VALUES (1, ?1, NULL, 'status_change', '{\"from\":\"pending\",\"to\":\"running\"}', ?2)",
        params![task_id, ts],
    )
    .expect("insert event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (items, runs, events) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    assert!(!items.is_empty());
    assert!(runs.is_empty());
    assert_eq!(events.len(), 1, "should have one event");

    let event = &events[0];
    assert_eq!(event.event_type, "status_change");
    assert!(event.payload.is_object());
}

#[test]
fn load_task_detail_rows_returns_empty_for_unknown_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let (items, runs, events) = repo
        .load_task_detail_rows("nonexistent-task-id")
        .expect("should succeed even for unknown task");

    assert!(items.is_empty());
    assert!(runs.is_empty());
    assert!(events.is_empty());
}

// ── list_task_ids_ordered_by_created_desc ─────────────────────────────

#[test]
fn list_task_ids_ordered_by_created_desc_returns_tasks() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let ids = repo
        .list_task_ids_ordered_by_created_desc()
        .expect("should list task ids");

    assert!(!ids.is_empty(), "should have at least one task");
}

#[test]
fn list_task_ids_ordered_by_created_desc_respects_order() {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let qa_file = state
        .app_root
        .join("workspace/default/docs/qa/repo_test.md");
    std::fs::write(&qa_file, "# repository test\n").expect("seed qa file");

    // Create tasks with small delays to ensure different timestamps
    let task1 = create_task_impl(
        &state,
        CreateTaskPayload {
            name: Some("task-first".to_string()),
            ..Default::default()
        },
    )
    .expect("task1 should be created");

    std::thread::sleep(std::time::Duration::from_millis(10));

    let task2 = create_task_impl(
        &state,
        CreateTaskPayload {
            name: Some("task-second".to_string()),
            ..Default::default()
        },
    )
    .expect("task2 should be created");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let ids = repo
        .list_task_ids_ordered_by_created_desc()
        .expect("should list task ids");

    // Most recent task should be first
    assert_eq!(ids.first(), Some(&task2.id));
    // Older task should come later
    assert!(ids.contains(&task1.id));
}

// ── list_task_log_runs edge cases ─────────────────────────────────────

#[test]
fn list_task_log_runs_returns_empty_when_no_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let runs = repo
        .list_task_log_runs(&task_id, 10)
        .expect("should succeed");

    assert!(runs.is_empty(), "should have no runs initially");
}

// ── pooled connection tests ────────────────────────────────────────────

// These tests verify the pooled connection path (Database) works correctly

// alongside the direct connection path (db_path) tested above.

#[test]
fn pooled_connection_read_path_works() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    // Use database (pooled connection) instead of db_path (direct connection)
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.database.clone()));

    // Test a representative read operation
    let summary = repo
        .load_task_summary(&task_id)
        .expect("summary should load via pooled connection");
    assert_eq!(summary.id, task_id);
    assert!(!summary.created_at.is_empty());
}

#[test]
fn pooled_connection_write_path_works() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    // Use database (pooled connection) instead of db_path (direct connection)
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.database.clone()));

    // Test a representative write operation
    repo.set_task_status(&task_id, "running", false)
        .expect("set status should work via pooled connection");

    // Verify the write via a read
    let status = repo
        .load_task_status(&task_id)
        .expect("status should be readable");
    assert_eq!(status, Some("running".to_string()));
}

#[test]
fn list_task_log_runs_respects_limit() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task item exists");

    // Insert 5 command runs
    let ts = crate::config_load::now_ts();
    for i in 0..5 {
        conn.execute(
            "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES (?1, ?2, 'qa', 'echo test', '/tmp', 'default', 'agent', 0, '/tmp/out.log', '/tmp/err.log', '{}', '[]', NULL, NULL, 'unknown', ?3, ?3, 0)",
            params![format!("cr-limit-{}", i), item_id, ts],
        )
        .expect("insert command run");
    }

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let runs = repo
        .list_task_log_runs(&task_id, 3)
        .expect("should succeed");

    assert_eq!(runs.len(), 3, "should respect limit");
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

#[test]
fn find_latest_resumable_task_id_includes_restart_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='restart_pending' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set restart_pending");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
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
