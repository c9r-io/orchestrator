use crate::sqlite::configure_conn;
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
    /// Opens the database and configures paired writer and reader connections.
    ///
    /// The writer connection uses default read-write flags, while the reader
    /// connection is opened read-only to reduce contention.
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();

        // Writer: read-write (default flags)
        let writer = tokio_rusqlite::Connection::open(&db_path)
            .await
            .map_err(flatten_err)?;
        writer
            .call(|conn| configure_conn(conn).map_err(|e| tokio_rusqlite::Error::Other(e.into())))
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
            .call(|conn| configure_conn(conn).map_err(|e| tokio_rusqlite::Error::Other(e.into())))
            .await
            .map_err(flatten_err)?;

        Ok(Self {
            db_path,
            writer,
            reader,
        })
    }

    /// Returns the filesystem path for the database file.
    pub fn path(&self) -> &Path {
        &self.db_path
    }

    /// Returns the write-capable SQLite connection.
    pub fn writer(&self) -> &tokio_rusqlite::Connection {
        &self.writer
    }

    /// Returns the read-only SQLite connection.
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
