use anyhow::Result;
use std::sync::Arc;

use crate::async_database::{AsyncDatabase, flatten_err};

/// Increment the persistent daemon incarnation counter and return the new value.
///
/// Uses an explicit transaction to guarantee the UPDATE executes exactly once,
/// even if the underlying connection encounters transient contention.
pub async fn increment_incarnation(db: &Arc<AsyncDatabase>) -> Result<u64> {
    db.writer()
        .call(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE daemon_meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'incarnation'",
                [],
            )?;
            let incarnation: u64 = tx.query_row(
                "SELECT CAST(value AS INTEGER) FROM daemon_meta WHERE key = 'incarnation'",
                [],
                |row| row.get(0),
            )?;
            tx.commit()?;
            Ok(incarnation)
        })
        .await
        .map_err(flatten_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    #[tokio::test]
    async fn increment_incarnation_starts_from_one() {
        let mut ts = TestState::new();
        let state = ts.build();
        let result = increment_incarnation(&state.async_database).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn increment_incarnation_returns_increasing_values() {
        let mut ts = TestState::new();
        let state = ts.build();
        let first = increment_incarnation(&state.async_database).await.unwrap();
        let second = increment_incarnation(&state.async_database).await.unwrap();
        assert!(second > first, "second ({second}) should be greater than first ({first})");
    }
}
