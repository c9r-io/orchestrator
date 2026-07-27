use crate::migration as schema_migration;
pub use crate::migration::SchemaStatus;
use crate::sqlite::open_conn;
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

/// Whether the `secret_keys` table exists yet, and how many rows it holds.
///
/// Returns `None` when the table is absent, which during bootstrap means the
/// migration chain has not reached it yet rather than that anything is wrong.
pub fn secret_keys_row_count(db_path: &std::path::Path) -> anyhow::Result<Option<i64>> {
    let conn = crate::sqlite::open_conn(db_path)?;
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='secret_keys'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !table_exists {
        return Ok(None);
    }
    Ok(Some(conn.query_row(
        "SELECT COUNT(*) FROM secret_keys",
        [],
        |row| row.get(0),
    )?))
}

/// Which of `key_ids` are still referenced by encrypted SecretStore rows.
///
/// One connection for the whole set rather than one per key: the caller is
/// reporting on crash recovery, and a per-key connection would make the answer
/// depend on how many keys were revoked.
pub fn secret_store_keys_still_referenced(
    db_path: &std::path::Path,
    key_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let conn = crate::sqlite::open_conn(db_path)?;
    let mut referenced = Vec::new();
    for key_id in key_ids {
        if crate::db::secret_store_resources_reference_key(&conn, key_id).unwrap_or(false) {
            referenced.push(key_id.clone());
        }
    }
    Ok(referenced)
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
