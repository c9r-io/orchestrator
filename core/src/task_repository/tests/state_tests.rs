use super::super::SqliteTaskRepository;
use super::super::state;
use super::super::trait_def::TaskStateRepository;
use super::super::types::TaskRepositorySource;
use super::fixtures::{get_item_id, seed_task};
use crate::db::open_conn;
use crate::test_utils::TestState;
use rusqlite::params;

#[test]
fn prepare_task_for_start_batch_resets_unresolved_items() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='failed' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task failed");
    conn.execute(
        "UPDATE task_items SET status='unresolved', fix_required=1, fixed=1, last_error='x' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("mark unresolved");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.prepare_task_for_start_batch(&task_id)
        .expect("prepare should succeed");

    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("task status query");
    assert_eq!(task_status, "running");

    let reset_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_items WHERE task_id=?1 AND status='pending' AND fix_required=0 AND fixed=0",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task_items query");
    assert!(reset_count >= 1);
}

#[test]
fn prepare_task_for_start_batch_resets_unresolved_items_from_paused() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task paused");
    conn.execute(
        "UPDATE task_items SET status='unresolved', fix_required=1, fixed=1, last_error='x' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("mark unresolved");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.prepare_task_for_start_batch(&task_id)
        .expect("prepare should succeed");

    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("task status query");
    assert_eq!(task_status, "running");

    let reset_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_items WHERE task_id=?1 AND status='pending' AND fix_required=0 AND fixed=0",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task_items query");
    assert!(
        reset_count >= 1,
        "unresolved items should be reset to pending on resume from paused"
    );
}

#[test]
fn prepare_task_for_start_batch_rejects_already_running() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let err = repo
        .prepare_task_for_start_batch(&task_id)
        .expect_err("should reject already-running task");
    assert!(
        err.to_string().contains("already running"),
        "error should mention 'already running', got: {}",
        err
    );
}

#[test]
fn prepare_task_for_start_batch_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let err = repo
        .prepare_task_for_start_batch("nonexistent-task-id")
        .expect_err("should error for missing task");
    assert!(
        err.to_string().contains("task not found"),
        "error should mention 'task not found', got: {}",
        err
    );
}

#[test]
fn prepare_task_for_start_batch_sets_started_at_when_previously_null() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Verify started_at is initially null
    let started_at_before: Option<String> = conn
        .query_row(
            "SELECT started_at FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert!(
        started_at_before.is_none(),
        "started_at should be null before batch start"
    );

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.prepare_task_for_start_batch(&task_id)
        .expect("prepare should succeed");

    let started_at_after: Option<String> = conn
        .query_row(
            "SELECT started_at FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert!(
        started_at_after.is_some(),
        "started_at should be set after batch start"
    );
}

#[test]
fn prepare_task_for_start_batch_works_for_pending_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    // Task is created with 'pending' status by default

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.prepare_task_for_start_batch(&task_id)
        .expect("prepare should succeed for pending task");

    let conn = open_conn(&state.db_path).expect("open sqlite");
    let status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query status");
    assert_eq!(status, "running");
}

// ── set_task_status ────────────────────────────────────────────────

#[test]
fn set_task_status_with_completed_sets_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    repo.set_task_status(&task_id, "completed", true)
        .expect("set status should succeed");

    let conn = open_conn(&state.db_path).expect("open sqlite");
    let (status, completed_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(status, "completed");
    assert!(
        completed_at.is_some(),
        "completed_at should be set when set_completed is true"
    );
}

#[test]
fn set_task_status_running_clears_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    // First set completed_at to a value
    conn.execute(
        "UPDATE tasks SET status='completed', completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "running", false)
        .expect("set status should succeed");

    let (status, completed_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(status, "running");
    assert_eq!(
        completed_at, None,
        "completed_at should be cleared for running status"
    );
}

#[test]
fn set_task_status_pending_clears_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='completed', completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "pending", false)
        .expect("set status should succeed");

    let completed_at: Option<String> = conn
        .query_row(
            "SELECT completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task");
    assert_eq!(completed_at, None);
}

#[test]
fn set_task_status_paused_clears_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='completed', completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "paused", false)
        .expect("set status should succeed");

    let completed_at: Option<String> = conn
        .query_row(
            "SELECT completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task");
    assert_eq!(completed_at, None);
}

#[test]
fn set_task_status_interrupted_clears_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='completed', completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "interrupted", false)
        .expect("set status should succeed");

    let completed_at: Option<String> = conn
        .query_row(
            "SELECT completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task");
    assert_eq!(completed_at, None);
}

#[test]
fn set_task_status_failed_without_set_completed_preserves_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed_at");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    // "failed" is not in the list that clears completed_at, and set_completed is false
    repo.set_task_status(&task_id, "failed", false)
        .expect("set status should succeed");

    let (status, completed_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(status, "failed");
    assert_eq!(
        completed_at,
        Some("2026-01-01T00:00:00Z".to_string()),
        "completed_at should be preserved for non-clearing status without set_completed"
    );
}

// ── update_task_cycle_state ────────────────────────────────────────

#[test]
fn update_task_cycle_state_sets_cycle_and_init_done() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    repo.update_task_cycle_state(&task_id, 3, true)
        .expect("update should succeed");

    let conn = open_conn(&state.db_path).expect("open sqlite");
    let (cycle, init_done): (i64, i64) = conn
        .query_row(
            "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(cycle, 3);
    assert_eq!(init_done, 1);
}

#[test]
fn update_task_cycle_state_can_clear_init_done() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    // First set init_done to true
    repo.update_task_cycle_state(&task_id, 1, true)
        .expect("update should succeed");
    // Then clear it
    repo.update_task_cycle_state(&task_id, 2, false)
        .expect("update should succeed");

    let conn = open_conn(&state.db_path).expect("open sqlite");
    let (cycle, init_done): (i64, i64) = conn
        .query_row(
            "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(cycle, 2);
    assert_eq!(init_done, 0);
}

// ── restart_pending ────────────────────────────────────────

#[test]
fn prepare_task_restart_pending_preserves_items() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Set task to restart_pending and items to various non-pending states
    conn.execute(
        "UPDATE tasks SET status='restart_pending' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set restart_pending");
    conn.execute(
        "UPDATE task_items SET status='qa_passed' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("mark items as qa_passed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.prepare_task_for_start_batch(&task_id)
        .expect("prepare should succeed");

    // Task status should be running
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "running");

    // Items should NOT have been reset (still qa_passed, not pending)
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE task_id = ?1 LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(
        item_status, "qa_passed",
        "restart_pending should preserve item statuses"
    );
}

#[test]
fn set_task_status_restart_pending_clears_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed_at");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "restart_pending", false)
        .expect("set status should succeed");

    let (status, completed_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(status, "restart_pending");
    assert_eq!(
        completed_at, None,
        "restart_pending should clear completed_at"
    );
}

// ── recover_orphaned_running_items ────────────────────────────────

#[test]
fn recover_orphaned_running_items_resets_items_and_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task and item to running
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let recovered = state::recover_orphaned_running_items(&conn).expect("recover should succeed");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].0, task_id);
    assert_eq!(recovered[0].1, vec![item_id.clone()]);

    // Verify item is now pending
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(item_status, "pending");

    // Verify started_at is cleared
    let started_at: Option<String> = conn
        .query_row(
            "SELECT started_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert!(started_at.is_none(), "started_at should be cleared");

    // Verify task is now restart_pending
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "restart_pending");
}

#[test]
fn recover_orphaned_running_items_returns_empty_when_no_orphans() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let recovered = state::recover_orphaned_running_items(&conn).expect("recover should succeed");
    assert!(recovered.is_empty());
}

#[test]
fn recover_orphaned_running_items_does_not_affect_terminal_items() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set item to a terminal status
    conn.execute(
        "UPDATE task_items SET status='qa_passed' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item qa_passed");

    let recovered = state::recover_orphaned_running_items(&conn).expect("recover should succeed");
    assert!(recovered.is_empty());

    // Verify item is still qa_passed
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(item_status, "qa_passed");
}

#[test]
fn recover_orphaned_running_items_for_task_only_affects_target_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Create a second task
    let qa_file2 = state
        .data_dir
        .join("workspace/default/docs/qa/repo_test2.md");
    std::fs::write(&qa_file2, "# repo test 2\n").expect("seed second qa file");
    let created2 = crate::task_ops::create_task_impl(
        &state,
        crate::dto::CreateTaskPayload {
            name: Some("repo-test-2".to_string()),
            goal: Some("repo-test-2-goal".to_string()),
            ..Default::default()
        },
    )
    .expect("create second task");
    let task_id2 = created2.id;
    let item_id2 = get_item_id(&state, &task_id2);

    // Set both tasks and items to running
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id IN (?1, ?2)",
        params![task_id.clone(), task_id2.clone()],
    )
    .expect("mark tasks running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id IN (?1, ?2)",
        params![item_id.clone(), item_id2.clone()],
    )
    .expect("mark items running");

    // Recover only the first task
    let recovered =
        state::recover_orphaned_running_items_for_task(&conn, &task_id).expect("recover");
    assert_eq!(recovered, vec![item_id.clone()]);

    // First task item should be pending
    let status1: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("item1 status");
    assert_eq!(status1, "pending");

    // Second task item should STILL be running
    let status2: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id2],
            |row| row.get(0),
        )
        .expect("item2 status");
    assert_eq!(status2, "running");
}

#[test]
fn recover_stalled_running_items_respects_threshold() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task and item to running with a started_at in the past (2 hours ago)
    let old_ts = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at=?2 WHERE id = ?1",
        params![item_id.clone(), old_ts],
    )
    .expect("mark item running with old started_at");

    // Threshold of 3 hours → should NOT recover (item is only 2h old)
    let no_exclude = std::collections::HashSet::new();
    let recovered =
        state::recover_stalled_running_items(&conn, 3 * 3600, &no_exclude).expect("recover");
    assert!(
        recovered.is_empty(),
        "should not recover items within threshold"
    );

    // Verify item is still running
    let status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id.clone()],
            |row| row.get(0),
        )
        .expect("item status");
    assert_eq!(status, "running");

    // Threshold of 1 hour → SHOULD recover (item is 2h old)
    let recovered =
        state::recover_stalled_running_items(&conn, 3600, &no_exclude).expect("recover");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].0, task_id);

    // Verify item is now pending
    let status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("item status");
    assert_eq!(status, "pending");
}

#[test]
fn recover_stalled_running_items_skips_excluded_tasks() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task and item to running with a started_at 2 hours ago
    let old_ts = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at=?2 WHERE id = ?1",
        params![item_id.clone(), old_ts],
    )
    .expect("mark item running with old started_at");

    // Exclude this task (simulating an active worker)
    let exclude = std::collections::HashSet::from([task_id.clone()]);
    let recovered = state::recover_stalled_running_items(&conn, 3600, &exclude).expect("recover");
    assert!(
        recovered.is_empty(),
        "excluded task should not be recovered"
    );

    // Verify item is STILL running (not reset)
    let status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id.clone()],
            |row| row.get(0),
        )
        .expect("item status");
    assert_eq!(
        status, "running",
        "item should remain running when task is excluded"
    );

    // Verify task is STILL running (not set to restart_pending)
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task status");
    assert_eq!(
        task_status, "running",
        "excluded task should remain running"
    );
}

// ── recover_orphaned: paused task skipped in return value ──────────

#[test]
fn recover_orphaned_running_items_skips_paused_task_in_return() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task to paused but leave item in running state
    // (simulates the state after shutdown_running_tasks paused the task
    // but items were not reset before a crash)
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task paused");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let recovered = state::recover_orphaned_running_items(&conn).expect("recover should succeed");

    // Return value should be EMPTY — paused task is not returned for worker notification
    assert!(
        recovered.is_empty(),
        "paused task should not appear in recovered list, got: {:?}",
        recovered
    );

    // But the item should still be reset to pending (ready for later resume)
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(
        item_status, "pending",
        "item should be reset to pending even for paused task"
    );

    // Task should remain paused (NOT changed to restart_pending)
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(
        task_status, "paused",
        "paused task should NOT be changed to restart_pending"
    );
}

// ── pause_all_running_tasks_and_items ──────────────────────────────

#[test]
fn pause_all_running_tasks_and_items_pauses_tasks_and_resets_items() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task and item to running
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let count =
        state::pause_all_running_tasks_and_items(&conn).expect("blanket pause should succeed");
    assert_eq!(count, 1, "should reset 1 running item");

    // Task should be paused
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "paused");

    // Item should be pending with cleared started_at
    let (item_status, started_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, started_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query item");
    assert_eq!(item_status, "pending");
    assert!(started_at.is_none(), "started_at should be cleared");
}

#[test]
fn pause_all_running_does_not_affect_paused_tasks() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Task is paused, item is pending (normal state)
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task paused");

    let count =
        state::pause_all_running_tasks_and_items(&conn).expect("blanket pause should succeed");
    assert_eq!(count, 0, "should not reset any items");

    // Task should remain paused
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "paused");

    // Item should remain pending
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(item_status, "pending");
}

// ── reset_unresolved_items ───────────────────────────────────────

#[test]
fn reset_unresolved_items_resets_unresolved_to_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Mark item as unresolved with various fields set
    conn.execute(
        "UPDATE task_items SET status='unresolved', fix_required=1, fixed=1, last_error='some error', ticket_files_json='[\"a.rs\"]', ticket_content_json='[\"content\"]' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item unresolved");

    state::reset_unresolved_items(&conn, &task_id).expect("reset should succeed");

    // Item should be reset to pending with all fields cleared
    let (status, fix_required, fixed, last_error, ticket_files, ticket_content): (
        String,
        i64,
        i64,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT status, fix_required, fixed, last_error, ticket_files_json, ticket_content_json FROM task_items WHERE id = ?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .expect("query item");
    assert_eq!(status, "pending");
    assert_eq!(fix_required, 0);
    assert_eq!(fixed, 0);
    assert_eq!(last_error, "");
    assert_eq!(ticket_files, "[]");
    assert_eq!(ticket_content, "[]");
}

#[test]
fn reset_unresolved_items_resets_cycle_counter_when_pending_items_exist() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task cycle state
    conn.execute(
        "UPDATE tasks SET current_cycle=5, init_done=1 WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set cycle state");

    // Mark item as unresolved — after reset it becomes pending
    conn.execute(
        "UPDATE task_items SET status='unresolved' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item unresolved");

    state::reset_unresolved_items(&conn, &task_id).expect("reset should succeed");

    // Cycle counter should be reset because there are now pending items
    let (cycle, init_done): (i64, i64) = conn
        .query_row(
            "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(cycle, 0, "current_cycle should be reset to 0");
    assert_eq!(init_done, 0, "init_done should be reset to 0");
}

#[test]
fn reset_unresolved_items_no_op_when_no_unresolved() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Set task cycle state — items are pending by default (not unresolved)
    conn.execute(
        "UPDATE tasks SET current_cycle=5, init_done=1 WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set cycle state");

    // Items are already pending, so there are no unresolved items to reset,
    // but there ARE pending items, so the cycle counter should still be reset.
    state::reset_unresolved_items(&conn, &task_id).expect("reset should succeed");

    let (cycle, init_done): (i64, i64) = conn
        .query_row(
            "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(
        cycle, 0,
        "cycle should still be reset because pending items exist"
    );
    assert_eq!(init_done, 0);
}

#[test]
fn reset_unresolved_items_skips_cycle_reset_when_no_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task cycle state
    conn.execute(
        "UPDATE tasks SET current_cycle=5, init_done=1 WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set cycle state");

    // Mark item as qa_passed (not unresolved, not pending)
    conn.execute(
        "UPDATE task_items SET status='qa_passed' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item qa_passed");

    state::reset_unresolved_items(&conn, &task_id).expect("reset should succeed");

    // Cycle counter should NOT be reset because there are no pending items
    let (cycle, init_done): (i64, i64) = conn
        .query_row(
            "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(cycle, 5, "current_cycle should remain unchanged");
    assert_eq!(init_done, 1, "init_done should remain unchanged");
}

// ── pause_restart_pending_tasks_and_items ─────────────────────────

#[test]
fn pause_restart_pending_tasks_and_items_pauses_and_resets() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task to restart_pending with a running item
    conn.execute(
        "UPDATE tasks SET status='restart_pending' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task restart_pending");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let count = state::pause_restart_pending_tasks_and_items(&conn)
        .expect("pause restart_pending should succeed");
    assert_eq!(count, 1, "should reset 1 running item");

    // Task should now be paused
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "paused");

    // Item should be pending with cleared started_at
    let (item_status, started_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, started_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query item");
    assert_eq!(item_status, "pending");
    assert!(started_at.is_none(), "started_at should be cleared");
}

#[test]
fn pause_restart_pending_does_not_affect_running_tasks() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Set task to running (not restart_pending) with a running item
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task running");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let count = state::pause_restart_pending_tasks_and_items(&conn).expect("pause should succeed");
    assert_eq!(count, 0, "should not reset items for running tasks");

    // Task should remain running
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "running");

    // Item should remain running
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(item_status, "running");
}

#[test]
fn pause_restart_pending_returns_zero_when_no_restart_pending_tasks() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Default task is pending — no restart_pending tasks exist
    let count = state::pause_restart_pending_tasks_and_items(&conn).expect("pause should succeed");
    assert_eq!(count, 0);
}

// ── recover_orphaned_running_items_for_task: edge cases ──────────

#[test]
fn recover_orphaned_running_items_for_task_returns_empty_when_no_running_items() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Items are pending by default — no running items
    let recovered =
        state::recover_orphaned_running_items_for_task(&conn, &task_id).expect("recover");
    assert!(recovered.is_empty());
}

#[test]
fn recover_orphaned_running_items_for_task_skips_non_running_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Task is paused but item is running (edge case)
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("mark task paused");
    conn.execute(
        "UPDATE task_items SET status='running', started_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("mark item running");

    let recovered =
        state::recover_orphaned_running_items_for_task(&conn, &task_id).expect("recover");
    // Items are still recovered (reset to pending)
    assert_eq!(recovered, vec![item_id.clone()]);

    // Item should be pending
    let item_status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item status");
    assert_eq!(item_status, "pending");

    // Task should remain paused (UPDATE WHERE status='running' does not match)
    let task_status: String = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query task status");
    assert_eq!(task_status, "paused");
}

// ── set_task_status: started_at COALESCE behavior ────────────────

#[test]
fn set_task_status_running_sets_started_at_when_null() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // started_at should be null initially
    let started_at_before: Option<String> = conn
        .query_row(
            "SELECT started_at FROM tasks WHERE id = ?1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert!(started_at_before.is_none());

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "running", false)
        .expect("set status");

    let started_at_after: Option<String> = conn
        .query_row(
            "SELECT started_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert!(
        started_at_after.is_some(),
        "started_at should be set when transitioning to running"
    );
}

#[test]
fn set_task_status_running_preserves_existing_started_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let original_ts = "2025-06-15T10:00:00Z";
    conn.execute(
        "UPDATE tasks SET started_at = ?2 WHERE id = ?1",
        params![task_id.clone(), original_ts],
    )
    .expect("set started_at");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "running", false)
        .expect("set status");

    let started_at: Option<String> = conn
        .query_row(
            "SELECT started_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("query started_at");
    assert_eq!(
        started_at,
        Some(original_ts.to_string()),
        "started_at should be preserved via COALESCE"
    );
}

#[test]
fn set_task_status_completed_with_set_completed_sets_started_at_when_null() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "completed", true)
        .expect("set status");

    let (started_at, completed_at): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT started_at, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert!(
        started_at.is_some(),
        "started_at should be set via COALESCE"
    );
    assert!(completed_at.is_some(), "completed_at should be set");
}

#[test]
fn set_task_status_unknown_status_does_not_clear_completed_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET completed_at='2026-01-01T00:00:00Z' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set completed_at");

    // An arbitrary/unknown status hits the else branch
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_status(&task_id, "cancelled", false)
        .expect("set status");

    let (status, completed_at): (String, Option<String>) = conn
        .query_row(
            "SELECT status, completed_at FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query task");
    assert_eq!(status, "cancelled");
    assert_eq!(
        completed_at,
        Some("2026-01-01T00:00:00Z".to_string()),
        "else branch should preserve completed_at"
    );
}
