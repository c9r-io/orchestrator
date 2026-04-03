use crate::async_database::{AsyncDatabase, flatten_err};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
/// One workflow store entry returned from persistence.
pub struct WorkflowStoreEntryRow {
    /// Entry key within the named workflow store.
    pub key: String,
    /// Serialized JSON value stored for the key.
    pub value_json: String,
    /// Timestamp when the entry was last updated.
    pub updated_at: String,
}

#[async_trait]
/// Persistence interface for workflow store key-value data.
pub trait WorkflowStoreRepository: Send + Sync {
    /// Loads one workflow store entry by key.
    async fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<Option<String>>;
    /// Inserts or updates one workflow store entry.
    async fn put(
        &self,
        store_name: &str,
        project_id: &str,
        key: &str,
        value: &str,
        task_id: &str,
    ) -> Result<()>;
    /// Deletes one workflow store entry by key.
    async fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<()>;
    /// Lists workflow store entries ordered by most recent update time.
    async fn list(
        &self,
        store_name: &str,
        project_id: &str,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<WorkflowStoreEntryRow>>;
    /// Prunes workflow store entries using TTL and/or maximum-entry retention rules.
    async fn prune(
        &self,
        store_name: &str,
        project_id: &str,
        max_entries: Option<u64>,
        ttl_days: Option<u64>,
    ) -> Result<()>;
}

/// SQLite-backed workflow store repository.
pub struct SqliteWorkflowStoreRepository {
    async_db: Arc<AsyncDatabase>,
}

impl SqliteWorkflowStoreRepository {
    /// Creates a repository backed by the provided async database handle.
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }
}

#[async_trait]
impl WorkflowStoreRepository for SqliteWorkflowStoreRepository {
    async fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<Option<String>> {
        let store_name = store_name.to_owned();
        let project_id = project_id.to_owned();
        let key = key.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT value_json FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2 AND key = ?3",
                    )
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;

                let value = stmt
                    .query_row(rusqlite::params![store_name, project_id, key], |row| {
                        row.get(0)
                    })
                    .ok();

                Ok(value)
            })
            .await
            .map_err(flatten_err)
    }

    async fn put(
        &self,
        store_name: &str,
        project_id: &str,
        key: &str,
        value: &str,
        task_id: &str,
    ) -> Result<()> {
        let store_name = store_name.to_owned();
        let project_id = project_id.to_owned();
        let key = key.to_owned();
        let value = value.to_owned();
        let task_id = task_id.to_owned();
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
                    rusqlite::params![store_name, project_id, key, value, task_id],
                )
                .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    async fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<()> {
        let store_name = store_name.to_owned();
        let project_id = project_id.to_owned();
        let key = key.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM workflow_store_entries
                     WHERE store_name = ?1 AND project_id = ?2 AND key = ?3",
                    rusqlite::params![store_name, project_id, key],
                )
                .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    async fn list(
        &self,
        store_name: &str,
        project_id: &str,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<WorkflowStoreEntryRow>> {
        let store_name = store_name.to_owned();
        let project_id = project_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT key, value_json, updated_at FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                         ORDER BY updated_at DESC
                         LIMIT ?3 OFFSET ?4",
                    )
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;

                let rows = stmt
                    .query_map(
                        rusqlite::params![store_name, project_id, limit, offset],
                        |row| {
                            Ok(WorkflowStoreEntryRow {
                                key: row.get(0)?,
                                value_json: row.get(1)?,
                                updated_at: row.get(2)?,
                            })
                        },
                    )
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;

                Ok(rows)
            })
            .await
            .map_err(flatten_err)
    }

    async fn prune(
        &self,
        store_name: &str,
        project_id: &str,
        max_entries: Option<u64>,
        ttl_days: Option<u64>,
    ) -> Result<()> {
        let store_name = store_name.to_owned();
        let project_id = project_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                if let Some(ttl_days) = ttl_days {
                    conn.execute(
                        "DELETE FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                           AND updated_at < datetime('now', ?3)",
                        rusqlite::params![store_name, project_id, format!("-{} days", ttl_days)],
                    )
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                }

                if let Some(max_entries) = max_entries {
                    conn.execute(
                        "DELETE FROM workflow_store_entries
                         WHERE store_name = ?1 AND project_id = ?2
                           AND rowid NOT IN (
                               SELECT rowid FROM workflow_store_entries
                               WHERE store_name = ?1 AND project_id = ?2
                               ORDER BY updated_at DESC
                               LIMIT ?3
                           )",
                        rusqlite::params![store_name, project_id, max_entries],
                    )
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))?;
                }

                Ok(())
            })
            .await
            .map_err(flatten_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    const STORE: &str = "test-store";
    const PROJECT: &str = "test-project";
    const TASK: &str = "task-1";

    fn make_repo(state: &Arc<crate::state::InnerState>) -> SqliteWorkflowStoreRepository {
        SqliteWorkflowStoreRepository::new(state.async_database.clone())
    }

    #[tokio::test]
    async fn get_missing_key_returns_none() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        let result = repo.get(STORE, PROJECT, "no-such-key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn put_and_get_round_trip() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", r#"{"v":1}"#, TASK)
            .await
            .unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert_eq!(val, Some(r#"{"v":1}"#.to_string()));
    }

    #[tokio::test]
    async fn put_overwrites_existing() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", "old", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "k1", "new", TASK).await.unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert_eq!(val, Some("new".to_string()));
    }

    #[tokio::test]
    async fn delete_removes_entry() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "k1", "val", TASK).await.unwrap();
        repo.delete(STORE, PROJECT, "k1").await.unwrap();
        let val = repo.get(STORE, PROJECT, "k1").await.unwrap();
        assert!(val.is_none());
    }

    /// Helper: manually set updated_at for a key to a specific offset from now.
    async fn set_updated_at(state: &Arc<crate::state::InnerState>, key: &str, offset: &str) {
        let k = key.to_string();
        let o = offset.to_string();
        state
            .async_database
            .writer()
            .call(move |conn| {
                conn.execute(
                "UPDATE workflow_store_entries SET updated_at = datetime('now', ?1) WHERE key = ?2",
                rusqlite::params![o, k],
            )?;
                Ok(())
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn list_returns_entries_ordered_by_time() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "a", "1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "b", "2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "c", "3", TASK).await.unwrap();
        // Force distinct timestamps: a=-3min, b=-2min, c=-1min
        set_updated_at(&state, "a", "-3 minutes").await;
        set_updated_at(&state, "b", "-2 minutes").await;
        set_updated_at(&state, "c", "-1 minutes").await;
        let rows = repo.list(STORE, PROJECT, 10, 0).await.unwrap();
        assert_eq!(rows.len(), 3);
        // ORDER BY updated_at DESC — most recent first
        assert_eq!(rows[0].key, "c");
        assert_eq!(rows[1].key, "b");
        assert_eq!(rows[2].key, "a");
    }

    #[tokio::test]
    async fn list_respects_limit_and_offset() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "a", "1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "b", "2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "c", "3", TASK).await.unwrap();
        // Force distinct timestamps: a=-3min, b=-2min, c=-1min
        set_updated_at(&state, "a", "-3 minutes").await;
        set_updated_at(&state, "b", "-2 minutes").await;
        set_updated_at(&state, "c", "-1 minutes").await;
        let rows = repo.list(STORE, PROJECT, 2, 1).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Offset 1 skips the newest ("c"), so we get "b" and "a"
        assert_eq!(rows[0].key, "b");
        assert_eq!(rows[1].key, "a");
    }

    #[tokio::test]
    async fn prune_by_ttl_removes_old_entries() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        repo.put(STORE, PROJECT, "old1", "v1", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "old2", "v2", TASK).await.unwrap();
        repo.put(STORE, PROJECT, "fresh", "v3", TASK).await.unwrap();

        // Manually backdate old1 and old2
        for key in &["old1", "old2"] {
            let k = key.to_string();
            state.async_database.writer().call(move |conn| {
                conn.execute(
                    "UPDATE workflow_store_entries SET updated_at = datetime('now', '-30 days') WHERE key = ?1",
                    rusqlite::params![k],
                )?;
                Ok(())
            }).await.unwrap();
        }

        repo.prune(STORE, PROJECT, None, Some(7)).await.unwrap();
        let rows = repo.list(STORE, PROJECT, 100, 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "fresh");
    }

    #[tokio::test]
    async fn prune_by_max_entries_keeps_newest() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = make_repo(&state);
        for i in 1..=5 {
            let key = format!("k{i}");
            let val = format!("v{i}");
            repo.put(STORE, PROJECT, &key, &val, TASK).await.unwrap();
        }
        // Force distinct timestamps so prune ordering is deterministic
        for i in 1..=5 {
            let offset = format!("-{} minutes", 6 - i); // k1=-5min .. k5=-1min
            set_updated_at(&state, &format!("k{i}"), &offset).await;
        }
        repo.prune(STORE, PROJECT, Some(2), None).await.unwrap();
        let rows = repo.list(STORE, PROJECT, 100, 0).await.unwrap();
        assert_eq!(rows.len(), 2);
        // The two newest should remain: k5 and k4
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"k5"));
        assert!(keys.contains(&"k4"));
    }
}
