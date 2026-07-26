//! Writer/reader connection pair for async database I/O.
//!
//! The implementation moved to `orchestrator-persistence` (FR-130 Phase A) and
//! is re-exported here so every existing `crate::async_database::*` and
//! `agent_orchestrator::async_database::*` path keeps resolving.
//!
//! The tests stay in core because they drive the pair through `crate::db`'s
//! schema bootstrap, which is core-side until Phase A's last commit.

pub use orchestrator_persistence::async_database::*;

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
            .call(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?))
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
            .call(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?))
            .await
            .expect("read via original");
        assert_eq!(count, 1);
    }
}
