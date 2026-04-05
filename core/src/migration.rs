use anyhow::Result;
use rusqlite::Connection;

pub use crate::persistence::migration::Migration;

/// Returns the current schema version (0 if no migrations have run).
pub fn current_version(conn: &Connection) -> Result<u32> {
    crate::persistence::migration::current_version(conn)
}

/// Run all pending migrations. Returns the number of migrations applied.
pub fn run_pending(conn: &Connection, migrations: &[Migration]) -> Result<u32> {
    crate::persistence::migration::run_pending(conn, migrations).map(|summary| summary.count())
}

/// All registered migrations in version order.
pub fn all_migrations() -> Vec<Migration> {
    crate::persistence::migration::registered_migrations()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::configure_conn;
    use crate::persistence::migration_steps::{
        HISTORICAL_AGENT_PLACEHOLDER, m0001_baseline_schema, m0009_normalize_unspecified_agent_ids,
    };
    use tempfile::tempdir;

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
    fn file_backed_blank_database_upgrades_to_latest() {
        let (_temp, _db_path, conn) = file_conn("blank-upgrade.db");
        let migrations = all_migrations();

        let applied = run_pending(&conn, &migrations).expect("upgrade blank db");
        let latest_version = migrations.last().expect("latest migration").version;

        assert_eq!(applied, latest_version);
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

        assert_eq!(applied, latest_version - 8);
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
        assert_eq!(applied, 1);
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
            24,
            "expected 24 migrations, got {}",
            migrations.len()
        );
    }
}
