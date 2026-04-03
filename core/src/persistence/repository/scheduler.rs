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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use crate::test_utils::TestState;

    /// Helper: insert a task row with a given status and created_at.
    async fn insert_task_with_status(
        db: &AsyncDatabase,
        task_id: &str,
        status: &str,
        created_at: &str,
    ) {
        let id = task_id.to_owned();
        let st = status.to_owned();
        let ca = created_at.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, \
                     project_id, workspace_id, workflow_id, workspace_root, \
                     qa_targets_json, ticket_dir, created_at, updated_at) \
                     VALUES (?1, ?1, ?2, '', '[]', 'auto', 'default', 'default', 'basic', \
                     '/tmp', '[]', '/tmp/tickets', ?3, ?3)",
                    rusqlite::params![id, st, ca],
                )?;
                Ok(())
            })
            .await
            .expect("insert_task_with_status");
    }

    #[tokio::test]
    async fn no_pending_tasks() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = SqliteSchedulerRepository::new(state.async_database.clone());

        assert!(repo.next_pending_task_id().await.unwrap().is_none());
        assert_eq!(repo.pending_task_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn pending_task_found() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = SqliteSchedulerRepository::new(state.async_database.clone());

        insert_task_with_status(
            &state.async_database,
            "task-1",
            "pending",
            "2024-06-01T00:00:00",
        )
        .await;

        let next = repo.next_pending_task_id().await.unwrap();
        assert_eq!(next.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn claim_transitions_to_running() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = SqliteSchedulerRepository::new(state.async_database.clone());

        insert_task_with_status(
            &state.async_database,
            "task-c",
            "pending",
            "2024-06-01T00:00:00",
        )
        .await;

        let claimed = repo.claim_next_pending_task().await.unwrap();
        assert_eq!(claimed.as_deref(), Some("task-c"));

        // After claiming, no pending tasks remain.
        assert!(repo.next_pending_task_id().await.unwrap().is_none());
        assert_eq!(repo.pending_task_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn claim_prefers_restart_pending_over_pending() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = SqliteSchedulerRepository::new(state.async_database.clone());

        // Insert pending first (older), then restart_pending (newer).
        insert_task_with_status(
            &state.async_database,
            "p-task",
            "pending",
            "2024-01-01T00:00:00",
        )
        .await;
        insert_task_with_status(
            &state.async_database,
            "rp-task",
            "restart_pending",
            "2024-06-01T00:00:00",
        )
        .await;

        let claimed = repo.claim_next_pending_task().await.unwrap();
        assert_eq!(claimed.as_deref(), Some("rp-task"));
    }

    #[tokio::test]
    async fn pending_count_accurate() {
        let mut ts = TestState::new();
        let state = ts.build();
        let repo = SqliteSchedulerRepository::new(state.async_database.clone());

        insert_task_with_status(&state.async_database, "a", "pending", "2024-01-01T00:00:00").await;
        insert_task_with_status(&state.async_database, "b", "pending", "2024-01-02T00:00:00").await;
        insert_task_with_status(&state.async_database, "c", "running", "2024-01-03T00:00:00").await;

        assert_eq!(repo.pending_task_count().await.unwrap(), 2);
    }
}
