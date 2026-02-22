use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

pub fn open_conn(db_path: &Path) -> Result<Connection> {
    Connection::open(db_path).context("failed to open sqlite db")
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
        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            goal TEXT NOT NULL,
            target_files_json TEXT NOT NULL,
            mode TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            workflow_id TEXT NOT NULL,
            workspace_root TEXT NOT NULL,
            qa_targets_json TEXT NOT NULL,
            ticket_dir TEXT NOT NULL,
            execution_plan_json TEXT NOT NULL DEFAULT '{}',
            loop_mode TEXT NOT NULL DEFAULT 'once',
            current_cycle INTEGER NOT NULL DEFAULT 0,
            init_done INTEGER NOT NULL DEFAULT 0,
            resume_token TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS task_items (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            order_no INTEGER NOT NULL,
            qa_file_path TEXT NOT NULL,
            status TEXT NOT NULL,
            ticket_files_json TEXT NOT NULL,
            ticket_content_json TEXT NOT NULL,
            fix_required INTEGER NOT NULL DEFAULT 0,
            fixed INTEGER NOT NULL DEFAULT 0,
            last_error TEXT NOT NULL DEFAULT '',
            started_at TEXT,
            completed_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id)
        );

        CREATE TABLE IF NOT EXISTS command_runs (
            id TEXT PRIMARY KEY,
            task_item_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            exit_code INTEGER,
            stdout_path TEXT NOT NULL,
            stderr_path TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            interrupted INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(task_item_id) REFERENCES task_items(id)
        );

        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            task_item_id TEXT,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS orchestrator_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            config_yaml TEXT NOT NULL,
            config_json TEXT NOT NULL,
            version INTEGER NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS orchestrator_config_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            version INTEGER NOT NULL,
            config_yaml TEXT NOT NULL,
            config_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            author TEXT NOT NULL DEFAULT 'system'
        );

        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_task_items_task_order ON task_items(task_id, order_no);
        CREATE INDEX IF NOT EXISTS idx_task_items_status ON task_items(status);
        CREATE INDEX IF NOT EXISTS idx_command_runs_task_item_phase ON command_runs(task_item_id, phase);
        CREATE INDEX IF NOT EXISTS idx_events_task_created_at ON events(task_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_cfg_versions_version ON orchestrator_config_versions(version DESC);
        "#,
    )
    .context("failed to initialize schema")?;

    ensure_column(
        &conn,
        "tasks",
        "workspace_id",
        "ALTER TABLE tasks ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "workflow_id",
        "ALTER TABLE tasks ADD COLUMN workflow_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "project_id",
        "ALTER TABLE tasks ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "workspace_root",
        "ALTER TABLE tasks ADD COLUMN workspace_root TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "qa_targets_json",
        "ALTER TABLE tasks ADD COLUMN qa_targets_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "ticket_dir",
        "ALTER TABLE tasks ADD COLUMN ticket_dir TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "execution_plan_json",
        "ALTER TABLE tasks ADD COLUMN execution_plan_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "loop_mode",
        "ALTER TABLE tasks ADD COLUMN loop_mode TEXT NOT NULL DEFAULT 'once'",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "current_cycle",
        "ALTER TABLE tasks ADD COLUMN current_cycle INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &conn,
        "tasks",
        "init_done",
        "ALTER TABLE tasks ADD COLUMN init_done INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &conn,
        "command_runs",
        "workspace_id",
        "ALTER TABLE command_runs ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "command_runs",
        "agent_id",
        "ALTER TABLE command_runs ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        &conn,
        "command_runs",
        "project_id",
        "ALTER TABLE command_runs ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
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

pub fn reset_db(state: &crate::state::InnerState, include_history: bool) -> Result<()> {
    let conn = open_conn(&state.db_path)?;
    conn.execute("DELETE FROM events", [])?;
    conn.execute("DELETE FROM command_runs", [])?;
    conn.execute("DELETE FROM task_items", [])?;
    conn.execute("DELETE FROM tasks", [])?;
    if include_history {
        conn.execute(
            "DELETE FROM orchestrator_config_versions WHERE version < (SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions)",
            [],
        )?;
    }
    Ok(())
}
