//! The registered migration chain, exercised end to end and against partial
//! chains.
//!
//! These tests lived in `core::migration` until FR-141. FR-130 Phase A left
//! them there with a reason — they reach the chain through `crate::db`, the
//! admin facade, "which is core-side until this phase's last commit" — and
//! that reason expired when `db` moved into this crate. Every path they import
//! now resolves inside the layer.
//!
//! They are also what kept `Migration`'s `up` field public. The field is a
//! `fn(&Connection)`, so a struct the layer hands out named the driver in its
//! own definition; the partial-chain tests are the only code that reads it.
//! With them here, the field is `pub(crate)` and the public API no longer
//! mentions a connection type through this struct.

use anyhow::Result;
use rusqlite::Connection;

use crate::async_database::AsyncDatabase;
use crate::db::configure_conn;
use crate::migration::{
    Migration, current_version, registered_migrations as all_migrations, run_pending,
};
use crate::migration_steps::HISTORICAL_AGENT_PLACEHOLDER;
use crate::process_metrics_store::{
    AsyncProcessMetricsRepository, MetricObservation, SUPPORTED_BUCKET_SECONDS,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::tempdir;

/// The registered migration at `version`, for tests that need a partial
/// chain.
///
/// The step functions live in `orchestrator-persistence` and are private to
/// it; the registration list is the supported way to reach one. Building
/// the chain out of the registered entries is also the stronger test — a
/// hand-written `Migration { version, name, up }` asserts against a copy,
/// and a copy stays green when the registration it mirrors changes.
fn registered(version: u32) -> Migration {
    all_migrations()
        .into_iter()
        .find(|migration| migration.version == version)
        .unwrap_or_else(|| panic!("migration {version} is registered"))
}

fn mem_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    configure_conn(&conn).expect("configure conn");
    conn
}

fn file_conn(name: &str) -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let temp = tempdir().expect("create tempdir");
    let db_path = temp.path().join(name);
    let conn = Connection::open(&db_path).expect("open sqlite db file");
    configure_conn(&conn).expect("configure conn");
    (temp, db_path, conn)
}

#[test]
fn run_pending_applies_all_on_fresh_db() {
    let conn = mem_conn();
    let migrations = all_migrations();
    let applied = run_pending(&conn, &migrations).expect("run_pending");
    let latest_version = migrations.last().expect("at least one migration").version;
    assert_eq!(applied.count(), latest_version);
    assert_eq!(current_version(&conn).expect("version"), latest_version);
}

#[test]
fn run_pending_is_idempotent() {
    let conn = mem_conn();
    let migrations = all_migrations();
    run_pending(&conn, &migrations).expect("first run");
    let applied = run_pending(&conn, &migrations).expect("second run");
    let latest_version = migrations.last().expect("at least one migration").version;
    assert_eq!(applied.count(), 0);
    assert_eq!(current_version(&conn).expect("version"), latest_version);
}

#[test]
fn file_backed_blank_database_upgrades_to_latest() {
    let (_temp, _db_path, conn) = file_conn("blank-upgrade.db");
    let migrations = all_migrations();

    let applied = run_pending(&conn, &migrations).expect("upgrade blank db");
    let latest_version = migrations.last().expect("latest migration").version;

    assert_eq!(applied.count(), latest_version);
    assert_eq!(current_version(&conn).expect("version"), latest_version);
}

#[test]
fn file_backed_mid_schema_database_upgrades_to_latest() {
    let (_temp, _db_path, conn) = file_conn("mid-schema-upgrade.db");
    let migrations = all_migrations();
    let mid: Vec<Migration> = migrations
        .iter()
        .filter(|migration| migration.version <= 8)
        .map(|migration| Migration {
            version: migration.version,
            name: migration.name,
            up: migration.up,
        })
        .collect();
    run_pending(&conn, &mid).expect("seed mid-schema db");
    assert_eq!(current_version(&conn).expect("mid version"), 8);

    let applied = run_pending(&conn, &migrations).expect("upgrade mid-schema db");
    let latest_version = migrations.last().expect("latest migration").version;

    assert_eq!(applied.count(), latest_version - 8);
    assert_eq!(
        current_version(&conn).expect("latest version"),
        latest_version
    );
}

#[test]
fn file_backed_partial_upgrade_database_recovers_to_latest() {
    let (_temp, _db_path, conn) = file_conn("partial-upgrade.db");
    let migrations = all_migrations();
    run_pending(&conn, &migrations).expect("seed latest schema");
    let latest_version = migrations.last().expect("latest migration").version;
    conn.execute(
        "DELETE FROM schema_migrations WHERE version = ?1",
        rusqlite::params![latest_version],
    )
    .expect("rewind latest schema record only");

    assert_eq!(
        current_version(&conn).expect("partial version"),
        latest_version - 1
    );

    let applied = run_pending(&conn, &migrations).expect("recover partial upgrade");
    assert_eq!(applied.count(), 1);
    assert_eq!(
        current_version(&conn).expect("recovered version"),
        latest_version
    );
}

#[test]
fn file_backed_current_database_is_noop() {
    let (_temp, _db_path, conn) = file_conn("current-schema.db");
    let migrations = all_migrations();
    run_pending(&conn, &migrations).expect("seed latest schema");
    let latest_version = migrations.last().expect("latest migration").version;

    let applied = run_pending(&conn, &migrations).expect("rerun current db");

    assert_eq!(applied.count(), 0);
    assert_eq!(current_version(&conn).expect("version"), latest_version);
}

#[test]
fn populated_v30_database_upgrades_with_action_audit_links() {
    let (_temp, _db_path, conn) = file_conn("populated-v30-action-audit.db");
    let migrations = all_migrations();
    let through_v30 = migrations
        .iter()
        .take_while(|migration| migration.version <= 30)
        .map(|migration| Migration {
            version: migration.version,
            name: migration.name,
            up: migration.up,
        })
        .collect::<Vec<_>>();
    run_pending(&conn, &through_v30).expect("seed v30");
    conn.execute(
        "INSERT INTO control_plane_audit
         (created_at,transport,rpc,authn_result,authz_result)
         VALUES('2026-07-14T00:00:00Z','uds','AttentionClaim','authenticated','allowed')",
        [],
    )
    .expect("populate audit row");

    assert_eq!(run_pending(&conn, &migrations).expect("upgrade").count(), 7);
    let preserved: i64 = conn
        .query_row("SELECT COUNT(*) FROM control_plane_audit", [], |row| {
            row.get(0)
        })
        .expect("preserved row");
    assert_eq!(preserved, 1);
    let canonical_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='control_action_audit'",
            [],
            |row| row.get(0),
        )
        .expect("canonical table");
    assert_eq!(canonical_exists, 1);
    for table in [
        "control_plane_audit",
        "attention_actions",
        "resume_executions",
        "session_control_actions",
        "source_command_actions",
        "source_events",
        "source_bindings",
        "events",
    ] {
        let count: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='request_id'"
                ),
                [],
                |row| row.get(0),
            )
            .expect("request_id column");
        assert_eq!(count, 1, "missing request_id on {table}");
    }
}

#[tokio::test]
async fn populated_v26_process_console_upgrade_preserves_entities_and_rebuilds_metrics() {
    let (_temp, db_path, conn) = file_conn("populated-v26-process-console.db");
    let migrations = all_migrations();
    run_pending(&conn, &migrations[..26]).expect("seed console predecessor schema");
    conn.execute_batch(
        r#"
        INSERT INTO tasks
        (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,project_id,
         workspace_root,qa_targets_json,ticket_dir,created_at,updated_at)
        VALUES('console-task','console task','failed','preserve me','[]','once','default',
               'fixture','console-project','/tmp/console','[]','docs/ticket',
               '2026-07-15T00:00:00Z','2026-07-15T00:00:00Z');
        INSERT INTO events(task_id,event_type,payload_json,created_at)
        VALUES('console-task','step_failed','{}','2026-07-15T00:00:01Z');
        INSERT INTO agent_sessions
        (id,task_id,step_id,phase,agent_id,state,pid,pty_backend,cwd,command,
         input_fifo_path,stdout_path,stderr_path,transcript_path,created_at,updated_at)
        VALUES('console-session','console-task','implement','implement','fixture','exited',0,
               'script','/private/work','private command','/private/input','/private/stdout',
               '/private/stderr','/private/transcript','2026-07-15T00:00:00Z',
               '2026-07-15T00:00:00Z');
        "#,
    )
    .expect("seed pre-console task, event, and session");

    run_pending(&conn, &migrations[..27]).expect("apply attention migration");
    conn.execute_batch(
        r#"
        INSERT INTO attention_items
        (id,project_id,task_id,kind,severity,state,title,summary,actions_json,dedupe_key,
         source_event_id,created_at,updated_at,last_occurred_at)
        VALUES('console-attention','console-project','console-task','step_failed','high','open',
               'fixture failure','bounded summary','[]','console-dedupe','event-1',
               '2026-07-15T00:00:01Z','2026-07-15T00:00:01Z','2026-07-15T00:00:01Z');
        INSERT INTO attention_actions
        (attention_item_id,actor,mutation_kind,idempotency_key,request_hash,target_version,
         status,created_at)
        VALUES('console-attention','operator','claim','console-claim','hash',1,'succeeded',
               '2026-07-15T00:00:02Z');
        INSERT INTO attention_changes
        (attention_item_id,change_kind,item_version,created_at)
        VALUES('console-attention','open',1,'2026-07-15T00:00:01Z');
        "#,
    )
    .expect("seed attention data");

    run_pending(&conn, &migrations[..28]).expect("apply handoff migration");
    conn.execute(
        "INSERT INTO handoff_snapshots
         (id,project_id,task_id,source_event_cursor,projection_version,briefing_json,
          content_hash,state_version,generated_by,created_at)
         VALUES('console-handoff','console-project','console-task',1,1,'{}','content-hash',
                'state-v1','operator','2026-07-15T00:00:03Z')",
        [],
    )
    .expect("seed handoff");

    run_pending(&conn, &migrations[..29]).expect("apply session control migration");
    conn.execute(
        "INSERT INTO session_control_actions
         (session_id,actor,client_id,action,idempotency_key,request_hash,result,created_at)
         VALUES('console-session','operator','writer','writer_attach','console-session-action',
                'hash','succeeded','2026-07-15T00:00:04Z')",
        [],
    )
    .expect("seed session action");

    run_pending(&conn, &migrations[..30]).expect("apply source migration");
    conn.execute_batch(
        r#"
        INSERT INTO source_events
        (id,project_id,provider,installation_id,external_event_id,event_type,occurred_at,
         received_at,normalized_payload_json,payload_hash,routing_state,routed_task_id)
        VALUES('console-source','console-project','fixture','installation','external-1','message',
               '2026-07-15T00:00:05Z','2026-07-15T00:00:05Z','{}','payload-hash','routed',
               'console-task');
        INSERT INTO source_bindings
        (id,project_id,task_id,provider,installation_id,conversation_id,correlation_key,
         binding_type,created_by_event_id,created_at)
        VALUES('console-binding','console-project','console-task','fixture','installation',
               'conversation','correlation','conversation','console-source',
               '2026-07-15T00:00:05Z');
        "#,
    )
    .expect("seed source event and binding");

    run_pending(&conn, &migrations[..31]).expect("apply action audit migration");
    conn.execute_batch(
        r#"
        INSERT INTO control_action_audit
        (request_id,project_id,actor,resolved_role,transport,target_type,target_id,action,
         reason_code,idempotency_key,request_hash,status,created_at,updated_at,completed_at)
        VALUES('req-console','console-project','operator','operator','uds','attention',
               'console-attention','attention.claim','accepted','console-claim','hash','succeeded',
               '2026-07-15T00:00:02Z','2026-07-15T00:00:02Z','2026-07-15T00:00:02Z');
        UPDATE attention_actions SET request_id='req-console'
        WHERE attention_item_id='console-attention';
        UPDATE session_control_actions SET request_id='req-console'
        WHERE session_id='console-session';
        UPDATE source_events SET request_id='req-console' WHERE id='console-source';
        UPDATE source_bindings SET request_id='req-console' WHERE id='console-binding';
        UPDATE events SET request_id='req-console' WHERE task_id='console-task';
        "#,
    )
    .expect("seed canonical audit joins");

    assert_eq!(
        run_pending(&conn, &migrations)
            .expect("upgrade to latest")
            .count(),
        6
    );
    assert_eq!(current_version(&conn).expect("latest version"), 37);
    for (table, id) in [
        ("tasks", "console-task"),
        ("agent_sessions", "console-session"),
        ("attention_items", "console-attention"),
        ("handoff_snapshots", "console-handoff"),
        ("source_events", "console-source"),
        ("source_bindings", "console-binding"),
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE id=?1"),
                [id],
                |row| row.get(0),
            )
            .expect("query preserved entity");
        assert_eq!(count, 1, "{table}.{id} was not preserved");
    }
    let session: (String, i64) = conn
        .query_row(
            "SELECT state,state_version FROM agent_sessions WHERE id='console-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query migrated session");
    assert_eq!(session, ("closed".to_string(), 1));
    let joined: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM control_action_audit a
             JOIN attention_actions aa ON aa.request_id=a.request_id
             JOIN session_control_actions sa ON sa.request_id=a.request_id
             JOIN source_bindings sb ON sb.request_id=a.request_id
             JOIN events e ON e.request_id=a.request_id
             WHERE a.request_id='req-console'",
            [],
            |row| row.get(0),
        )
        .expect("query preserved audit joins");
    assert_eq!(joined, 1);
    let attention_project: String = conn
        .query_row(
            "SELECT project_id FROM attention_changes WHERE attention_item_id='console-attention'",
            [],
            |row| row.get(0),
        )
        .expect("query attention change backfill");
    assert_eq!(attention_project, "console-project");

    drop(conn);
    let async_db = Arc::new(
        AsyncDatabase::open(&db_path)
            .await
            .expect("open upgraded db"),
    );
    let metrics = AsyncProcessMetricsRepository::new(Arc::clone(&async_db));
    assert!(
        metrics
            .record(MetricObservation {
                project_id: "console-project".to_string(),
                metric_name: "timeline_projection_seconds".to_string(),
                dimensions: BTreeMap::new(),
                value: 0.25,
                occurred_at: "2026-07-15T00:00:06Z".to_string(),
                source_kind: "release_fixture".to_string(),
                source_key: "console-upgrade".to_string(),
            })
            .await
            .expect("record post-upgrade metric")
    );
    async_db
        .writer()
        .call(|conn| {
            conn.execute(
                "DELETE FROM process_metric_rollups WHERE project_id='console-project'",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("clear rebuildable rollups");
    assert_eq!(
        metrics
            .rebuild("console-project")
            .await
            .expect("rebuild metrics"),
        1
    );
    let rollups: i64 = async_db
        .reader()
        .call(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM process_metric_rollups
                 WHERE project_id='console-project'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .expect("query rebuilt rollups");
    assert_eq!(rollups, SUPPORTED_BUCKET_SECONDS.len() as i64);
}

#[test]
fn populated_v33_source_automation_upgrade_preserves_route_and_provenance() {
    let (_temp, _db_path, conn) = file_conn("populated-v33-source-automation.db");
    let migrations = all_migrations();
    run_pending(&conn, &migrations[..33]).expect("seed source automation route schema");
    conn.execute_batch(
        r#"
        INSERT INTO tasks
        (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,project_id,
         workspace_root,qa_targets_json,ticket_dir,created_at,updated_at)
        VALUES('release-task','release task','completed','bounded goal','[]','once',
               'release-workspace','release-workflow','release-project','/tmp/release',
               '[]','docs/ticket','2026-07-17T00:00:00Z','2026-07-17T00:00:10Z');
        INSERT INTO source_events
        (id,project_id,provider,installation_id,external_event_id,event_type,occurred_at,
         received_at,normalized_payload_json,payload_hash,routing_state,routed_task_id,
         request_id,automation_route_id)
        VALUES('release-source','release-project','slack','T_RELEASE','Ev-release',
               'reaction_added','2026-07-17T00:00:01Z','2026-07-17T00:00:01Z',
               '{"kind":"reaction_added"}','payload-hash','routed','release-task',
               'req-release','release-route');
        INSERT INTO source_bindings
        (id,project_id,task_id,provider,installation_id,conversation_id,correlation_key,
         binding_type,created_by_event_id,request_id,created_at)
        VALUES('release-binding','release-project','release-task','slack','T_RELEASE',
               'C_RELEASE:1.0','automation-key','automation','release-source','req-release',
               '2026-07-17T00:00:02Z');
        INSERT INTO control_action_audit
        (request_id,project_id,actor,resolved_role,transport,target_type,target_id,action,
         reason_code,idempotency_key,request_hash,status,result_id,created_at,updated_at,
         completed_at)
        VALUES('req-release','release-project','source-router','system','internal','task',
               'release-task','source.automation.create_task','accepted','automation-key',
               'request-hash','succeeded','release-task','2026-07-17T00:00:02Z',
               '2026-07-17T00:00:02Z','2026-07-17T00:00:02Z');
        INSERT INTO source_automation_routes
        (id,project_id,automation_key,source_event_id,provider,installation_id,
         message_identity,channel_id,message_ts,reaction,resolved_role,binding_name,
         binding_revision,template_name,template_hash,binding_snapshot_json,
         template_snapshot_json,credential_store,credential_key,permalink_status,permalink,
         request_id,deterministic_task_id,task_id,status,created_at,updated_at,completed_at)
        VALUES('release-route','release-project','automation-key','release-source','slack',
               'T_RELEASE','C_RELEASE:1.0','C_RELEASE','1.0','agent-implement','operator',
               'slack-implement','binding-revision','implement-from-slack','template-hash',
               '{"name":"slack-implement"}','{"name":"implement-from-slack"}',
               'slack-release-secret','bot-token','resolved',
               'https://example.invalid/release','req-release','release-task','release-task',
               'completed','2026-07-17T00:00:01Z','2026-07-17T00:00:02Z',
               '2026-07-17T00:00:02Z');
        "#,
    )
    .expect("seed populated source automation route");

    assert_eq!(
        run_pending(&conn, &migrations)
            .expect("upgrade to v37")
            .count(),
        4
    );
    assert_eq!(current_version(&conn).expect("latest version"), 37);
    let route: (String, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT status,generation,version,attempt_count,max_attempts
             FROM source_automation_routes WHERE id='release-route'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("query upgraded route");
    assert_eq!(route, ("routed".to_string(), 1, 1, 0, 5));
    let generation: (String, String, String, String) = conn
        .query_row(
            "SELECT binding_name,template_name,request_id,deterministic_task_id
             FROM source_automation_route_generations
             WHERE route_id='release-route' AND generation=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("query frozen generation");
    assert_eq!(
        generation,
        (
            "slack-implement".to_string(),
            "implement-from-slack".to_string(),
            "req-release".to_string(),
            "release-task".to_string(),
        )
    );
    let change: (i64, String) = conn
        .query_row(
            "SELECT route_version,state FROM source_automation_route_changes
             WHERE route_id='release-route'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query route change backfill");
    assert_eq!(change, (1, "routed".to_string()));
    let provenance_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM source_automation_routes r
             JOIN source_events e ON e.id=r.source_event_id
             JOIN source_bindings b ON b.task_id=r.task_id
             JOIN tasks t ON t.id=r.task_id
             JOIN control_action_audit a ON a.request_id=r.request_id
             WHERE r.id='release-route' AND e.automation_route_id=r.id
               AND b.binding_type='automation' AND a.result_id=t.id",
            [],
            |row| row.get(0),
        )
        .expect("query preserved provenance");
    assert_eq!(provenance_count, 1);
}

#[test]
fn populated_v34_upgrade_adds_source_connections_and_dedicated_checkpoints() {
    let (_temp, _db_path, conn) = file_conn("populated-v34-source-connections.db");
    let migrations = all_migrations();
    run_pending(&conn, &migrations[..34]).expect("seed v34 schema");
    conn.execute(
        "INSERT INTO tasks
         (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,project_id,
          workspace_root,qa_targets_json,ticket_dir,created_at,updated_at)
         VALUES('pre-connection-task','preserved task','completed','bounded goal','[]','once',
                'workspace-a','workflow-a','project-a','/tmp/project-a','[]','docs/ticket',
                '2026-07-18T00:00:00Z','2026-07-18T00:00:01Z')",
        [],
    )
    .expect("seed populated v34 task");

    assert_eq!(
        run_pending(&conn, &migrations)
            .expect("upgrade to v37")
            .count(),
        3
    );
    assert_eq!(current_version(&conn).expect("latest version"), 37);
    let task: (String, String) = conn
        .query_row(
            "SELECT project_id,status FROM tasks WHERE id='pre-connection-task'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query preserved task");
    assert_eq!(task, ("project-a".to_string(), "completed".to_string()));
    for table in [
        "source_daemon_identity",
        "source_connections",
        "source_connection_intents",
        "source_connection_changes",
        "source_connection_provisioning",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("query source connection table");
        assert_eq!(count, 1, "missing source connection table {table}");
    }
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
    assert_eq!(applied.count(), 2);
    assert_eq!(current_version(&conn).expect("version"), 2);

    // Apply the full set after running the first two — only the remainder should execute.
    let applied = run_pending(&conn, &all).expect("full run");
    let latest_version = all.last().expect("at least one migration").version;
    assert_eq!(applied.count(), latest_version - 2);
    assert_eq!(current_version(&conn).expect("version"), latest_version);
}

#[test]
fn failed_migration_does_not_advance_version() {
    let conn = mem_conn();

    fn fail_migration(_conn: &Connection) -> Result<()> {
        anyhow::bail!("intentional failure");
    }

    // Run migration 1 first so we have tables
    let first = vec![registered(1)];
    run_pending(&conn, &first).expect("first migration");

    let bad = vec![
        registered(1),
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
    let migrations = vec![registered(1)];
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
    let migrations = vec![registered(1)];
    run_pending(&conn, &migrations).expect("baseline on existing db");
}

#[test]
fn latest_schema_contains_source_routing_tables() {
    let conn = mem_conn();
    run_pending(&conn, &all_migrations()).expect("run migrations");
    for table in [
        "source_events",
        "source_bindings",
        "source_routing_attempts",
        "source_command_actions",
    ] {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("query source table");
        assert_eq!(present, 1, "missing table {table}");
    }
    let claimed_at: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('source_events') WHERE name='routing_claimed_at'",
            [],
            |row| row.get(0),
        )
        .expect("query source column");
    assert_eq!(claimed_at, 1);
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
    )
    .expect("insert event 1");
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

    let normalize = vec![registered(9)];

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
        37,
        "expected 37 migrations, got {}",
        migrations.len()
    );
}
