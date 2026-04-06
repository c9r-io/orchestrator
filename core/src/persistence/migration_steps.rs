use anyhow::{Context, Result};
use rusqlite::Connection;

pub(crate) const HISTORICAL_AGENT_PLACEHOLDER: &str = "legacy";

fn ensure_column_exists(conn: &Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .with_context(|| format!("failed to read schema for {table}"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if !columns.iter().any(|existing| existing == column) {
        conn.execute(ddl, [])
            .with_context(|| format!("failed to add column {table}.{column}"))?;
    }

    Ok(())
}

pub(crate) fn m0001_baseline_schema(conn: &Connection) -> Result<()> {
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

    ensure_column_exists(
        conn,
        "tasks",
        "workspace_id",
        "ALTER TABLE tasks ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "workflow_id",
        "ALTER TABLE tasks ADD COLUMN workflow_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "project_id",
        "ALTER TABLE tasks ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "workspace_root",
        "ALTER TABLE tasks ADD COLUMN workspace_root TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "qa_targets_json",
        "ALTER TABLE tasks ADD COLUMN qa_targets_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "ticket_dir",
        "ALTER TABLE tasks ADD COLUMN ticket_dir TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "execution_plan_json",
        "ALTER TABLE tasks ADD COLUMN execution_plan_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "loop_mode",
        "ALTER TABLE tasks ADD COLUMN loop_mode TEXT NOT NULL DEFAULT 'once'",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "current_cycle",
        "ALTER TABLE tasks ADD COLUMN current_cycle INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "init_done",
        "ALTER TABLE tasks ADD COLUMN init_done INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "workspace_id",
        "ALTER TABLE command_runs ADD COLUMN workspace_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "agent_id",
        "ALTER TABLE command_runs ADD COLUMN agent_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "project_id",
        "ALTER TABLE command_runs ADD COLUMN project_id TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "output_json",
        "ALTER TABLE command_runs ADD COLUMN output_json TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "artifacts_json",
        "ALTER TABLE command_runs ADD COLUMN artifacts_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "confidence",
        "ALTER TABLE command_runs ADD COLUMN confidence REAL",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "quality_score",
        "ALTER TABLE command_runs ADD COLUMN quality_score REAL",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "validation_status",
        "ALTER TABLE command_runs ADD COLUMN validation_status TEXT NOT NULL DEFAULT 'unknown'",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "session_id",
        "ALTER TABLE command_runs ADD COLUMN session_id TEXT",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "machine_output_source",
        "ALTER TABLE command_runs ADD COLUMN machine_output_source TEXT NOT NULL DEFAULT 'stdout'",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "output_json_path",
        "ALTER TABLE command_runs ADD COLUMN output_json_path TEXT",
    )?;
    ensure_column_exists(
        conn,
        "command_runs",
        "pid",
        "ALTER TABLE command_runs ADD COLUMN pid INTEGER",
    )?;

    Ok(())
}

pub(crate) fn m0002_backfill_historical_defaults(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE command_runs SET agent_id = 'unspecified' WHERE agent_id = ''",
        [],
    )
    .context("m0002: failed to backfill agent_id")?;

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

pub(crate) fn m0003_events_promote_columns(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "events",
        "step",
        "ALTER TABLE events ADD COLUMN step TEXT",
    )?;
    ensure_column_exists(
        conn,
        "events",
        "step_scope",
        "ALTER TABLE events ADD COLUMN step_scope TEXT",
    )?;
    ensure_column_exists(
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

pub(crate) fn m0004_events_backfill_promoted(conn: &Connection) -> Result<()> {
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

pub(crate) fn m0005_add_task_lookup_indexes(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);
         CREATE INDEX IF NOT EXISTS idx_tasks_workspace_id ON tasks(workspace_id);
         CREATE INDEX IF NOT EXISTS idx_tasks_workflow_id ON tasks(workflow_id);",
    )
    .context("m0005: create task lookup indexes")?;
    Ok(())
}

pub(crate) fn m0006_add_pipeline_vars_json(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "tasks",
        "pipeline_vars_json",
        "ALTER TABLE tasks ADD COLUMN pipeline_vars_json TEXT",
    )?;
    Ok(())
}

pub(crate) fn m0007_workflow_store_entries(conn: &Connection) -> Result<()> {
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

pub(crate) fn m0008_workflow_primitives(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "tasks",
        "parent_task_id",
        "ALTER TABLE tasks ADD COLUMN parent_task_id TEXT",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "spawn_reason",
        "ALTER TABLE tasks ADD COLUMN spawn_reason TEXT",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "spawn_depth",
        "ALTER TABLE tasks ADD COLUMN spawn_depth INTEGER NOT NULL DEFAULT 0",
    )?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_tasks_parent_id ON tasks(parent_task_id);")
        .context("m0008: create parent_task_id index")?;

    ensure_column_exists(
        conn,
        "task_items",
        "dynamic_vars_json",
        "ALTER TABLE task_items ADD COLUMN dynamic_vars_json TEXT",
    )?;
    ensure_column_exists(
        conn,
        "task_items",
        "label",
        "ALTER TABLE task_items ADD COLUMN label TEXT",
    )?;
    ensure_column_exists(
        conn,
        "task_items",
        "source",
        "ALTER TABLE task_items ADD COLUMN source TEXT NOT NULL DEFAULT 'static'",
    )?;

    Ok(())
}

pub(crate) fn m0009_normalize_unspecified_agent_ids(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE command_runs SET agent_id = 'unspecified' WHERE agent_id = ?1 OR agent_id = ''",
        rusqlite::params![HISTORICAL_AGENT_PLACEHOLDER],
    )
    .context("m0009: failed to normalize command_runs.agent_id placeholders")?;

    Ok(())
}

pub(crate) fn m0010_per_resource_persistence(conn: &Connection) -> Result<()> {
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

pub(crate) fn m0011_finalize_resource_migration(conn: &Connection) -> Result<()> {
    let _ = conn;
    Ok(())
}

pub(crate) fn m0012_drop_legacy_orchestrator_config_blob(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP TABLE IF EXISTS orchestrator_config;")
        .context("m0012: failed to drop orchestrator_config")?;
    Ok(())
}

pub(crate) fn m0013_control_plane_audit(conn: &Connection) -> Result<()> {
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

pub(crate) fn m0015_control_plane_audit_rejection_stage(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "rejection_stage",
        "ALTER TABLE control_plane_audit ADD COLUMN rejection_stage TEXT",
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_control_plane_audit_rejection_stage
             ON control_plane_audit(rejection_stage, created_at);",
    )?;
    Ok(())
}

pub(crate) fn m0016_secret_key_lifecycle(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS secret_keys (
            key_id TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            file_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            activated_at TEXT,
            rotated_out_at TEXT,
            retired_at TEXT,
            revoked_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_secret_keys_state ON secret_keys(state);

        CREATE TABLE IF NOT EXISTS secret_key_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_kind TEXT NOT NULL,
            key_id TEXT NOT NULL,
            key_fingerprint TEXT NOT NULL,
            actor TEXT NOT NULL,
            detail_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_secret_key_audit_created ON secret_key_audit(created_at);
        CREATE INDEX IF NOT EXISTS idx_secret_key_audit_key_id ON secret_key_audit(key_id, created_at);
        "#,
    )
    .context("m0016: failed to create secret key lifecycle tables")?;

    // Import legacy key if it exists.
    // We need to find the data_dir from the db_path context. Since migrations run
    // in-transaction and we only have &Connection, we attempt to locate the legacy
    // key file relative to common paths. The import is best-effort during migration;
    // the bootstrap phase will ensure the keyring is populated.
    //
    // Note: Legacy import during migration is attempted via a pragmatic heuristic.
    // The authoritative import happens in bootstrap when load_keyring is called.
    Ok(())
}

pub(crate) fn m0017_control_plane_protection_fields(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "traffic_class",
        "ALTER TABLE control_plane_audit ADD COLUMN traffic_class TEXT",
    )?;
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "limit_scope",
        "ALTER TABLE control_plane_audit ADD COLUMN limit_scope TEXT",
    )?;
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "decision",
        "ALTER TABLE control_plane_audit ADD COLUMN decision TEXT",
    )?;
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "reason_code",
        "ALTER TABLE control_plane_audit ADD COLUMN reason_code TEXT",
    )?;
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_control_plane_audit_decision
            ON control_plane_audit(decision, created_at);
        CREATE INDEX IF NOT EXISTS idx_control_plane_audit_reason_code
            ON control_plane_audit(reason_code, created_at);
        "#,
    )?;
    Ok(())
}

pub(crate) fn m0018_trigger_state(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS trigger_state (
            trigger_name TEXT NOT NULL,
            project TEXT NOT NULL,
            last_fired_at TEXT,
            next_fire_at TEXT,
            fire_count INTEGER DEFAULT 0,
            last_task_id TEXT,
            last_status TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (trigger_name, project)
        );
        "#,
    )?;
    Ok(())
}

pub(crate) fn m0019_daemon_incarnation(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS daemon_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        INSERT OR IGNORE INTO daemon_meta (key, value) VALUES ('incarnation', '0');
        "#,
    )
    .context("m0019_daemon_incarnation")?;
    Ok(())
}

pub(crate) fn m0020_command_template_column(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "command_runs",
        "command_template",
        "ALTER TABLE command_runs ADD COLUMN command_template TEXT",
    )?;
    Ok(())
}

pub(crate) fn m0021_command_rule_index_column(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "command_runs",
        "command_rule_index",
        "ALTER TABLE command_runs ADD COLUMN command_rule_index INTEGER",
    )?;
    Ok(())
}

pub(crate) fn m0022_plugin_audit(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS plugin_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at TEXT NOT NULL,
            action TEXT NOT NULL,
            crd_kind TEXT NOT NULL,
            plugin_name TEXT,
            plugin_type TEXT,
            command TEXT NOT NULL,
            applied_by TEXT,
            transport TEXT,
            peer_pid INTEGER,
            result TEXT NOT NULL,
            policy_mode TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_plugin_audit_created_at
            ON plugin_audit(created_at);

        CREATE INDEX IF NOT EXISTS idx_plugin_audit_crd_kind
            ON plugin_audit(crd_kind);
        "#,
    )?;
    Ok(())
}

pub(crate) fn m0023_task_step_filter_and_initial_vars(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "tasks",
        "step_filter_json",
        "ALTER TABLE tasks ADD COLUMN step_filter_json TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column_exists(
        conn,
        "tasks",
        "initial_vars_json",
        "ALTER TABLE tasks ADD COLUMN initial_vars_json TEXT NOT NULL DEFAULT ''",
    )?;
    Ok(())
}

pub(crate) fn m0014_task_graph_debug_tables(conn: &Connection) -> Result<()> {
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

pub(crate) fn m0024_control_plane_audit_peer_exe(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "control_plane_audit",
        "peer_exe",
        "ALTER TABLE control_plane_audit ADD COLUMN peer_exe TEXT",
    )?;
    Ok(())
}

pub(crate) fn m0025_plugin_audit_sandbox_columns(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "plugin_audit",
        "sandbox_profile",
        "ALTER TABLE plugin_audit ADD COLUMN sandbox_profile TEXT",
    )?;
    ensure_column_exists(
        conn,
        "plugin_audit",
        "policy_verdict",
        "ALTER TABLE plugin_audit ADD COLUMN policy_verdict TEXT",
    )?;
    Ok(())
}

pub(crate) fn m0026_add_artifacts_dir(conn: &Connection) -> Result<()> {
    ensure_column_exists(
        conn,
        "tasks",
        "artifacts_dir",
        "ALTER TABLE tasks ADD COLUMN artifacts_dir TEXT NOT NULL DEFAULT ''",
    )?;
    Ok(())
}
