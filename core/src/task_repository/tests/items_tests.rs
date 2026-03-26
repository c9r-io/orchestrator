use super::super::SqliteTaskRepository;
use super::super::command_run::NewCommandRun;
use super::super::trait_def::TaskRepository;
use super::super::types::TaskRepositorySource;
use super::fixtures::{get_item_id, seed_task};
use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::test_utils::TestState;
use rusqlite::params;

#[test]
fn insert_and_list_task_log_runs_work() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let run = NewCommandRun {
        id: "run-test-1".to_string(),
        task_item_id: item_id,
        phase: "qa".to_string(),
        command: "echo test".to_string(),
        command_template: None,
        cwd: state.data_dir.to_string_lossy().to_string(),
        workspace_id: "default".to_string(),
        agent_id: "echo".to_string(),
        exit_code: 0,
        stdout_path: "/tmp/stdout.log".to_string(),
        stderr_path: "/tmp/stderr.log".to_string(),
        started_at: now_ts(),
        ended_at: now_ts(),
        interrupted: 0,
        output_json: "{}".to_string(),
        artifacts_json: "[]".to_string(),
        confidence: None,
        quality_score: None,
        validation_status: "unknown".to_string(),
        session_id: None,
        machine_output_source: "stdout".to_string(),
        output_json_path: None,
            command_rule_index: None,
    };
    repo.insert_command_run(&run).expect("insert command run");

    let runs = repo
        .list_task_log_runs(&task_id, 10)
        .expect("list task log runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, "run-test-1");
    assert_eq!(runs[0].phase, "qa");
}

#[test]
fn insert_command_run_with_all_optional_fields() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let run = NewCommandRun {
        id: "run-with-optionals".to_string(),
        task_item_id: item_id,
        phase: "fix".to_string(),
        command: "cargo test".to_string(),
        command_template: None,
        cwd: state.data_dir.to_string_lossy().to_string(),
        workspace_id: "test-workspace".to_string(),
        agent_id: "claude".to_string(),
        exit_code: 0,
        stdout_path: "/tmp/out.log".to_string(),
        stderr_path: "/tmp/err.log".to_string(),
        started_at: now_ts(),
        ended_at: now_ts(),
        interrupted: 0,
        output_json: r#"{"result":"passed"}"#.to_string(),
        artifacts_json: r#"[{"path":"artifact.txt"}]"#.to_string(),
        confidence: Some(0.95),
        quality_score: Some(0.88),
        validation_status: "passed".to_string(),
        session_id: Some("session-123".to_string()),
        machine_output_source: "json".to_string(),
        output_json_path: Some("/tmp/output.json".to_string()),
            command_rule_index: None,
    };
    repo.insert_command_run(&run)
        .expect("insert command run with optionals");

    // Verify it was inserted correctly
    let runs = repo
        .list_task_log_runs(&task_id, 10)
        .expect("list task log runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, "run-with-optionals");
    assert_eq!(runs[0].phase, "fix");
}

#[test]
fn delete_task_and_collect_log_paths_cleans_data() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);
    let stdout_path = state.logs_dir.join("repo_test.stdout");
    let stderr_path = state.logs_dir.join("repo_test.stderr");
    std::fs::write(&stdout_path, "stdout").expect("seed stdout log");
    std::fs::write(&stderr_path, "stderr").expect("seed stderr log");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let run = NewCommandRun {
        id: "run-test-delete".to_string(),
        task_item_id: item_id,
        phase: "qa".to_string(),
        command: "echo test".to_string(),
        command_template: None,
        cwd: state.data_dir.to_string_lossy().to_string(),
        workspace_id: "default".to_string(),
        agent_id: "echo".to_string(),
        exit_code: 0,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        started_at: now_ts(),
        ended_at: now_ts(),
        interrupted: 0,
        output_json: "{}".to_string(),
        artifacts_json: "[]".to_string(),
        confidence: None,
        quality_score: None,
        validation_status: "unknown".to_string(),
        session_id: None,
        machine_output_source: "stdout".to_string(),
        output_json_path: None,
            command_rule_index: None,
    };
    repo.insert_command_run(&run).expect("insert command run");

    let paths = repo
        .delete_task_and_collect_log_paths(&task_id)
        .expect("delete task");
    assert_eq!(paths.len(), 2);

    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("count tasks");
    assert_eq!(remaining, 0);
}

#[test]
fn delete_task_and_collect_log_paths_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let err = repo
        .delete_task_and_collect_log_paths("nonexistent-task-id")
        .expect_err("should error for missing task");
    assert!(
        err.to_string().contains("task not found"),
        "error should mention 'task not found', got: {}",
        err
    );
}

#[test]
fn delete_task_and_collect_log_paths_returns_empty_when_no_command_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    // Don't insert any command runs
    let paths = repo
        .delete_task_and_collect_log_paths(&task_id)
        .expect("delete task");
    assert!(
        paths.is_empty(),
        "should have no log paths without command runs"
    );

    // Verify task was still deleted
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("count tasks");
    assert_eq!(remaining, 0);
}

#[test]
fn delete_task_and_collect_log_paths_filters_empty_paths() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let _conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Insert run with empty paths
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let run = NewCommandRun {
        id: "run-empty-paths".to_string(),
        task_item_id: item_id,
        phase: "qa".to_string(),
        command: "echo test".to_string(),
        command_template: None,
        cwd: state.data_dir.to_string_lossy().to_string(),
        workspace_id: "default".to_string(),
        agent_id: "echo".to_string(),
        exit_code: 0,
        stdout_path: "   ".to_string(), // whitespace only
        stderr_path: "".to_string(),    // empty
        started_at: now_ts(),
        ended_at: now_ts(),
        interrupted: 0,
        output_json: "{}".to_string(),
        artifacts_json: "[]".to_string(),
        confidence: None,
        quality_score: None,
        validation_status: "unknown".to_string(),
        session_id: None,
        machine_output_source: "stdout".to_string(),
        output_json_path: None,
            command_rule_index: None,
    };
    repo.insert_command_run(&run).expect("insert command run");

    let paths = repo
        .delete_task_and_collect_log_paths(&task_id)
        .expect("delete task");
    assert!(
        paths.is_empty(),
        "empty/whitespace paths should be filtered"
    );
}

#[test]
fn delete_task_and_collect_log_paths_deletes_cascaded_data() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id = get_item_id(&state, &task_id);

    // Insert command run and event
    let ts = now_ts();
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES ('cr-cascade', ?1, 'qa', 'test', '/tmp', 'default', 'agent', 0, '/tmp/out.log', '/tmp/err.log', '{}', '[]', NULL, NULL, 'unknown', ?2, ?2, 0)",
        params![item_id, ts],
    )
    .expect("insert command run");

    conn.execute(
        "INSERT INTO events (id, task_id, task_item_id, event_type, payload_json, created_at) VALUES (1, ?1, NULL, 'test_event', '{}', ?2)",
        params![task_id, ts],
    )
    .expect("insert event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.delete_task_and_collect_log_paths(&task_id)
        .expect("delete task");

    // Verify all related data is deleted
    let events_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("count events");
    assert_eq!(events_count, 0, "events should be deleted");

    let runs_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM command_runs WHERE id = 'cr-cascade'",
            [],
            |row| row.get(0),
        )
        .expect("count runs");
    assert_eq!(runs_count, 0, "command_runs should be deleted");

    let items_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_items WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("count items");
    assert_eq!(items_count, 0, "task_items should be deleted");
}

// ── update_task_item_status ────────────────────────────────────────

#[test]
fn update_task_item_status_changes_status() {
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

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.update_task_item_status(&item_id, "qa_passed")
        .expect("update should succeed");

    let status: String = conn
        .query_row(
            "SELECT status FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item");
    assert_eq!(status, "qa_passed");
}

#[test]
fn update_task_item_status_updates_updated_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let (item_id, old_updated): (String, String) = conn
        .query_row(
            "SELECT id, updated_at FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("task item exists");

    // Small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.update_task_item_status(&item_id, "unresolved")
        .expect("update should succeed");

    let new_updated: String = conn
        .query_row(
            "SELECT updated_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item");
    assert_ne!(old_updated, new_updated, "updated_at should change");
}

// ── update_task_item_pipeline_vars ────────────────────────────────

#[test]
fn update_task_item_pipeline_vars_persists_json() {
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

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let vars_json = r#"{"score":"70","approach":"regex"}"#;
    repo.update_task_item_pipeline_vars(&item_id, vars_json)
        .expect("update should succeed");

    let stored: Option<String> = conn
        .query_row(
            "SELECT dynamic_vars_json FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item");
    assert_eq!(stored.as_deref(), Some(vars_json));
}

#[test]
fn update_task_item_pipeline_vars_updates_updated_at() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let (item_id, old_updated): (String, String) = conn
        .query_row(
            "SELECT id, updated_at FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("task item exists");

    std::thread::sleep(std::time::Duration::from_millis(10));

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.update_task_item_pipeline_vars(&item_id, r#"{"k":"v"}"#)
        .expect("update should succeed");

    let new_updated: String = conn
        .query_row(
            "SELECT updated_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| row.get(0),
        )
        .expect("query item");
    assert_ne!(old_updated, new_updated, "updated_at should change");
}

#[test]
fn mark_task_item_running_sets_started_at() {
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

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.mark_task_item_running(&item_id)
        .expect("mark should succeed");

    let (status, started_at, completed_at): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, started_at, completed_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query item");
    assert_eq!(status, "running");
    assert!(started_at.is_some());
    assert!(completed_at.is_none());
}

#[test]
fn set_task_item_terminal_status_sets_completed_at() {
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

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    repo.set_task_item_terminal_status(&item_id, "qa_passed")
        .expect("terminal update should succeed");

    let (status, started_at, completed_at): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT status, started_at, completed_at FROM task_items WHERE id = ?1",
            params![item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("query item");
    assert_eq!(status, "qa_passed");
    assert!(started_at.is_some());
    assert!(completed_at.is_some());
}
