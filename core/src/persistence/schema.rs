use crate::migration;
use crate::persistence::sqlite::open_conn;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaStatus {
    pub current_version: u32,
    pub target_version: u32,
    pub pending_versions: Vec<u32>,
    pub pending_names: Vec<&'static str>,
}

impl SchemaStatus {
    pub fn is_current(&self) -> bool {
        self.pending_versions.is_empty()
    }
}

pub struct PersistenceBootstrap;

impl PersistenceBootstrap {
    pub fn ensure_current(db_path: &Path) -> Result<SchemaStatus> {
        let conn = open_conn(db_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )
        .context("failed to configure sqlite wal mode")?;

        let migrations = migration::all_migrations();
        let applied = migration::run_pending(&conn, &migrations)?;
        if applied > 0 {
            tracing::info!(applied, "schema migrations applied");
        }

        Self::status_with_conn(&conn, &migrations)
    }

    pub fn status(db_path: &Path) -> Result<SchemaStatus> {
        let conn = open_conn(db_path)?;
        let migrations = migration::all_migrations();
        Self::status_with_conn(&conn, &migrations)
    }

    fn status_with_conn(
        conn: &rusqlite::Connection,
        migrations: &[migration::Migration],
    ) -> Result<SchemaStatus> {
        let current_version = migration::current_version(conn)?;
        let pending_versions = migrations
            .iter()
            .filter(|migration| migration.version > current_version)
            .map(|migration| migration.version)
            .collect::<Vec<_>>();
        let pending_names = migrations
            .iter()
            .filter(|migration| migration.version > current_version)
            .map(|migration| migration.name)
            .collect::<Vec<_>>();

        Ok(SchemaStatus {
            current_version,
            target_version: migrations
                .last()
                .map(|migration| migration.version)
                .unwrap_or(0),
            pending_versions,
            pending_names,
        })
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
