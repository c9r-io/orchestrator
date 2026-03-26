use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashSet;

/// Describes a schema migration that can be applied to a persistence database.
pub struct Migration {
    /// Monotonic schema version assigned to the migration.
    pub version: u32,
    /// Stable migration identifier recorded in `schema_migrations`.
    pub name: &'static str,
    /// Migration function executed inside a transaction.
    pub up: fn(&Connection) -> Result<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Public metadata for a registered migration.
pub struct MigrationDescriptor {
    /// Schema version introduced by the migration.
    pub version: u32,
    /// Stable migration identifier.
    pub name: &'static str,
}

impl MigrationDescriptor {
    fn from_migration(migration: &Migration) -> Self {
        Self {
            version: migration.version,
            name: migration.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Reports current and target schema versions plus the migrations still pending.
pub struct SchemaStatus {
    /// Highest schema version already applied to the database.
    pub current_version: u32,
    /// Highest schema version known to the running binary.
    pub target_version: u32,
    /// Ordered list of pending schema versions.
    pub pending_versions: Vec<u32>,
    /// Ordered list of pending migration names.
    pub pending_names: Vec<&'static str>,
}

impl SchemaStatus {
    /// Returns `true` when the database is already at the latest registered version.
    pub fn is_current(&self) -> bool {
        self.pending_versions.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Summarizes which migrations were applied during a `run_pending` invocation.
pub struct AppliedMigrationSummary {
    /// Ordered descriptors for migrations applied in the current run.
    pub applied: Vec<MigrationDescriptor>,
}

impl AppliedMigrationSummary {
    /// Returns the number of migrations applied in the current run.
    pub fn count(&self) -> u32 {
        self.applied.len() as u32
    }

    /// Returns `true` when no migrations were applied.
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }
}

fn ensure_schema_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
    )
    .context("failed to create schema_migrations table")?;
    Ok(())
}

/// Returns the highest applied schema version for the given database.
pub fn current_version(conn: &Connection) -> Result<u32> {
    ensure_schema_migrations_table(conn)?;

    let version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .context("failed to read current schema version")?;
    Ok(version)
}

/// Returns the full ordered list of schema migrations known to this binary.
pub fn registered_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            name: "m0001_baseline_schema",
            up: crate::persistence::migration_steps::m0001_baseline_schema,
        },
        Migration {
            version: 2,
            name: "m0002_backfill_legacy_defaults",
            up: crate::persistence::migration_steps::m0002_backfill_historical_defaults,
        },
        Migration {
            version: 3,
            name: "m0003_events_promote_columns",
            up: crate::persistence::migration_steps::m0003_events_promote_columns,
        },
        Migration {
            version: 4,
            name: "m0004_events_backfill_promoted",
            up: crate::persistence::migration_steps::m0004_events_backfill_promoted,
        },
        Migration {
            version: 5,
            name: "m0005_add_task_lookup_indexes",
            up: crate::persistence::migration_steps::m0005_add_task_lookup_indexes,
        },
        Migration {
            version: 6,
            name: "m0006_add_pipeline_vars_json",
            up: crate::persistence::migration_steps::m0006_add_pipeline_vars_json,
        },
        Migration {
            version: 7,
            name: "m0007_workflow_store_entries",
            up: crate::persistence::migration_steps::m0007_workflow_store_entries,
        },
        Migration {
            version: 8,
            name: "m0008_workflow_primitives",
            up: crate::persistence::migration_steps::m0008_workflow_primitives,
        },
        Migration {
            version: 9,
            name: "m0009_normalize_unspecified_agent_ids",
            up: crate::persistence::migration_steps::m0009_normalize_unspecified_agent_ids,
        },
        Migration {
            version: 10,
            name: "m0010_per_resource_persistence",
            up: crate::persistence::migration_steps::m0010_per_resource_persistence,
        },
        Migration {
            version: 11,
            name: "m0011_finalize_resource_migration",
            up: crate::persistence::migration_steps::m0011_finalize_resource_migration,
        },
        Migration {
            version: 12,
            name: "m0012_drop_legacy_orchestrator_config_blob",
            up: crate::persistence::migration_steps::m0012_drop_legacy_orchestrator_config_blob,
        },
        Migration {
            version: 13,
            name: "m0013_control_plane_audit",
            up: crate::persistence::migration_steps::m0013_control_plane_audit,
        },
        Migration {
            version: 14,
            name: "m0014_task_graph_debug_tables",
            up: crate::persistence::migration_steps::m0014_task_graph_debug_tables,
        },
        Migration {
            version: 15,
            name: "m0015_control_plane_audit_rejection_stage",
            up: crate::persistence::migration_steps::m0015_control_plane_audit_rejection_stage,
        },
        Migration {
            version: 16,
            name: "m0016_secret_key_lifecycle",
            up: crate::persistence::migration_steps::m0016_secret_key_lifecycle,
        },
        Migration {
            version: 17,
            name: "m0017_control_plane_protection_fields",
            up: crate::persistence::migration_steps::m0017_control_plane_protection_fields,
        },
        Migration {
            version: 18,
            name: "m0018_trigger_state",
            up: crate::persistence::migration_steps::m0018_trigger_state,
        },
        Migration {
            version: 19,
            name: "m0019_daemon_incarnation",
            up: crate::persistence::migration_steps::m0019_daemon_incarnation,
        },
        Migration {
            version: 20,
            name: "m0020_command_template_column",
            up: crate::persistence::migration_steps::m0020_command_template_column,
        },
        Migration {
            version: 21,
            name: "m0021_command_rule_index_column",
            up: crate::persistence::migration_steps::m0021_command_rule_index_column,
        },
    ]
}

/// Converts migration definitions into lightweight public descriptors.
pub fn descriptors(migrations: &[Migration]) -> Vec<MigrationDescriptor> {
    migrations
        .iter()
        .map(MigrationDescriptor::from_migration)
        .collect()
}

/// Returns descriptors for every registered migration.
pub fn registered_descriptors() -> Vec<MigrationDescriptor> {
    descriptors(&registered_migrations())
}

/// Computes the database schema status against the provided migration set.
pub fn status(conn: &Connection, migrations: &[Migration]) -> Result<SchemaStatus> {
    let current_version = current_version(conn)?;
    let descriptors = descriptors(migrations);
    let pending = descriptors
        .iter()
        .filter(|migration| migration.version > current_version)
        .copied()
        .collect::<Vec<_>>();

    Ok(SchemaStatus {
        current_version,
        target_version: descriptors
            .last()
            .map(|migration| migration.version)
            .unwrap_or(0),
        pending_versions: pending.iter().map(|migration| migration.version).collect(),
        pending_names: pending.iter().map(|migration| migration.name).collect(),
    })
}

/// Computes the database schema status against all registered migrations.
pub fn registered_status(conn: &Connection) -> Result<SchemaStatus> {
    let migrations = registered_migrations();
    status(conn, &migrations)
}

/// Returns every schema version already recorded in `schema_migrations`.
pub fn applied_versions(conn: &Connection) -> Result<Vec<u32>> {
    ensure_schema_migrations_table(conn)?;

    let mut stmt = conn
        .prepare("SELECT version FROM schema_migrations ORDER BY version ASC")
        .context("failed to prepare applied schema versions query")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, u32>(0))
        .context("failed to query applied schema versions")?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to collect applied schema versions")
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Indicates whether each registered migration has already been applied.
pub struct RegisteredMigrationStatus {
    /// Schema version represented by this row.
    pub version: u32,
    /// Stable migration identifier.
    pub name: &'static str,
    /// Whether the version is present in `schema_migrations`.
    pub applied: bool,
}

/// Returns the applied status for every registered migration.
pub fn registered_migration_statuses(conn: &Connection) -> Result<Vec<RegisteredMigrationStatus>> {
    let applied = applied_versions(conn)?.into_iter().collect::<HashSet<_>>();
    Ok(registered_descriptors()
        .into_iter()
        .map(|descriptor| RegisteredMigrationStatus {
            version: descriptor.version,
            name: descriptor.name,
            applied: applied.contains(&descriptor.version),
        })
        .collect())
}

/// Applies every migration newer than the current schema version.
pub fn run_pending(conn: &Connection, migrations: &[Migration]) -> Result<AppliedMigrationSummary> {
    let current = current_version(conn)?;
    let mut applied = Vec::new();

    for migration in migrations {
        if migration.version <= current {
            continue;
        }

        let tx = conn.unchecked_transaction().with_context(|| {
            format!(
                "failed to begin transaction for migration {}",
                migration.name
            )
        })?;

        (migration.up)(&tx).with_context(|| format!("migration {} failed", migration.name))?;

        tx.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![migration.version, migration.name],
        )
        .with_context(|| format!("failed to record migration version {}", migration.version))?;

        tx.commit()
            .with_context(|| format!("failed to commit migration {}", migration.name))?;

        applied.push(MigrationDescriptor::from_migration(migration));
    }

    Ok(AppliedMigrationSummary { applied })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_status_reports_pending_for_blank_database() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");

        let status = registered_status(&conn).expect("registered status");

        assert_eq!(status.current_version, 0);
        assert_eq!(
            status.target_version,
            registered_descriptors()
                .last()
                .expect("at least one migration")
                .version
        );
        assert!(!status.is_current());
        assert_eq!(
            status.pending_versions.len(),
            registered_descriptors().len()
        );
    }

    #[test]
    fn run_pending_summary_reports_applied_descriptors() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        let migrations = vec![Migration {
            version: 1,
            name: "m0001_test_only",
            up: |_conn| Ok(()),
        }];

        let summary = run_pending(&conn, &migrations).expect("run pending");

        assert_eq!(summary.count(), 1);
        assert!(!summary.is_empty());
        assert_eq!(
            summary.applied,
            vec![MigrationDescriptor {
                version: 1,
                name: "m0001_test_only",
            }]
        );
    }
}
