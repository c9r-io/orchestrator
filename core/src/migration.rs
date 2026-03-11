use anyhow::{Context, Result};
use rusqlite::Connection;

const HISTORICAL_AGENT_PLACEHOLDER: &str = "legacy";

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
            up: m0002_backfill_historical_defaults,
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
        Migration {
            version: 8,
            name: "m0008_workflow_primitives",
            up: m0008_workflow_primitives,
        },
        Migration {
            version: 9,
            name: "m0009_normalize_unspecified_agent_ids",
            up: m0009_normalize_unspecified_agent_ids,
        },
        Migration {
            version: 10,
            name: "m0010_per_resource_persistence",
            up: m0010_per_resource_persistence,
        },
        Migration {
            version: 11,
            name: "m0011_finalize_resource_migration",
            up: m0011_finalize_resource_migration,
        },
        Migration {
            version: 12,
            name: "m0012_drop_legacy_orchestrator_config_blob",
            up: m0012_drop_legacy_orchestrator_config_blob,
        },
        Migration {
            version: 13,
            name: "m0013_control_plane_audit",
            up: m0013_control_plane_audit,
        },
        Migration {
            version: 14,
            name: "m0014_task_graph_debug_tables",
            up: m0014_task_graph_debug_tables,
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

// ── Migration 2: One-Time Historical Backfills ──

fn m0002_backfill_historical_defaults(conn: &Connection) -> Result<()> {
    // Config-independent backfill: assign a neutral placeholder for old empty agent IDs.
    conn.execute(
        "UPDATE command_runs SET agent_id = 'unspecified' WHERE agent_id = ''",
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

// ── Migration 8: Workflow Primitives (WP02 + WP03) ──

fn m0008_workflow_primitives(conn: &Connection) -> Result<()> {
    use crate::db::ensure_column;

    // WP02: Task lineage
    ensure_column(
        conn,
        "tasks",
        "parent_task_id",
        "ALTER TABLE tasks ADD COLUMN parent_task_id TEXT",
    )?;
    ensure_column(
        conn,
        "tasks",
        "spawn_reason",
        "ALTER TABLE tasks ADD COLUMN spawn_reason TEXT",
    )?;
    ensure_column(
        conn,
        "tasks",
        "spawn_depth",
        "ALTER TABLE tasks ADD COLUMN spawn_depth INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_parent_id ON tasks(parent_task_id);")
        .context("m0008: create parent_task_id index")?;

    // WP03: Dynamic item metadata
    ensure_column(
        conn,
        "task_items",
        "dynamic_vars_json",
        "ALTER TABLE task_items ADD COLUMN dynamic_vars_json TEXT",
    )?;
    ensure_column(
        conn,
        "task_items",
        "label",
        "ALTER TABLE task_items ADD COLUMN label TEXT",
    )?;
    ensure_column(
        conn,
        "task_items",
        "source",
        "ALTER TABLE task_items ADD COLUMN source TEXT NOT NULL DEFAULT 'static'",
    )?;

    Ok(())
}

// ── Migration 10: Per-Resource Persistence ──

fn m0010_per_resource_persistence(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS resources (
            kind TEXT NOT NULL,
            project TEXT NOT NULL,
            name TEXT NOT NULL,
            api_version TEXT NOT NULL,
            spec_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            generation INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (kind, project, name)
        );

        CREATE TABLE IF NOT EXISTS resource_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            project TEXT NOT NULL,
            name TEXT NOT NULL,
            spec_json TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            version INTEGER NOT NULL,
            author TEXT NOT NULL DEFAULT 'system',
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_resources_project ON resources(project);
        CREATE INDEX IF NOT EXISTS idx_resource_versions_lookup
            ON resource_versions(kind, project, name, version DESC);
        "#,
    )
    .context("m0010: failed to create resources tables")?;

    Ok(())
}

fn m0011_finalize_resource_migration(conn: &Connection) -> Result<()> {
    let _ = conn;
    Ok(())
}

fn m0012_drop_legacy_orchestrator_config_blob(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP TABLE IF EXISTS orchestrator_config;")
        .context("m0012: failed to drop orchestrator_config")?;
    Ok(())
}

fn m0013_control_plane_audit(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS control_plane_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at TEXT NOT NULL,
            transport TEXT NOT NULL,
            remote_addr TEXT,
            rpc TEXT NOT NULL,
            subject_id TEXT,
            authn_result TEXT NOT NULL,
            authz_result TEXT NOT NULL,
            role TEXT,
            reason TEXT,
            tls_fingerprint TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_control_plane_audit_created_at
            ON control_plane_audit(created_at);

        CREATE INDEX IF NOT EXISTS idx_control_plane_audit_rpc
            ON control_plane_audit(rpc, created_at);
        "#,
    )?;
    Ok(())
}

fn m0014_task_graph_debug_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS task_graph_runs (
            graph_run_id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            cycle INTEGER NOT NULL DEFAULT 0,
            mode TEXT NOT NULL DEFAULT 'dynamic_dag',
            source TEXT NOT NULL DEFAULT 'unknown',
            status TEXT NOT NULL DEFAULT 'materialized',
            fallback_mode TEXT,
            planner_failure_class TEXT,
            planner_failure_message TEXT,
            entry_node_id TEXT,
            node_count INTEGER NOT NULL DEFAULT 0,
            edge_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS task_graph_snapshots (
            graph_run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            snapshot_kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY(graph_run_id, snapshot_kind),
            FOREIGN KEY(graph_run_id) REFERENCES task_graph_runs(graph_run_id) ON DELETE CASCADE,
            FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_task_graph_runs_task_cycle
            ON task_graph_runs(task_id, cycle DESC, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_task_graph_snapshots_task
            ON task_graph_snapshots(task_id, graph_run_id);
        "#,
    )?;
    Ok(())
}

fn m0009_normalize_unspecified_agent_ids(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE command_runs SET agent_id = 'unspecified' WHERE agent_id = ?1 OR agent_id = ''",
        rusqlite::params![HISTORICAL_AGENT_PLACEHOLDER],
    )
    .context("m0009: failed to normalize command_runs.agent_id placeholders")?;

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
        let latest_version = migrations.last().expect("at least one migration").version;
        assert_eq!(applied, latest_version);
        assert_eq!(current_version(&conn).expect("version"), latest_version);
    }

    #[test]
    fn run_pending_is_idempotent() {
        let conn = mem_conn();
        let migrations = all_migrations();
        run_pending(&conn, &migrations).expect("first run");
        let applied = run_pending(&conn, &migrations).expect("second run");
        let latest_version = migrations.last().expect("at least one migration").version;
        assert_eq!(applied, 0);
        assert_eq!(current_version(&conn).expect("version"), latest_version);
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

        // Apply the full set after running the first two — only the remainder should execute.
        let applied = run_pending(&conn, &all).expect("full run");
        let latest_version = all.last().expect("at least one migration").version;
        assert_eq!(applied, latest_version - 2);
        assert_eq!(current_version(&conn).expect("version"), latest_version);
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
        assert!(tables.contains(&"orchestrator_config_versions".to_string()));
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
        // Run migrations 1-3 first, then isolate m4 for testing.
        let migrations = all_migrations();
        let initial: Vec<Migration> = migrations
            .iter()
            .filter(|migration| migration.version <= 3)
            .map(|migration| Migration {
                version: migration.version,
                name: migration.name,
                up: migration.up,
            })
            .collect();
        let m4 = migrations
            .iter()
            .find(|migration| migration.version == 4)
            .map(|migration| Migration {
                version: migration.version,
                name: migration.name,
                up: migration.up,
            })
            .expect("find m4");
        run_pending(&conn, &initial).expect("run m1-m3");

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

    #[test]
    fn normalize_unspecified_agent_ids_rewrites_historical_placeholder() {
        let conn = mem_conn();
        let migrations = all_migrations();
        run_pending(&conn, &migrations).expect("run all");

        conn.execute(
            "INSERT INTO tasks (
                id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
                project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
             ) VALUES (
                'task-1', 'test', 'running', 'goal', '[]', 'once', 'default', 'basic',
                'default', '.', '[]', 'docs/ticket', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_items (
                id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json,
                created_at, updated_at
             ) VALUES (
                'item-1', 'task-1', 0, 'qa.md', 'pending', '[]', '{}',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task item");
        conn.execute(
            "INSERT INTO command_runs (
                id, task_item_id, phase, command, cwd, workspace_id, agent_id, project_id,
                stdout_path, stderr_path, started_at
             ) VALUES (
                'run-1', 'item-1', 'qa', 'echo ok', '.', 'default', ?1, 'default',
                '/tmp/stdout', '/tmp/stderr', '2026-01-01T00:00:00Z'
             )",
            rusqlite::params![HISTORICAL_AGENT_PLACEHOLDER],
        )
        .expect("insert command run with historical placeholder");

        let normalize = vec![Migration {
            version: 9,
            name: "m0009_normalize_unspecified_agent_ids",
            up: m0009_normalize_unspecified_agent_ids,
        }];

        conn.execute("DELETE FROM schema_migrations WHERE version >= 9", [])
            .expect("clear migration 9+ records");
        run_pending(&conn, &normalize).expect("rerun m0009");

        let agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM command_runs WHERE id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .expect("query normalized agent_id");
        assert_eq!(agent_id, "unspecified");
    }

    // ============================================================================
    // m0002 data-driven tests: backfill_historical_defaults
    // ============================================================================

    #[test]
    fn m0002_backfills_empty_agent_id() {
        let conn = mem_conn();
        // Run m0001 to create baseline schema
        let all = all_migrations();
        let m1 = vec![Migration {
            version: 1,
            name: all[0].name,
            up: all[0].up,
        }];
        run_pending(&conn, &m1).expect("run m0001");

        // Insert a task + item + command_run with empty agent_id
        conn.execute(
            "INSERT INTO tasks (
                id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
                project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
             ) VALUES (
                'task-1', 'test', 'running', 'goal', '[]', 'once', 'default', 'basic',
                'default', '.', '[]', 'docs/ticket', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_items (
                id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json,
                created_at, updated_at
             ) VALUES (
                'item-1', 'task-1', 0, 'qa.md', 'pending', '[]', '{}',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task item");
        conn.execute(
            "INSERT INTO command_runs (
                id, task_item_id, phase, command, cwd, workspace_id, agent_id, project_id,
                stdout_path, stderr_path, started_at
             ) VALUES (
                'run-1', 'item-1', 'qa', 'echo ok', '.', 'default', '', 'default',
                '/tmp/stdout', '/tmp/stderr', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert command run with empty agent_id");

        // Run m0002
        let m2 = vec![
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
        run_pending(&conn, &m2).expect("run m0002");

        let agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM command_runs WHERE id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .expect("query agent_id");
        assert_eq!(agent_id, "unspecified");
    }

    #[test]
    fn m0002_backfills_event_step_scope_from_task_item_id() {
        let conn = mem_conn();
        let all = all_migrations();
        let m1 = vec![Migration {
            version: 1,
            name: all[0].name,
            up: all[0].up,
        }];
        run_pending(&conn, &m1).expect("run m0001");

        // Insert event with task_item_id set (should infer "item" scope)
        conn.execute(
            "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
             VALUES ('t1', 'item-1', 'step_started', '{\"step\":\"qa\"}', '2026-01-01')",
            [],
        )
        .expect("insert event with task_item_id");

        // Insert event without task_item_id (should infer "task" scope)
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES ('t1', 'step_finished', '{\"step\":\"build\"}', '2026-01-01')",
            [],
        )
        .expect("insert event without task_item_id");

        let m2 = vec![
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
        run_pending(&conn, &m2).expect("run m0002");

        // Check item-scoped event
        let payload1: String = conn
            .query_row(
                "SELECT payload_json FROM events WHERE event_type = 'step_started'",
                [],
                |row| row.get(0),
            )
            .expect("query step_started payload");
        let parsed1: serde_json::Value = serde_json::from_str(&payload1).expect("parse payload");
        assert_eq!(parsed1["step_scope"], "item");

        // Check task-scoped event
        let payload2: String = conn
            .query_row(
                "SELECT payload_json FROM events WHERE event_type = 'step_finished'",
                [],
                |row| row.get(0),
            )
            .expect("query step_finished payload");
        let parsed2: serde_json::Value = serde_json::from_str(&payload2).expect("parse payload");
        assert_eq!(parsed2["step_scope"], "task");
    }

    #[test]
    fn m0002_skips_event_with_existing_step_scope() {
        let conn = mem_conn();
        let all = all_migrations();
        let m1 = vec![Migration {
            version: 1,
            name: all[0].name,
            up: all[0].up,
        }];
        run_pending(&conn, &m1).expect("run m0001");

        // Insert event that already has step_scope in payload
        conn.execute(
            "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
             VALUES ('t1', 'item-1', 'step_started', '{\"step\":\"qa\",\"step_scope\":\"item\"}', '2026-01-01')",
            [],
        )
        .expect("insert event with existing step_scope");

        let m2 = vec![
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
        run_pending(&conn, &m2).expect("run m0002");

        // Payload should remain unchanged
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM events WHERE event_type = 'step_started'",
                [],
                |row| row.get(0),
            )
            .expect("query payload");
        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("parse");
        assert_eq!(parsed["step_scope"], "item");
    }

    #[test]
    fn m0002_skips_unparseable_json() {
        let conn = mem_conn();
        let all = all_migrations();
        let m1 = vec![Migration {
            version: 1,
            name: all[0].name,
            up: all[0].up,
        }];
        run_pending(&conn, &m1).expect("run m0001");

        // Insert event with invalid JSON
        conn.execute(
            "INSERT INTO events (task_id, event_type, payload_json, created_at)
             VALUES ('t1', 'step_started', 'not-valid-json', '2026-01-01')",
            [],
        )
        .expect("insert event with bad json");

        let m2 = vec![
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
        // Should not fail — bad JSON is skipped via `continue`
        run_pending(&conn, &m2).expect("m0002 should skip bad JSON");

        // Payload should remain unchanged
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM events WHERE event_type = 'step_started'",
                [],
                |row| row.get(0),
            )
            .expect("query payload");
        assert_eq!(payload, "not-valid-json");
    }

    // ============================================================================
    // m0009 empty agent_id branch
    // ============================================================================

    #[test]
    fn m0009_normalizes_empty_agent_id() {
        let conn = mem_conn();
        let all = all_migrations();
        // Run all migrations up to m0008 to get the schema in place
        let up_to_8: Vec<Migration> = all
            .iter()
            .filter(|m| m.version <= 8)
            .map(|m| Migration {
                version: m.version,
                name: m.name,
                up: m.up,
            })
            .collect();
        run_pending(&conn, &up_to_8).expect("run m1-m8");

        // Insert task + item + command run with empty agent_id
        conn.execute(
            "INSERT INTO tasks (
                id, name, status, goal, target_files_json, mode, workspace_id, workflow_id,
                project_id, workspace_root, qa_targets_json, ticket_dir, created_at, updated_at
             ) VALUES (
                'task-1', 'test', 'running', 'goal', '[]', 'once', 'default', 'basic',
                'default', '.', '[]', 'docs/ticket', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task");
        conn.execute(
            "INSERT INTO task_items (
                id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json,
                created_at, updated_at
             ) VALUES (
                'item-1', 'task-1', 0, 'qa.md', 'pending', '[]', '{}',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert task item");
        conn.execute(
            "INSERT INTO command_runs (
                id, task_item_id, phase, command, cwd, workspace_id, agent_id, project_id,
                stdout_path, stderr_path, started_at
             ) VALUES (
                'run-empty', 'item-1', 'qa', 'echo ok', '.', 'default', '', 'default',
                '/tmp/stdout', '/tmp/stderr', '2026-01-01T00:00:00Z'
             )",
            [],
        )
        .expect("insert command run with empty agent_id");

        // Run m0009
        let m9 = all
            .iter()
            .filter(|m| m.version <= 9)
            .map(|m| Migration {
                version: m.version,
                name: m.name,
                up: m.up,
            })
            .collect::<Vec<_>>();
        run_pending(&conn, &m9).expect("run m9");

        let agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM command_runs WHERE id = 'run-empty'",
                [],
                |row| row.get(0),
            )
            .expect("query agent_id");
        assert_eq!(agent_id, "unspecified");
    }

    // ============================================================================
    // all_migrations() invariants
    // ============================================================================

    #[test]
    fn all_migrations_versions_are_ascending() {
        let migrations = all_migrations();
        for window in migrations.windows(2) {
            assert!(
                window[0].version < window[1].version,
                "migration versions must be ascending: {} >= {}",
                window[0].version,
                window[1].version
            );
        }
    }

    #[test]
    fn all_migrations_versions_are_contiguous() {
        let migrations = all_migrations();
        assert!(!migrations.is_empty(), "must have at least one migration");
        assert_eq!(
            migrations[0].version, 1,
            "first migration must be version 1"
        );
        for window in migrations.windows(2) {
            assert_eq!(
                window[1].version,
                window[0].version + 1,
                "migration versions must be contiguous: {} -> {}",
                window[0].version,
                window[1].version
            );
        }
    }

    #[test]
    fn all_migrations_names_are_unique() {
        let migrations = all_migrations();
        let mut seen = std::collections::HashSet::new();
        for m in &migrations {
            assert!(seen.insert(m.name), "duplicate migration name: {}", m.name);
        }
    }

    #[test]
    fn all_migrations_count_matches_expected() {
        let migrations = all_migrations();
        assert_eq!(
            migrations.len(),
            14,
            "expected 14 migrations, got {}",
            migrations.len()
        );
    }
}
