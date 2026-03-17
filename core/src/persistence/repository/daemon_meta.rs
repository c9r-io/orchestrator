use anyhow::Result;
use std::sync::Arc;

use crate::async_database::{flatten_err, AsyncDatabase};

/// Increment the persistent daemon incarnation counter and return the new value.
pub async fn increment_incarnation(db: &Arc<AsyncDatabase>) -> Result<u64> {
    db.writer()
        .call(|conn| {
            conn.execute(
                "UPDATE daemon_meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'incarnation'",
                [],
            )?;
            let incarnation: u64 = conn.query_row(
                "SELECT CAST(value AS INTEGER) FROM daemon_meta WHERE key = 'incarnation'",
                [],
                |row| row.get(0),
            )?;
            Ok(incarnation)
        })
        .await
        .map_err(flatten_err)
}
