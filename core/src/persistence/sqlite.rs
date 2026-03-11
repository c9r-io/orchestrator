use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

pub const SQLITE_BUSY_TIMEOUT_MS: u64 = 5000;

pub fn open_conn(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path).context("failed to open sqlite db")?;
    configure_conn(&conn)?;
    Ok(conn)
}

pub fn configure_conn(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))
        .context("failed to set sqlite busy timeout")?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        "#,
    )
    .context("failed to configure sqlite pragmas")?;
    Ok(())
}
