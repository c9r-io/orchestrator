use anyhow::{Context, Result};
use rusqlite::Connection;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: fn(&Connection) -> Result<()>,
}

/// Returns the current schema version (0 if no migrations have run).
pub fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
    )
    .context("failed to create schema_migrations table")?;

    let version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .context("failed to read current schema version")?;
    Ok(version)
}

/// Run all pending migrations. Returns the number of migrations applied.
pub fn run_pending(conn: &Connection, migrations: &[Migration]) -> Result<u32> {
    let current = current_version(conn)?;
    let mut applied = 0u32;

    for m in migrations {
        if m.version <= current {
            continue;
        }
        let tx = conn
            .unchecked_transaction()
            .with_context(|| format!("failed to begin transaction for migration {}", m.name))?;

        (m.up)(&tx).with_context(|| format!("migration {} failed", m.name))?;

        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![m.version, m.name],
        )
        .with_context(|| format!("failed to record migration version {}", m.version))?;

        tx.commit()
            .with_context(|| format!("failed to commit migration {}", m.name))?;

        applied += 1;
    }

    Ok(applied)
}

/// All registered migrations in version order.
pub fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "m0001_baseline_schema",
            up: m0001_baseline_schema,
        },
        Migration {
            version: 2,
            name: "m0002_backfill_legacy_defaults",
            up: m0002_backfill_legacy_defaults,
        },
        Migration {
            version: 3,
            name: "m0003_events_promote_columns",
            up: m0003_events_promote_columns,
        },
        Migration {
            version: 4,
            name: "m0004_events_backfill_promoted",
            up: m0004_events_backfill_promoted,
        },
        Migration {
            version: 5,
            name: "m0005_add_task_lookup_indexes",
            up: m0005_add_task_lookup_indexes,
        },
        Migration {
            version: 6,
            name: "m0006_add_pipeline_vars_json",
            up: m0006_add_pipeline_vars_json,
        },
        Migration {
            version: 7,
            name: "m0007_workflow_store_entries",
            up: m0007_workflow_store_entries,
        },
    ]
}

// ── Migration 1: Baseline Schema ──

fn m0001_baseline_schema(conn: &Connection) -> Result<()> {
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
            project_id TEXT NOT NULL DEFAULT '',
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
            workspace_id TEXT NOT NULL DEFAULT '',
            agent_id TEXT NOT NULL DEFAULT '',
            project_id TEXT NOT NULL DEFAULT '',
            exit_code INTEGER,
            stdout_path TEXT NOT NULL,
            stderr_path TEXT NOT NULL,
            output_json TEXT NOT NULL DEFAULT '{}',
            artifacts_json TEXT NOT NULL DEFAULT '[]',
            confidence REAL,
            quality_score REAL,
            validation_status TEXT NOT NULL DEFAULT 'unknown',
            started_at TEXT NOT NULL,
            ended_at TEXT,
            interrupted INTEGER NOT NULL DEFAULT 0,
            session_id TEXT,
            machine_output_source TEXT NOT NULL DEFAULT 'stdout',
            output_json_path TEXT,
            pid INTEGER,
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

        CREATE TABLE IF NOT EXISTS task_execution_metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            status TEXT NOT NULL,
            current_cycle INTEGER NOT NULL,
            unresolved_items INTEGER NOT NULL,
            total_items INTEGER NOT NULL,
            failed_items INTEGER NOT NULL,
            command_runs INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_sessions (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            task_item_id TEXT,
            step_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            state TEXT NOT NULL,
            pid INTEGER NOT NULL,
            pty_backend TEXT NOT NULL,
            cwd TEXT NOT NULL,
            command TEXT NOT NULL,
            input_fifo_path TEXT NOT NULL,
            stdout_path TEXT NOT NULL,
            stderr_path TEXT NOT NULL,
            transcript_path TEXT NOT NULL,
            output_json_path TEXT,
            writer_client_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            ended_at TEXT,
            exit_code INTEGER
        );

        CREATE TABLE IF NOT EXISTS session_attachments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            client_id TEXT NOT NULL,
            mode TEXT NOT NULL,
            attached_at TEXT NOT NULL,
            detached_at TEXT,
            reason TEXT
        );

        CREATE TABLE IF NOT EXISTS config_heal_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            version INTEGER NOT NULL,
            original_error TEXT NOT NULL,
            workflow_id TEXT NOT NULL,
            step_id TEXT NOT NULL,
            rule TEXT NOT NULL,
            detail TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_task_items_task_order ON task_items(task_id, order_no);
        CREATE INDEX IF NOT EXISTS idx_task_items_status ON task_items(status);
        CREATE INDEX IF NOT EXISTS idx_command_runs_task_item_phase ON command_runs(task_item_id, phase);
        CREATE INDEX IF NOT EXISTS idx_command_runs_task_item_phase_started ON command_runs(task_item_id, phase, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_events_task_created_at ON events(task_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_cfg_versions_version ON orchestrator_config_versions(version DESC);
        CREATE INDEX IF NOT EXISTS idx_task_exec_metrics_task_created ON task_execution_metrics(task_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_task_step_state ON agent_sessions(task_id, step_id, state);
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_pid_state ON agent_sessions(pid, state);
        CREATE INDEX IF NOT EXISTS idx_session_attachments_session_attached ON session_attachments(session_id, attached_at DESC);
        CREATE INDEX IF NOT EXISTS idx_config_heal_log_version ON config_heal_log(version DESC);
        CREATE INDEX IF NOT EXISTS idx_command_runs_validation_status ON command_runs(validation_status);
        "#,
    )
    .context("m0001: failed to create baseline schema")?;

    // Safety net for partially-migrated databases: ensure all columns exist
    // (no-op if the CREATE TABLE above already created them inline)
    use crate::db::ensure_column;
    ensure_column(
        conn,
        "tasks",
        "workspace_id",
        "ALTER TABLE tasks ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tasks",
        "workflow_id",
        "ALTER TABLE tasks ADD COLUMN workflow_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tasks",
        "project_id",
        "ALTER TABLE tasks ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tasks",
        "workspace_root",
        "ALTER TABLE tasks ADD COLUMN workspace_root TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tasks",
        "qa_targets_json",
        "ALTER TABLE tasks ADD COLUMN qa_targets_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "tasks",
        "ticket_dir",
        "ALTER TABLE tasks ADD COLUMN ticket_dir TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "tasks",
        "execution_plan_json",
        "ALTER TABLE tasks ADD COLUMN execution_plan_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        conn,
        "tasks",
        "loop_mode",
        "ALTER TABLE tasks ADD COLUMN loop_mode TEXT NOT NULL DEFAULT 'once'",
    )?;
    ensure_column(
        conn,
        "tasks",
        "current_cycle",
        "ALTER TABLE tasks ADD COLUMN current_cycle INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "tasks",
        "init_done",
        "ALTER TABLE tasks ADD COLUMN init_done INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "workspace_id",
        "ALTER TABLE command_runs ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "agent_id",
        "ALTER TABLE command_runs ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "project_id",
        "ALTER TABLE command_runs ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "output_json",
        "ALTER TABLE command_runs ADD COLUMN output_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "artifacts_json",
        "ALTER TABLE command_runs ADD COLUMN artifacts_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "confidence",
        "ALTER TABLE command_runs ADD COLUMN confidence REAL",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "quality_score",
        "ALTER TABLE command_runs ADD COLUMN quality_score REAL",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "validation_status",
        "ALTER TABLE command_runs ADD COLUMN validation_status TEXT NOT NULL DEFAULT 'unknown'",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "session_id",
        "ALTER TABLE command_runs ADD COLUMN session_id TEXT",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "machine_output_source",
        "ALTER TABLE command_runs ADD COLUMN machine_output_source TEXT NOT NULL DEFAULT 'stdout'",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "output_json_path",
        "ALTER TABLE command_runs ADD COLUMN output_json_path TEXT",
    )?;
    ensure_column(
        conn,
        "command_runs",
        "pid",
        "ALTER TABLE command_runs ADD COLUMN pid INTEGER",
    )?;

    Ok(())
}

// ── Migration 2: One-Time Legacy Backfills ──

fn m0002_backfill_legacy_defaults(conn: &Connection) -> Result<()> {
    // Config-independent backfill: hardcoded value
    conn.execute(
        "UPDATE command_runs SET agent_id = 'legacy' WHERE agent_id = ''",
        [],
    )
    .context("m0002: failed to backfill agent_id")?;

    // Event step_scope backfill (moved from events_backfill.rs)
    // Uses row-by-row Rust logic to parse JSON + infer scope from task_item_id
    let mut stmt = conn.prepare(
        "SELECT id, task_item_id, payload_json FROM events
         WHERE event_type IN ('step_started','step_finished','step_skipped','step_spawned','step_timeout')
           AND payload_json NOT LIKE '%step_scope%'",
    )?;

    let rows: Vec<(i64, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (id, task_item_id, payload_json) in &rows {
        let mut payload: serde_json::Value = match serde_json::from_str(payload_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if payload.get("step_scope").is_some() {
            continue;
        }

        let inferred_scope = if task_item_id.is_some() {
            "item"
        } else {
            "task"
        };

        payload["step_scope"] = serde_json::Value::String(inferred_scope.to_string());
        let new_json = serde_json::to_string(&payload)?;
        conn.execute(
            "UPDATE events SET payload_json = ?1 WHERE id = ?2",
            rusqlite::params![new_json, id],
        )?;
    }

    Ok(())
}

// ── Migration 3: Events Column Promotion ──

fn m0003_events_promote_columns(conn: &Connection) -> Result<()> {
    use crate::db::ensure_column;
    ensure_column(
        conn,
        "events",
        "step",
        "ALTER TABLE events ADD COLUMN step TEXT",
    )?;
    ensure_column(
        conn,
        "events",
        "step_scope",
        "ALTER TABLE events ADD COLUMN step_scope TEXT",
    )?;
    ensure_column(
        conn,
        "events",
        "cycle",
        "ALTER TABLE events ADD COLUMN cycle INTEGER",
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_task_type_step ON events(task_id, event_type, step)",
        [],
    )
    .context("m0003: failed to create idx_events_task_type_step")?;

    Ok(())
}

// ── Migration 4: Backfill Promoted Event Columns ──

fn m0004_events_backfill_promoted(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        UPDATE events SET step = json_extract(payload_json, '$.step')
        WHERE step IS NULL AND json_extract(payload_json, '$.step') IS NOT NULL;

        UPDATE events SET step = json_extract(payload_json, '$.phase')
        WHERE step IS NULL AND json_extract(payload_json, '$.phase') IS NOT NULL;

        UPDATE events SET step_scope = json_extract(payload_json, '$.step_scope')
        WHERE step_scope IS NULL AND json_extract(payload_json, '$.step_scope') IS NOT NULL;

        UPDATE events SET cycle = json_extract(payload_json, '$.cycle')
        WHERE cycle IS NULL AND event_type = 'cycle_started'
          AND json_extract(payload_json, '$.cycle') IS NOT NULL;
        "#,
    )
    .context("m0004: failed to backfill promoted event columns")?;

    Ok(())
}

// ── Migration 5: Task Lookup Indexes ──

fn m0005_add_task_lookup_indexes(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);
         CREATE INDEX IF NOT EXISTS idx_tasks_workspace_id ON tasks(workspace_id);
         CREATE INDEX IF NOT EXISTS idx_tasks_workflow_id ON tasks(workflow_id);",
    )
    .context("m0005: create task lookup indexes")?;
    Ok(())
}

// ── Migration 6: Pipeline Variables JSON Column ──

fn m0006_add_pipeline_vars_json(conn: &Connection) -> Result<()> {
    use crate::db::ensure_column;
    ensure_column(
        conn,
        "tasks",
        "pipeline_vars_json",
        "ALTER TABLE tasks ADD COLUMN pipeline_vars_json TEXT",
    )?;
    Ok(())
}

// ── Migration 7: Workflow Store Entries Table ──

fn m0007_workflow_store_entries(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workflow_store_entries (
            store_name TEXT NOT NULL,
            project_id TEXT NOT NULL DEFAULT '',
            key TEXT NOT NULL,
            value_json TEXT NOT NULL,
            task_id TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (store_name, project_id, key)
        );

        CREATE INDEX IF NOT EXISTS idx_wse_store_project
            ON workflow_store_entries(store_name, project_id);
        CREATE INDEX IF NOT EXISTS idx_wse_updated_at
            ON workflow_store_entries(store_name, project_id, updated_at DESC);
        "#,
    )
    .context("m0007: failed to create workflow_store_entries table")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::configure_conn;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        configure_conn(&conn).expect("configure conn");
        conn
    }

    #[test]
    fn run_pending_applies_all_on_fresh_db() {
        let conn = mem_conn();
        let migrations = all_migrations();
        let applied = run_pending(&conn, &migrations).expect("run_pending");
        assert_eq!(applied, 7);
        assert_eq!(current_version(&conn).expect("version"), 7);
    }

    #[test]
    fn run_pending_is_idempotent() {
        let conn = mem_conn();
        let migrations = all_migrations();
        run_pending(&conn, &migrations).expect("first run");
        let applied = run_pending(&conn, &migrations).expect("second run");
        assert_eq!(applied, 0);
        assert_eq!(current_version(&conn).expect("version"), 7);
    }

    #[test]
    fn partial_then_full_applies_remaining() {
        let conn = mem_conn();
        let all = all_migrations();

        // Apply only first 2
        let partial: Vec<Migration> = vec![
            Migration {
                version: 1,
                name: all[0].name,
                up: all[0].up,
            },
            Migration {
                version: 2,
                name: all[1].name,
                up: all[1].up,
            },
        ];
        let applied = run_pending(&conn, &partial).expect("partial run");
        assert_eq!(applied, 2);
        assert_eq!(current_version(&conn).expect("version"), 2);

        // Apply all 7 — should only run 3, 4, 5, 6, and 7
        let applied = run_pending(&conn, &all).expect("full run");
        assert_eq!(applied, 5);
        assert_eq!(current_version(&conn).expect("version"), 7);
    }

    #[test]
    fn failed_migration_does_not_advance_version() {
        let conn = mem_conn();

        fn fail_migration(_conn: &Connection) -> Result<()> {
            anyhow::bail!("intentional failure");
        }

        // Run migration 1 first so we have tables
        let first = vec![Migration {
            version: 1,
            name: "m0001_baseline_schema",
            up: m0001_baseline_schema,
        }];
        run_pending(&conn, &first).expect("first migration");

        let bad = vec![
            Migration {
                version: 1,
                name: "m0001_baseline_schema",
                up: m0001_baseline_schema,
            },
            Migration {
                version: 2,
                name: "m_fail",
                up: fail_migration,
            },
        ];

        let err = run_pending(&conn, &bad);
        assert!(err.is_err());
        assert_eq!(current_version(&conn).expect("version"), 1);
    }

    #[test]
    fn baseline_schema_creates_all_tables() {
        let conn = mem_conn();
        let migrations = vec![Migration {
            version: 1,
            name: "m0001_baseline_schema",
            up: m0001_baseline_schema,
        }];
        run_pending(&conn, &migrations).expect("run baseline");

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
        assert!(tables.contains(&"config_heal_log".to_string()));
        assert!(tables.contains(&"schema_migrations".to_string()));
    }

    #[test]
    fn baseline_schema_is_idempotent_on_existing_db() {
        let conn = mem_conn();
        // Simulate an existing DB with partial schema (old-style CREATE TABLE)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
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
            )",
        )
        .expect("create old tasks table");

        // Run baseline — should not fail
        let migrations = vec![Migration {
            version: 1,
            name: "m0001_baseline_schema",
            up: m0001_baseline_schema,
        }];
        run_pending(&conn, &migrations).expect("baseline on existing db");
    }

    #[test]
    fn events_promote_columns_adds_columns() {
        let conn = mem_conn();
        let migrations = all_migrations();
        run_pending(&conn, &migrations).expect("run all");

        // Verify events has the new columns
        let mut stmt = conn.prepare("PRAGMA table_info(events)").expect("prepare");
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect");

        assert!(cols.contains(&"step".to_string()));
        assert!(cols.contains(&"step_scope".to_string()));
        assert!(cols.contains(&"cycle".to_string()));
    }

    #[test]
    fn backfill_promoted_populates_from_json() {
        let conn = mem_conn();
        // Run migrations 1-3 first
        let mut migs = all_migrations();
        let _m7 = migs.pop().expect("pop m7");
        let _m6 = migs.pop().expect("pop m6");
        let _m5 = migs.pop().expect("pop m5");
        let m4 = migs.pop().expect("pop m4");
        run_pending(&conn, &migs).expect("run m1-m3");

        // Insert test events
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES ('t1', 'step_started', '{\"step\":\"qa\",\"step_scope\":\"item\"}', '2026-01-01')",
            [],
        ).expect("insert event 1");
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES ('t1', 'cycle_started', '{\"cycle\":2}', '2026-01-01')",
            [],
        )
        .expect("insert event 2");
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES ('t1', 'step_spawned', '{\"phase\":\"implement\"}', '2026-01-01')",
            [],
        )
        .expect("insert event 3");

        // Run m4
        let m4_vec = vec![m4];
        run_pending(&conn, &m4_vec).expect("run m4");

        // Verify backfill
        let (step, scope): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT step, step_scope FROM events WHERE event_type = 'step_started'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query step_started");
        assert_eq!(step.as_deref(), Some("qa"));
        assert_eq!(scope.as_deref(), Some("item"));

        let cycle: Option<i64> = conn
            .query_row(
                "SELECT cycle FROM events WHERE event_type = 'cycle_started'",
                [],
                |row| row.get(0),
            )
            .expect("query cycle_started");
        assert_eq!(cycle, Some(2));

        // phase fallback → step column
        let step_from_phase: Option<String> = conn
            .query_row(
                "SELECT step FROM events WHERE event_type = 'step_spawned'",
                [],
                |row| row.get(0),
            )
            .expect("query step_spawned");
        assert_eq!(step_from_phase.as_deref(), Some("implement"));
    }
}
