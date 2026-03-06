use crate::db::configure_conn;
use anyhow::Result;
use rusqlite::OpenFlags;
use std::path::{Path, PathBuf};

/// Async wrapper around SQLite using `tokio_rusqlite`.
///
/// Uses two named connections (not a pool):
/// - **writer**: all write operations, serialized to match SQLite WAL single-writer model
/// - **reader**: read-only queries, avoids contention with writer lock
#[derive(Clone)]
pub struct AsyncDatabase {
    db_path: PathBuf,
    writer: tokio_rusqlite::Connection,
    reader: tokio_rusqlite::Connection,
}

impl AsyncDatabase {
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();

        // Writer: read-write (default flags)
        let writer = tokio_rusqlite::Connection::open(&db_path)
            .await
            .map_err(flatten_err)?;
        writer
            .call(|conn| {
                configure_conn(conn).map_err(|e| {
                    tokio_rusqlite::Error::Other(e.into())
                })
            })
            .await
            .map_err(flatten_err)?;

        // Reader: read-only
        let reader = tokio_rusqlite::Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .await
        .map_err(flatten_err)?;
        reader
            .call(|conn| {
                configure_conn(conn).map_err(|e| {
                    tokio_rusqlite::Error::Other(e.into())
                })
            })
            .await
            .map_err(flatten_err)?;

        Ok(Self {
            db_path,
            writer,
            reader,
        })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    pub fn writer(&self) -> &tokio_rusqlite::Connection {
        &self.writer
    }

    pub fn reader(&self) -> &tokio_rusqlite::Connection {
        &self.reader
    }
}

/// Flatten `tokio_rusqlite::Error` into `anyhow::Error`.
pub fn flatten_err(err: tokio_rusqlite::Error) -> anyhow::Error {
    match err {
        tokio_rusqlite::Error::ConnectionClosed => anyhow::anyhow!("db connection closed"),
        tokio_rusqlite::Error::Close((_, e)) => e.into(),
        tokio_rusqlite::Error::Rusqlite(e) => e.into(),
        tokio_rusqlite::Error::Other(e) => anyhow::anyhow!(e),
        _ => anyhow::anyhow!("unknown tokio-rusqlite error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use tempfile::tempdir;

    #[tokio::test]
    async fn async_database_open_and_configure() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("async_test.db");
        init_schema(&db_path).expect("init schema");

        let db = AsyncDatabase::open(&db_path).await.expect("open async db");
        assert_eq!(db.path(), db_path);

        // Verify writer pragmas
        let busy_timeout: i64 = db
            .writer()
            .call(|conn| {
                conn.query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
                    .map_err(|e| e.into())
            })
            .await
            .expect("query busy_timeout");
        assert_eq!(busy_timeout, 5000);

        let foreign_keys: i64 = db
            .writer()
            .call(|conn| {
                conn.query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
                    .map_err(|e| e.into())
            })
            .await
            .expect("query foreign_keys");
        assert_eq!(foreign_keys, 1);
    }

    #[tokio::test]
    async fn async_database_read_write_roundtrip() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("rw_test.db");
        init_schema(&db_path).expect("init schema");

        let db = AsyncDatabase::open(&db_path).await.expect("open async db");

        // Write via writer
        db.writer()
            .call(|conn| {
                conn.execute(
                    "INSERT INTO events (task_id, event_type, payload_json, created_at) VALUES ('t1', 'test', '{}', '2026-01-01')",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("write event");

        // Read via reader
        let count: i64 = db
            .reader()
            .call(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?)
            })
            .await
            .expect("read count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn async_database_clone_shares_connections() {
        let temp = tempdir().expect("temp dir");
        let db_path = temp.path().join("clone_test.db");
        init_schema(&db_path).expect("init schema");

        let db = AsyncDatabase::open(&db_path).await.expect("open async db");
        let db2 = db.clone();

        // Write through clone
        db2.writer()
            .call(|conn| {
                conn.execute(
                    "INSERT INTO events (task_id, event_type, payload_json, created_at) VALUES ('t1', 'test', '{}', '2026-01-01')",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("write via clone");

        // Read through original
        let count: i64 = db
            .reader()
            .call(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?)
            })
            .await
            .expect("read via original");
        assert_eq!(count, 1);
    }
}
