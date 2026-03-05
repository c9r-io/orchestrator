use super::super::trait_def::TaskRepository;
use super::super::types::TaskRepositorySource;
use super::super::SqliteTaskRepository;
use super::fixtures::seed_task;
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
    assert_eq!(item_status, "qa_passed", "restart_pending should preserve item statuses");
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
    assert_eq!(completed_at, None, "restart_pending should clear completed_at");
}
