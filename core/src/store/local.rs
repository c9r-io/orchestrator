//! Local store backend — SQLite-based persistent store.

use crate::async_database::AsyncDatabase;
use crate::store::{StoreEntry, StoreOp, StoreOpResult};
use anyhow::{Context, Result};
use std::sync::Arc;

pub struct LocalStoreBackend {
    async_db: Arc<AsyncDatabase>,
}

impl LocalStoreBackend {
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }

    pub async fn execute(&self, op: StoreOp) -> Result<StoreOpResult> {
        match op {
            StoreOp::Get {
                store_name,
                project_id,
                key,
            } => self.get(&store_name, &project_id, &key).await,
            StoreOp::Put {
                store_name,
                project_id,
                key,
                value,
                task_id,
            } => {
                self.put(&store_name, &project_id, &key, &value, &task_id)
                    .await
            }
            StoreOp::Delete {
                store_name,
                project_id,
                key,
            } => self.delete(&store_name, &project_id, &key).await,
            StoreOp::List {
                store_name,
                project_id,
                limit,
                offset,
            } => self.list(&store_name, &project_id, limit, offset).await,
            StoreOp::Prune {
                store_name,
                project_id,
                max_entries,
                ttl_days,
            } => {
                self.prune(&store_name, &project_id, max_entries, ttl_days)
                    .await
            }
        }
    }

    async fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<StoreOpResult> {
        let sn = store_name.to_string();
        let pid = project_id.to_string();
        let k = key.to_string();

        let result = self
            .async_db
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT value_json FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2 AND key = ?3",
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;

                let value: Option<String> = stmt
                    .query_row(rusqlite::params![sn, pid, k], |row| row.get(0))
                    .ok();

                Ok(value)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let parsed = result
            .map(|v| serde_json::from_str::<serde_json::Value>(&v))
            .transpose()
            .context("failed to parse stored JSON value")?;

        Ok(StoreOpResult::Value(parsed))
    }

    async fn put(
        &self,
        store_name: &str,
        project_id: &str,
        key: &str,
        value: &str,
        task_id: &str,
    ) -> Result<StoreOpResult> {
        let sn = store_name.to_string();
        let pid = project_id.to_string();
        let k = key.to_string();
        let v = value.to_string();
        let tid = task_id.to_string();

        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO workflow_store_entries (store_name, project_id, key, value_json, task_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
                     ON CONFLICT(store_name, project_id, key) DO UPDATE SET
                         value_json = excluded.value_json,
                         task_id = excluded.task_id,
                         updated_at = datetime('now')",
                    rusqlite::params![sn, pid, k, v, tid],
                )
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(StoreOpResult::Ok)
    }

    async fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<StoreOpResult> {
        let sn = store_name.to_string();
        let pid = project_id.to_string();
        let k = key.to_string();

        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM workflow_store_entries
                     WHERE store_name = ?1 AND project_id = ?2 AND key = ?3",
                    rusqlite::params![sn, pid, k],
                )
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(StoreOpResult::Ok)
    }

    async fn list(
        &self,
        store_name: &str,
        project_id: &str,
        limit: u64,
        offset: u64,
    ) -> Result<StoreOpResult> {
        let sn = store_name.to_string();
        let pid = project_id.to_string();

        let entries = self
            .async_db
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT key, value_json, updated_at FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                         ORDER BY updated_at DESC
                         LIMIT ?3 OFFSET ?4",
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;

                let rows = stmt
                    .query_map(rusqlite::params![sn, pid, limit, offset], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;

                Ok(rows)
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let entries: Vec<StoreEntry> = entries
            .into_iter()
            .filter_map(|(key, value_json, updated_at)| {
                let value = serde_json::from_str(&value_json).ok()?;
                Some(StoreEntry {
                    key,
                    value,
                    updated_at,
                })
            })
            .collect();

        Ok(StoreOpResult::Entries(entries))
    }

    async fn prune(
        &self,
        store_name: &str,
        project_id: &str,
        max_entries: Option<u64>,
        ttl_days: Option<u64>,
    ) -> Result<StoreOpResult> {
        let sn = store_name.to_string();
        let pid = project_id.to_string();

        self.async_db
            .writer()
            .call(move |conn| {
                // TTL-based pruning
                if let Some(ttl) = ttl_days {
                    conn.execute(
                        "DELETE FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                           AND updated_at < datetime('now', ?3)",
                        rusqlite::params![sn, pid, format!("-{} days", ttl)],
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                }

                // Max-entries pruning (keep newest N)
                if let Some(max) = max_entries {
                    conn.execute(
                        "DELETE FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                           AND rowid NOT IN (
                               SELECT rowid FROM workflow_store_entries
                               WHERE store_name = ?1 AND project_id = ?2
                               ORDER BY updated_at DESC
                               LIMIT ?3
                           )",
                        rusqlite::params![sn, pid, max],
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                }

                Ok(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(StoreOpResult::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    #[tokio::test]
    async fn put_get_delete_round_trip() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let backend = LocalStoreBackend::new(state.async_database.clone());

        // Put
        let result = backend
            .put(
                "metrics",
                "",
                "bench_001",
                r#"{"test_count": 42}"#,
                "task-1",
            )
            .await
            .expect("put should succeed");
        assert!(matches!(result, StoreOpResult::Ok));

        // Get
        let result = backend
            .get("metrics", "", "bench_001")
            .await
            .expect("get should succeed");
        match result {
            StoreOpResult::Value(Some(v)) => {
                assert_eq!(v["test_count"], 42);
            }
            other => panic!("expected Value(Some(...)), got {:?}", other),
        }

        // Delete
        let result = backend
            .delete("metrics", "", "bench_001")
            .await
            .expect("delete should succeed");
        assert!(matches!(result, StoreOpResult::Ok));

        // Get after delete
        let result = backend
            .get("metrics", "", "bench_001")
            .await
            .expect("get should succeed");
        assert!(matches!(result, StoreOpResult::Value(None)));
    }

    #[tokio::test]
    async fn list_returns_entries() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let backend = LocalStoreBackend::new(state.async_database.clone());

        backend
            .put("metrics", "p1", "k1", r#"{"v": 1}"#, "t1")
            .await
            .expect("put k1");
        backend
            .put("metrics", "p1", "k2", r#"{"v": 2}"#, "t1")
            .await
            .expect("put k2");

        let result = backend
            .list("metrics", "p1", 100, 0)
            .await
            .expect("list should succeed");
        match result {
            StoreOpResult::Entries(entries) => {
                assert_eq!(entries.len(), 2);
            }
            other => panic!("expected Entries, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn put_upserts_existing_key() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let backend = LocalStoreBackend::new(state.async_database.clone());

        backend
            .put("s", "", "k", r#"{"v": 1}"#, "t1")
            .await
            .expect("first put");
        backend
            .put("s", "", "k", r#"{"v": 2}"#, "t2")
            .await
            .expect("second put (upsert)");

        let result = backend.get("s", "", "k").await.expect("get");
        match result {
            StoreOpResult::Value(Some(v)) => assert_eq!(v["v"], 2),
            other => panic!("expected Value(Some(...)), got {:?}", other),
        }
    }
}
