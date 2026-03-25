use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::OptionalExtension;
use std::sync::Arc;

#[async_trait]
/// Persistence interface for claiming and counting schedulable tasks.
pub trait SchedulerRepository: Send + Sync {
    /// Returns the next pending task identifier without mutating state.
    async fn next_pending_task_id(&self) -> Result<Option<String>>;
    /// Claims the next schedulable task and transitions it to running.
    async fn claim_next_pending_task(&self) -> Result<Option<String>>;
    /// Returns the number of tasks currently waiting to run.
    async fn pending_task_count(&self) -> Result<i64>;
}

/// SQLite-backed scheduler repository.
pub struct SqliteSchedulerRepository {
    async_db: Arc<AsyncDatabase>,
}

impl SqliteSchedulerRepository {
    /// Creates a repository backed by the provided async database handle.
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }
}

#[async_trait]
impl SchedulerRepository for SqliteSchedulerRepository {
    async fn next_pending_task_id(&self) -> Result<Option<String>> {
        self.async_db
            .reader()
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM tasks WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1",
                )?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    return Ok(Some(row.get(0)?));
                }
                Ok(None)
            })
            .await
            .map_err(flatten_err)
    }

    async fn claim_next_pending_task(&self) -> Result<Option<String>> {
        self.async_db
            .writer()
            .call(|conn| {
                let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

                let task_id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM tasks WHERE status IN ('restart_pending', 'pending') ORDER BY CASE status WHEN 'restart_pending' THEN 0 ELSE 1 END, created_at ASC LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .optional()?;

                let Some(task_id) = task_id else {
                    tx.commit()?;
                    return Ok(None);
                };

                let now = now_ts();
                let updated = tx.execute(
                    "UPDATE tasks SET status = 'running', started_at = COALESCE(started_at, ?2), completed_at = NULL, updated_at = ?3 WHERE id = ?1 AND status IN ('restart_pending', 'pending')",
                    rusqlite::params![task_id, now, now_ts()],
                )?;
                tx.commit()?;
                if updated == 1 {
                    Ok(Some(task_id))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(flatten_err)
    }

    async fn pending_task_count(&self) -> Result<i64> {
        self.async_db
            .reader()
            .call(|conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE status = 'pending'",
                    [],
                    |row| row.get(0),
                )?;
                Ok(count)
            })
            .await
            .map_err(flatten_err)
    }
}
