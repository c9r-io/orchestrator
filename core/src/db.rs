use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;

pub use crate::persistence::sqlite::SQLITE_BUSY_TIMEOUT_MS;

/// Counts returned after deleting all persisted state for one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectResetStats {
    /// Number of task rows removed.
    pub tasks: u64,
    /// Number of task-item rows removed.
    pub task_items: u64,
    /// Number of command-run rows removed.
    pub command_runs: u64,
    /// Number of event rows removed.
    pub events: u64,
    /// Number of ticket files removed from disk.
    pub tickets_cleaned: u64,
}

/// Minimal reference to a non-terminal task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReference {
    /// Stable task identifier.
    pub task_id: String,
    /// Current task status label.
    pub status: String,
}

/// Execution metrics materialized into the persistence layer.
#[derive(Debug, Clone)]
pub struct TaskExecutionMetric {
    /// Stable task identifier.
    pub task_id: String,
    /// Task status associated with the metric sample.
    pub status: String,
    /// Workflow cycle active when the sample was recorded.
    pub current_cycle: u32,
    /// Number of unresolved task items at sample time.
    pub unresolved_items: i64,
    /// Total number of task items known for the task.
    pub total_items: i64,
    /// Number of failed task items at sample time.
    pub failed_items: i64,
    /// Number of command runs recorded so far.
    pub command_runs: i64,
    /// RFC 3339 timestamp when the metric was captured.
    pub created_at: String,
}

/// Audit payload written for one control-plane authorization decision.
#[derive(Debug, Clone)]
pub struct ControlPlaneAuditRecord {
    /// Transport used by the incoming RPC, such as `tcp`.
    pub transport: String,
    /// Remote peer address when known.
    pub remote_addr: Option<String>,
    /// Fully qualified RPC name.
    pub rpc: String,
    /// Authenticated subject identifier when available.
    pub subject_id: Option<String>,
    /// Authentication outcome label.
    pub authn_result: String,
    /// Authorization outcome label.
    pub authz_result: String,
    /// Effective role assigned to the subject.
    pub role: Option<String>,
    /// Human-readable reason for denial or fallback behavior.
    pub reason: Option<String>,
    /// SHA-256 fingerprint of the presented client certificate.
    pub tls_fingerprint: Option<String>,
    /// Pipeline stage that rejected the request.
    pub rejection_stage: Option<String>,
    /// Traffic bucket selected for protection enforcement.
    pub traffic_class: Option<String>,
    /// Whether subject-scoped or global limits produced the decision.
    pub limit_scope: Option<String>,
    /// Final decision label written by the limiter.
    pub decision: Option<String>,
    /// Stable machine-readable reason code.
    pub reason_code: Option<String>,
    /// Executable path of the peer process (UDS only, forensic audit).
    pub peer_exe: Option<String>,
}

/// Audit payload for plugin-related authorization and execution decisions.
#[derive(Debug, Clone)]
pub struct PluginAuditRecord {
    /// Action: `crd_apply`, `plugin_execute`, or `hook_execute`.
    pub action: String,
    /// CRD kind that owns the plugin.
    pub crd_kind: String,
    /// Plugin name (None for hooks).
    pub plugin_name: Option<String>,
    /// Plugin type: `interceptor`, `transformer`, `cron`, or `hook`.
    pub plugin_type: Option<String>,
    /// Full command string.
    pub command: String,
    /// Caller identity (TLS subject_id or `uds:<pid>`).
    pub applied_by: Option<String>,
    /// Transport: `tcp` or `uds`.
    pub transport: Option<String>,
    /// Peer process ID (UDS only).
    pub peer_pid: Option<i32>,
    /// Verdict: `allowed`, `denied`, or `audit_warning`.
    pub result: String,
    /// Active policy mode: `deny`, `allowlist`, or `audit`.
    pub policy_mode: Option<String>,
    /// Name of the sandbox execution profile applied at runtime.
    pub sandbox_profile: Option<String>,
    /// Runtime policy verdict: `allowed`, `denied`, or `audit_warning`.
    pub policy_verdict: Option<String>,
}

/// Inserts one plugin-audit record into persistence.
pub fn insert_plugin_audit(db_path: &Path, record: &PluginAuditRecord) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO plugin_audit (
            created_at, action, crd_kind, plugin_name, plugin_type,
            command, applied_by, transport, peer_pid, result, policy_mode,
            sandbox_profile, policy_verdict
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            crate::config_load::now_ts(),
            record.action,
            record.crd_kind,
            record.plugin_name,
            record.plugin_type,
            record.command,
            record.applied_by,
            record.transport,
            record.peer_pid,
            record.result,
            record.policy_mode,
            record.sandbox_profile,
            record.policy_verdict,
        ],
    )?;
    Ok(())
}

/// Opens a SQLite connection using the orchestrator persistence defaults.
pub fn open_conn(db_path: &Path) -> Result<Connection> {
    crate::persistence::sqlite::open_conn(db_path)
}

/// Applies the standard busy timeout and pragma configuration to a connection.
pub fn configure_conn(conn: &Connection) -> Result<()> {
    crate::persistence::sqlite::configure_conn(conn)
}

/// Ensures the persistence schema exists and is migrated to the current version.
pub fn init_schema(db_path: &Path) -> Result<()> {
    crate::persistence::schema::PersistenceBootstrap::ensure_current(db_path)?;
    Ok(())
}

/// Counts running or pending tasks for one project workspace pair.
pub fn count_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE project_id = ?1
           AND workspace_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')",
        params![project_id, workspace_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Counts running or pending tasks for one project workflow pair.
pub fn count_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE project_id = ?1
           AND workflow_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')",
        params![project_id, workflow_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Lists the oldest non-terminal tasks for one project workspace pair.
pub fn list_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, status FROM tasks
         WHERE project_id = ?1
           AND workspace_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')
         ORDER BY created_at ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, workspace_id, limit], |row| {
        Ok(TaskReference {
            task_id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }
    Ok(tasks)
}

/// Lists the oldest non-terminal tasks for one project workflow pair.
pub fn list_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    let mut stmt = conn.prepare(
        "SELECT id, status FROM tasks
         WHERE project_id = ?1
           AND workflow_id = ?2
           AND status IN ('created', 'pending', 'running', 'restart_pending')
         ORDER BY created_at ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![project_id, workflow_id, limit], |row| {
        Ok(TaskReference {
            task_id: row.get(0)?,
            status: row.get(1)?,
        })
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }
    Ok(tasks)
}

/// Resets persisted runtime data using the active daemon state.
pub fn reset_db(
    state: &crate::state::InnerState,
    include_history: bool,
    include_config: bool,
) -> Result<()> {
    reset_db_by_path(&state.db_path, include_history, include_config)
}

/// Deletes persisted runtime data from a database path after guarding against active tasks.
pub fn reset_db_by_path(db_path: &Path, include_history: bool, include_config: bool) -> Result<()> {
    let conn = open_conn(db_path)?;

    let active_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE status IN ('running', 'restart_pending')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active_count > 0 {
        anyhow::bail!(
            "db reset blocked: {} active task(s) with status running/restart_pending. \
             Use `project reset <project> --force` for project-scoped cleanup instead.",
            active_count
        );
    }

    conn.execute("DELETE FROM events", [])?;
    let _ = conn.execute("DELETE FROM task_graph_snapshots", []);
    let _ = conn.execute("DELETE FROM task_graph_runs", []);
    conn.execute("DELETE FROM command_runs", [])?;
    conn.execute("DELETE FROM task_items", [])?;
    conn.execute("DELETE FROM tasks", [])?;
    conn.execute("DELETE FROM task_execution_metrics", [])?;
    let _ = conn.execute("DELETE FROM control_plane_audit", []);
    if include_config {
        conn.execute("DELETE FROM orchestrator_config_versions", [])?;
    } else if include_history {
        conn.execute(
            "DELETE FROM orchestrator_config_versions WHERE version < (SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions)",
            [],
        )?;
    }
    Ok(())
}

/// Inserts one control-plane audit record into persistence.
pub fn insert_control_plane_audit(db_path: &Path, record: &ControlPlaneAuditRecord) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO control_plane_audit (
            created_at, transport, remote_addr, rpc, subject_id, authn_result,
            authz_result, role, reason, tls_fingerprint, rejection_stage,
            traffic_class, limit_scope, decision, reason_code, peer_exe
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            crate::config_load::now_ts(),
            record.transport,
            record.remote_addr,
            record.rpc,
            record.subject_id,
            record.authn_result,
            record.authz_result,
            record.role,
            record.reason,
            record.tls_fingerprint,
            record.rejection_stage,
            record.traffic_class,
            record.limit_scope,
            record.decision,
            record.reason_code,
            record.peer_exe,
        ],
    )?;
    Ok(())
}

/// Inserts one task execution metric sample into persistence.
pub fn insert_task_execution_metric(db_path: &Path, metric: &TaskExecutionMetric) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO task_execution_metrics (task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
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

/// Deletes all persisted records and ticket files associated with one project.
pub fn reset_project_data(
    state: &crate::state::InnerState,
    project_id: &str,
) -> Result<ProjectResetStats> {
    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;

    let tasks: i64 = tx.query_row(
        "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )?;
    let task_items: i64 = tx.query_row(
        "SELECT COUNT(*) FROM task_items WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
        |row| row.get(0),
    )?;
    let command_runs: i64 = tx.query_row(
        "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
            SELECT ti.id FROM task_items ti
            JOIN tasks t ON t.id = ti.task_id
            WHERE t.project_id = ?1
        )",
        params![project_id],
        |row| row.get(0),
    )?;
    let events: i64 = tx.query_row(
        "SELECT COUNT(*) FROM events WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
        |row| row.get(0),
    )?;

    tx.execute(
        "DELETE FROM task_graph_snapshots WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_graph_runs WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id IN (
            SELECT ti.id FROM task_items ti
            JOIN tasks t ON t.id = ti.task_id
            WHERE t.project_id = ?1
        )",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM events WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_items WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM task_execution_metrics WHERE task_id IN (SELECT id FROM tasks WHERE project_id = ?1)",
        params![project_id],
    )?;
    tx.execute(
        "DELETE FROM tasks WHERE project_id = ?1",
        params![project_id],
    )?;

    tx.commit()?;

    Ok(ProjectResetStats {
        tasks: tasks.max(0) as u64,
        task_items: task_items.max(0) as u64,
        command_runs: command_runs.max(0) as u64,
        events: events.max(0) as u64,
        tickets_cleaned: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    fn tmp_db_path() -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create tmp dir");
        let db_path = dir.join("test.db");
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
        assert!(
            err.contains("db reset blocked"),
            "unexpected error: {}",
            err
        );
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
        let _debug = format!("{:?}", a);
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
        assert!(versions_after <= 1, "Expected <= 1, got {}", versions_after);
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
