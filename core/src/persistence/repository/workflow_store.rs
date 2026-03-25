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
