use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Duration;

pub const SQLITE_BUSY_TIMEOUT_MS: u64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectResetStats {
    pub tasks: u64,
    pub task_items: u64,
    pub command_runs: u64,
    pub events: u64,
}

#[derive(Debug, Clone)]
pub struct TaskExecutionMetric {
    pub task_id: String,
    pub status: String,
    pub current_cycle: u32,
    pub unresolved_items: i64,
    pub total_items: i64,
    pub failed_items: i64,
    pub command_runs: i64,
    pub created_at: String,
}

pub fn open_conn(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).context("failed to open sqlite db")?;
    configure_conn(&conn)?;
    Ok(conn)
}

pub fn configure_conn(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))
        .context("failed to set sqlite busy timeout")?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        "#,
    )
    .context("failed to configure sqlite pragmas")?;
    Ok(())
}

pub fn ensure_column(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .with_context(|| format!("failed to read schema for {}", table))?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !cols.iter().any(|c| c == column) {
        conn.execute(ddl, [])
            .with_context(|| format!("failed to add column {}.{}", table, column))?;
    }
    Ok(())
}

pub fn init_schema(db_path: &Path) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        "#,
    )
    .context("failed to configure sqlite wal mode")?;
    let applied = crate::migration::run_pending(&conn, &crate::migration::all_migrations())?;
    if applied > 0 {
        tracing::info!(applied, "schema migrations applied");
    }
    Ok(())
}

pub fn count_tasks_by_workspace(conn: &Connection, workspace_id: &str) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE workspace_id = ?1",
        params![workspace_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn count_tasks_by_workflow(conn: &Connection, workflow_id: &str) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE workflow_id = ?1",
        params![workflow_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn reset_db(
    state: &crate::state::InnerState,
    include_history: bool,
    include_config: bool,
) -> Result<()> {
    reset_db_by_path(&state.db_path, include_history, include_config)
}

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
             Use `qa project reset <project> --force` for project-scoped cleanup instead.",
            active_count
        );
    }

    conn.execute("DELETE FROM events", [])?;
    conn.execute("DELETE FROM command_runs", [])?;
    conn.execute("DELETE FROM task_items", [])?;
    conn.execute("DELETE FROM tasks", [])?;
    conn.execute("DELETE FROM task_execution_metrics", [])?;
    if include_config {
        conn.execute("DELETE FROM orchestrator_config", [])?;
        conn.execute("DELETE FROM orchestrator_config_versions", [])?;
    } else if include_history {
        conn.execute(
            "DELETE FROM orchestrator_config_versions WHERE version < (SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions)",
            [],
        )?;
    }
    Ok(())
}

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
        assert!(tables.contains(&"orchestrator_config".to_string()));
        assert!(tables.contains(&"agent_sessions".to_string()));
        assert!(tables.contains(&"session_attachments".to_string()));
    }

    #[test]
    fn init_schema_is_idempotent() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("first init");
        init_schema(&db_path).expect("second init should succeed");
    }

    // ── ensure_column ──

    #[test]
    fn ensure_column_adds_missing_column() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        // Add a new column that doesn't exist yet
        ensure_column(
            &conn,
            "tasks",
            "test_col_xyz",
            "ALTER TABLE tasks ADD COLUMN test_col_xyz TEXT",
        )
        .expect("ensure_column add");

        // Verify column exists
        let mut stmt = conn.prepare("PRAGMA table_info(tasks)").expect("prepare");
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect");
        assert!(cols.contains(&"test_col_xyz".to_string()));
    }

    #[test]
    fn ensure_column_noop_if_exists() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        // "status" already exists on tasks; should be a no-op
        ensure_column(
            &conn,
            "tasks",
            "status",
            "ALTER TABLE tasks ADD COLUMN status TEXT",
        )
        .expect("ensure_column noop");
    }

    // ── count_tasks_by_workspace / count_tasks_by_workflow ──

    #[test]
    fn count_tasks_by_workspace_returns_zero_initially() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let count = count_tasks_by_workspace(&conn, "nonexistent").expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_tasks_by_workspace_counts_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/count_ws_test.md");
        std::fs::write(&qa_file, "# count ws test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("task 2");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let count = count_tasks_by_workspace(&conn, "default").expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn count_tasks_by_workflow_returns_zero_initially() {
        let (_dir, db_path) = tmp_db_path();
        init_schema(&db_path).expect("init_schema");

        let conn = open_conn(&db_path).expect("open_conn");
        let count = count_tasks_by_workflow(&conn, "nonexistent").expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_tasks_by_workflow_counts_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/count_wf_test.md");
        std::fs::write(&qa_file, "# count wf test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("task 1");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let count = count_tasks_by_workflow(&conn, "basic").expect("count");
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
            .app_root
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

        // Confirm config exists
        let conn = open_conn(&state.db_path).expect("open sqlite");
        let config_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM orchestrator_config", [], |row| {
                row.get(0)
            })
            .expect("count config before");
        assert!(config_before > 0);
        drop(conn);

        reset_db(&state, false, true).expect("reset_db with config");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let config_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM orchestrator_config", [], |row| {
                row.get(0)
            })
            .expect("count config after");
        assert_eq!(config_after, 0);
    }

    #[test]
    fn reset_db_blocked_when_running_task_exists() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .app_root
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
            .app_root
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
        assert!(result
            .expect_err("should be blocked")
            .to_string()
            .contains("db reset blocked"));
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
        };
        let b = a;
        assert_eq!(a, b);
        // Debug should work
        let _debug = format!("{:?}", a);
    }
}
