//! Admin facade over the orchestrator database: project queries, audit,
//! metrics and reset.
//!
//! The implementation moved to `orchestrator-persistence` (FR-130 Phase A) and
//! is re-exported here, so every existing `crate::db::*` and
//! `agent_orchestrator::db::*` path keeps resolving.
//!
//! What did not move is the pair of entry points that take the live daemon
//! state. `InnerState` is core's, and a persistence layer that has to name it
//! is a layer only in the directory listing. Both were already reading one
//! field, so both are wrappers here over a `_by_path` function below.

use anyhow::Result;

pub use orchestrator_persistence::db::*;

/// Resets persisted runtime data using the active daemon state.
pub fn reset_db(
    state: &crate::state::InnerState,
    include_history: bool,
    include_config: bool,
) -> Result<()> {
    reset_db_by_path(&state.db_path, include_history, include_config)
}

/// Deletes all persisted records and ticket files associated with one project.
pub fn reset_project_data(
    state: &crate::state::InnerState,
    project_id: &str,
) -> Result<ProjectResetStats> {
    reset_project_data_by_path(&state.db_path, project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use orchestrator_persistence::test_support::{
        count_non_terminal_tasks_by_workflow, count_non_terminal_tasks_by_workspace,
        list_non_terminal_tasks_by_workflow, list_non_terminal_tasks_by_workspace, open_conn,
    };
    use rusqlite::params;

    /// Returns a self-deleting temp directory and a database path inside it.
    ///
    /// The first element owns the cleanup and must stay bound for as long as
    /// `db_path` is used. It was a bare `PathBuf` until FR-159, which leaked
    /// 10 directories per run (2859 accumulated). The FR's own inventory could
    /// not see them: it enumerated directories holding `agent_orchestrator.db`,
    /// and this one holds `test.db`.
    fn tmp_db_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix("db-test-")
            .tempdir()
            .expect("create tmp dir");
        let db_path = dir.path().join("test.db");
        (dir, db_path)
    }

    // ── open_conn ──

    #[test]
    fn open_conn_creates_connection() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        // Verify foreign keys are enabled
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("pragma");
        assert_eq!(fk, 1);
    }

    // ── init_schema ──

    #[test]
    fn init_schema_creates_tables() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let tables: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .expect("prepare");
            stmt.query_map([], |row| row.get(0))
                .expect("query")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("collect")
        };

        assert!(tables.contains(&"tasks".to_string()));
        assert!(tables.contains(&"task_items".to_string()));
        assert!(tables.contains(&"command_runs".to_string()));
        assert!(tables.contains(&"events".to_string()));
        assert!(tables.contains(&"agent_sessions".to_string()));
        assert!(tables.contains(&"session_attachments".to_string()));
    }

    #[test]
    fn init_schema_is_idempotent() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("first init");
        init_schema(&db_path).expect("second init should succeed");
    }

    // ── non-terminal task reference counts ──

    #[test]
    fn count_non_terminal_tasks_by_workspace_returns_zero_initially() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let count =
            count_non_terminal_tasks_by_workspace(&conn, "default", "nonexistent").expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_non_terminal_tasks_by_workspace_counts_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/count_ws_test.md");
        std::fs::write(&qa_file, "# count ws test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 2");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let count = count_non_terminal_tasks_by_workspace(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "default",
        )
        .expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn count_non_terminal_tasks_by_workflow_returns_zero_initially() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let count =
            count_non_terminal_tasks_by_workflow(&conn, "default", "nonexistent").expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_non_terminal_tasks_by_workflow_counts_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/count_wf_test.md");
        std::fs::write(&qa_file, "# count wf test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let count =
            count_non_terminal_tasks_by_workflow(&conn, crate::config::DEFAULT_PROJECT_ID, "basic")
                .expect("count");
        assert_eq!(count, 1);
    }

    // ── insert_task_execution_metric ──

    #[test]
    fn insert_task_execution_metric_stores_row() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let metric = TaskExecutionMetric {
            task_id: "task-123".to_string(),
            status: "running".to_string(),
            current_cycle: 2,
            unresolved_items: 3,
            total_items: 10,
            failed_items: 1,
            command_runs: 5,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        insert_task_execution_metric(&db_path, &metric).expect("insert metric");

        let conn = open_conn(&db_path).expect("open sqlite");
        let (tid, status, cycle, unresolved, total, failed, runs): (
            String, String, i64, i64, i64, i64, i64,
        ) = conn
            .query_row(
                "SELECT task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs FROM task_execution_metrics WHERE task_id = ?1",
                params!["task-123"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )
            .expect("query metric");

        assert_eq!(tid, "task-123");
        assert_eq!(status, "running");
        assert_eq!(cycle, 2);
        assert_eq!(unresolved, 3);
        assert_eq!(total, 10);
        assert_eq!(failed, 1);
        assert_eq!(runs, 5);
    }

    // ── reset_db ──

    #[test]
    fn reset_db_clears_data() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/reset_test.md");
        std::fs::write(&qa_file, "# reset test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        // Confirm task exists
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .expect("count before");
        assert!(before > 0);
        drop(conn);

        reset_db(&state, false, false).expect("reset_db");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .expect("count after");
        assert_eq!(after, 0);
    }

    #[test]
    fn reset_db_with_config_clears_config() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // Confirm config versions exist in the active config history table.
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let versions_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestrator_config_versions",
                [],
                |row| row.get(0),
            )
            .expect("count config versions before");
        assert!(versions_before > 0);
        drop(conn);

        reset_db(&state, false, true).expect("reset_db with config");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let versions_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestrator_config_versions",
                [],
                |row| row.get(0),
            )
            .expect("count config versions after");
        assert_eq!(versions_after, 0);
    }

    #[test]
    fn reset_db_blocked_when_running_task_exists() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/guard_test.md");
        std::fs::write(&qa_file, "# guard test\n").expect("seed qa file");

        let task = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        // Simulate running status
        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status = 'running' WHERE id = ?1",
            params![task.id],
        )
        .expect("set task running");
        drop(conn);

        let result = reset_db(&state, false, false);
        assert!(result.is_err());
        let err = result.expect_err("should be blocked").to_string();
        assert!(err.contains("db reset blocked"), "unexpected error: {err}");
    }

    #[test]
    fn reset_db_blocked_when_restart_pending_task_exists() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/restart_guard.md");
        std::fs::write(&qa_file, "# restart guard\n").expect("seed qa file");

        let task = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status = 'restart_pending' WHERE id = ?1",
            params![task.id],
        )
        .expect("set task restart_pending");
        drop(conn);

        let result = reset_db(&state, false, false);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("should be blocked")
                .to_string()
                .contains("db reset blocked")
        );
    }

    // ── reset_project_data ──

    #[test]
    fn reset_project_data_returns_zero_stats_for_unknown_project() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let stats = reset_project_data(&state, "nonexistent-project").expect("reset project data");
        assert_eq!(
            stats,
            ProjectResetStats {
                tasks: 0,
                task_items: 0,
                command_runs: 0,
                events: 0,
                tickets_cleaned: 0,
            }
        );
    }

    // ── ProjectResetStats ──

    #[test]
    fn project_reset_stats_debug_and_eq() {
        let a = ProjectResetStats {
            tasks: 1,
            task_items: 2,
            command_runs: 3,
            events: 4,
            tickets_cleaned: 0,
        };
        let b = a;
        assert_eq!(a, b);
        // Debug should work
        let _debug = format!("{a:?}");
    }

    // ── list_non_terminal_tasks_by_workspace ──

    #[test]
    fn list_non_terminal_tasks_by_workspace_empty() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let tasks =
            list_non_terminal_tasks_by_workspace(&conn, "default", "ws1", 10).expect("list empty");
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_non_terminal_tasks_by_workspace_returns_matching() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/list_ws_test.md");
        std::fs::write(&qa_file, "# list ws test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 2");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let tasks = list_non_terminal_tasks_by_workspace(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "default",
            10,
        )
        .expect("list");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].status, "created");
        assert_eq!(tasks[1].status, "created");
    }

    #[test]
    fn list_non_terminal_tasks_by_workspace_respects_limit() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/limit_ws_test.md");
        std::fs::write(&qa_file, "# limit ws test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 2");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 3");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let tasks = list_non_terminal_tasks_by_workspace(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "default",
            2,
        )
        .expect("list limited");
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn list_non_terminal_tasks_by_workspace_excludes_terminal() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/terminal_ws_test.md");
        std::fs::write(&qa_file, "# terminal ws test\n").expect("seed qa file");

        let task = create_task_impl(&state, CreateTaskPayload::default()).expect("task");

        // Mark as completed (terminal)
        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status = 'completed' WHERE id = ?1",
            params![task.id],
        )
        .expect("set task completed");

        let tasks = list_non_terminal_tasks_by_workspace(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "default",
            10,
        )
        .expect("list");
        assert!(tasks.is_empty());
    }

    // ── list_non_terminal_tasks_by_workflow ──

    #[test]
    fn list_non_terminal_tasks_by_workflow_empty() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let tasks =
            list_non_terminal_tasks_by_workflow(&conn, "default", "wf1", 10).expect("list empty");
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_non_terminal_tasks_by_workflow_returns_matching() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/list_wf_test.md");
        std::fs::write(&qa_file, "# list wf test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let tasks = list_non_terminal_tasks_by_workflow(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "basic",
            10,
        )
        .expect("list");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "created");
    }

    #[test]
    fn list_non_terminal_tasks_by_workflow_respects_limit() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/limit_wf_test.md");
        std::fs::write(&qa_file, "# limit wf test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 2");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 3");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let tasks = list_non_terminal_tasks_by_workflow(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "basic",
            1,
        )
        .expect("list limited");
        assert_eq!(tasks.len(), 1);
    }

    // ── insert_control_plane_audit ──

    #[test]
    fn insert_control_plane_audit_stores_row() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let record = ControlPlaneAuditRecord {
            request_id: None,
            transport: "grpc".to_string(),
            remote_addr: Some("127.0.0.1:5000".to_string()),
            rpc: "CreateTask".to_string(),
            subject_id: Some("user-1".to_string()),
            authn_result: "ok".to_string(),
            authz_result: "allowed".to_string(),
            role: Some("admin".to_string()),
            reason: Some("normal access".to_string()),
            tls_fingerprint: None,
            rejection_stage: None,
            traffic_class: None,
            limit_scope: None,
            decision: None,
            reason_code: None,
            peer_exe: None,
        };
        insert_control_plane_audit(&db_path, &record).expect("insert audit");

        let conn = open_conn(&db_path).expect("open sqlite");
        let (transport, rpc, authn, authz): (String, String, String, String) = conn
            .query_row(
                "SELECT transport, rpc, authn_result, authz_result FROM control_plane_audit LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("query audit");
        assert_eq!(transport, "grpc");
        assert_eq!(rpc, "CreateTask");
        assert_eq!(authn, "ok");
        assert_eq!(authz, "allowed");
    }

    #[test]
    fn insert_control_plane_audit_with_none_fields() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let record = ControlPlaneAuditRecord {
            request_id: None,
            transport: "uds".to_string(),
            remote_addr: None,
            rpc: "ListTasks".to_string(),
            subject_id: None,
            authn_result: "skipped".to_string(),
            authz_result: "skipped".to_string(),
            role: None,
            reason: None,
            tls_fingerprint: None,
            rejection_stage: None,
            traffic_class: None,
            limit_scope: None,
            decision: None,
            reason_code: None,
            peer_exe: None,
        };
        insert_control_plane_audit(&db_path, &record).expect("insert audit with nones");

        let conn = open_conn(&db_path).expect("open sqlite");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM control_plane_audit", [], |row| {
                row.get(0)
            })
            .expect("count audit");
        assert_eq!(count, 1);
    }

    // ── reset_db include_history branch ──

    #[test]
    fn reset_db_with_history_keeps_latest_config_version() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // Confirm config versions exist
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let versions_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestrator_config_versions",
                [],
                |row| row.get(0),
            )
            .expect("count config versions before");
        assert!(versions_before > 0);
        drop(conn);

        // Reset with include_history=true, include_config=false
        // Should keep only the latest config version
        reset_db(&state, true, false).expect("reset_db with history");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let versions_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orchestrator_config_versions",
                [],
                |row| row.get(0),
            )
            .expect("count config versions after");
        // Should keep at most 1 (the latest)
        assert!(versions_after <= 1, "Expected <= 1, got {versions_after}");
        // Tasks should be cleared
        let tasks: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
            .expect("count tasks");
        assert_eq!(tasks, 0);
    }

    // ── reset_project_data with actual data ──

    #[test]
    fn reset_project_data_clears_project_data_and_returns_stats() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/proj_reset_test.md");
        std::fs::write(&qa_file, "# proj reset test\n").expect("seed qa file");

        let task = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        // Verify task exists
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
                params![crate::config::DEFAULT_PROJECT_ID],
                |row| row.get(0),
            )
            .expect("count tasks");
        assert!(task_count > 0);

        // Insert an event for the task
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![task.id, "test", "{}", crate::config_load::now_ts()],
        )
        .expect("insert event");
        drop(conn);

        let stats =
            reset_project_data(&state, crate::config::DEFAULT_PROJECT_ID).expect("reset project");
        assert!(stats.tasks > 0, "expected tasks > 0, got {}", stats.tasks);
        assert_eq!(stats.tickets_cleaned, 0); // hardcoded to 0

        // Verify data is cleared
        let conn = open_conn(&state.db_path).expect("open sqlite after reset");
        let task_count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
                params![crate::config::DEFAULT_PROJECT_ID],
                |row| row.get(0),
            )
            .expect("count tasks after");
        assert_eq!(task_count_after, 0);

        let event_count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .expect("count events after");
        assert_eq!(event_count_after, 0);
    }

    // ── count excludes terminal statuses ──

    #[test]
    fn count_non_terminal_tasks_by_workspace_excludes_completed() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/terminal_count_test.md");
        std::fs::write(&qa_file, "# terminal count test\n").expect("seed qa file");

        let task = create_task_impl(&state, CreateTaskPayload::default()).expect("task");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status = 'completed' WHERE id = ?1",
            params![task.id],
        )
        .expect("set completed");

        let count = count_non_terminal_tasks_by_workspace(
            &conn,
            crate::config::DEFAULT_PROJECT_ID,
            "default",
        )
        .expect("count");
        assert_eq!(count, 0);
    }
}
