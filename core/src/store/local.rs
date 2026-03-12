//! Local store backend — SQLite-based persistent store via repository boundary.

use crate::persistence::repository::{SqliteWorkflowStoreRepository, WorkflowStoreRepository};
use crate::store::{StoreEntry, StoreOp, StoreOpResult};
use anyhow::{Context, Result};
use std::sync::Arc;

/// SQLite-backed workflow store implementation.
pub struct LocalStoreBackend {
    repository: Arc<dyn WorkflowStoreRepository>,
}

impl LocalStoreBackend {
    /// Creates a backend backed by the default SQLite repository.
    pub fn new(async_db: Arc<crate::async_database::AsyncDatabase>) -> Self {
        Self::with_repository(Arc::new(SqliteWorkflowStoreRepository::new(async_db)))
    }

    /// Creates a backend from an injected repository implementation.
    pub fn with_repository(repository: Arc<dyn WorkflowStoreRepository>) -> Self {
        Self { repository }
    }

    /// Executes a store operation using the repository boundary.
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
        let result = self.repository.get(store_name, project_id, key).await?;

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
        self.repository
            .put(store_name, project_id, key, value, task_id)
            .await?;

        Ok(StoreOpResult::Ok)
    }

    async fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<StoreOpResult> {
        self.repository.delete(store_name, project_id, key).await?;

        Ok(StoreOpResult::Ok)
    }

    async fn list(
        &self,
        store_name: &str,
        project_id: &str,
        limit: u64,
        offset: u64,
    ) -> Result<StoreOpResult> {
        let entries = self
            .repository
            .list(store_name, project_id, limit, offset)
            .await?;

        let entries: Vec<StoreEntry> = entries
            .into_iter()
            .filter_map(|row| {
                let value = serde_json::from_str(&row.value_json).ok()?;
                Some(StoreEntry {
                    key: row.key,
                    value,
                    updated_at: row.updated_at,
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
        self.repository
            .prune(store_name, project_id, max_entries, ttl_days)
            .await?;

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
