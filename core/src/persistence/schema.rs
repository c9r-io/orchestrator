use crate::persistence::migration as schema_migration;
pub use crate::persistence::migration::SchemaStatus;
use crate::persistence::sqlite::open_conn;
use anyhow::{Context, Result};
use std::path::Path;

/// Bootstraps the persistence schema and exposes status helpers.
pub struct PersistenceBootstrap;

impl PersistenceBootstrap {
    /// Opens the database, applies pending migrations, and returns the resulting schema status.
    pub fn ensure_current(db_path: &Path) -> Result<SchemaStatus> {
        let conn = open_conn(db_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )
        .context("failed to configure sqlite wal mode")?;

        let migrations = schema_migration::registered_migrations();
        let applied = schema_migration::run_pending(&conn, &migrations)?;
        if !applied.is_empty() {
            tracing::info!(
                applied = applied.count(),
                versions = ?applied.applied.iter().map(|migration| migration.version).collect::<Vec<_>>(),
                "schema migrations applied"
            );
        }

        schema_migration::status(&conn, &migrations)
    }

    /// Returns the current schema status without applying migrations.
    pub fn status(db_path: &Path) -> Result<SchemaStatus> {
        let conn = open_conn(db_path)?;
        schema_migration::registered_status(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_creates_latest_schema_and_reports_current_status() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("schema.db");

        let status = PersistenceBootstrap::ensure_current(&db_path).expect("bootstrap schema");

        assert_eq!(status.current_version, status.target_version);
        assert!(status.is_current());

        let status_after = PersistenceBootstrap::status(&db_path).expect("status");
        assert_eq!(status_after.current_version, status_after.target_version);
        assert!(status_after.pending_versions.is_empty());
    }
}
