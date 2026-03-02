use crate::db::configure_conn;
use anyhow::{Context, Result};
use r2d2::{CustomizeConnection, Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_POOL_MAX_SIZE: u32 = 12;
const DEFAULT_POOL_MIN_IDLE: u32 = 1;
const DEFAULT_POOL_CONNECTION_TIMEOUT_MS: u64 = 2000;

type SqlitePool = Pool<SqliteConnectionManager>;
type SqlitePooledConnection = PooledConnection<SqliteConnectionManager>;

#[derive(Debug)]
struct SqliteConnectionCustomizer;

impl CustomizeConnection<Connection, rusqlite::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        configure_conn(conn).map_err(|err| match err.downcast::<rusqlite::Error>() {
            Ok(db_err) => db_err,
            Err(other) => rusqlite::Error::ToSqlConversionFailure(other.into()),
        })
    }
}

#[derive(Clone)]
pub struct Database {
    db_path: PathBuf,
    pool: SqlitePool,
}

impl Database {
    pub fn new<P: Into<PathBuf>>(db_path: P) -> Result<Self> {
        let db_path = db_path.into();
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder()
            .max_size(DEFAULT_POOL_MAX_SIZE)
            .min_idle(Some(DEFAULT_POOL_MIN_IDLE))
            .connection_timeout(Duration::from_millis(DEFAULT_POOL_CONNECTION_TIMEOUT_MS))
            .test_on_check_out(true)
            .connection_customizer(Box::new(SqliteConnectionCustomizer))
            .build(manager)
            .with_context(|| format!("failed to create sqlite pool for {}", db_path.display()))?;
        Ok(Self { db_path, pool })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    pub fn connection(&self) -> Result<SqlitePooledConnection> {
        self.pool
            .get()
            .with_context(|| format!("db pool checkout timed out for {}", self.db_path.display()))
    }

    pub fn pool_max_size(&self) -> u32 {
        DEFAULT_POOL_MAX_SIZE
    }

    pub fn pool_min_idle(&self) -> u32 {
        DEFAULT_POOL_MIN_IDLE
    }

    pub fn pool_connection_timeout_ms(&self) -> u64 {
        DEFAULT_POOL_CONNECTION_TIMEOUT_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    #[test]
    fn pooled_connections_apply_sqlite_configuration() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("pool.db");
        init_schema(&db_path).expect("init schema");

        let db = Database::new(db_path).expect("create pool");
        let conn = db.connection().expect("checkout connection");
        let busy_timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .expect("busy timeout");
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .expect("foreign_keys");

        assert_eq!(busy_timeout_ms, 5000);
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn pooled_connection_exposes_static_pool_settings() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("pool-config.db");
        init_schema(&db_path).expect("init schema");

        let db = Database::new(db_path).expect("create pool");

        assert_eq!(db.pool_max_size(), 12);
        assert_eq!(db.pool_min_idle(), 1);
        assert_eq!(db.pool_connection_timeout_ms(), 2000);
    }
}
