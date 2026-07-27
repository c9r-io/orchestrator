//! Scheduler-owned persistence rows.
//!
//! Moved to `orchestrator-persistence` (FR-130 Phase A) and re-exported here.
//! The tests stay in core: they drive the repository from a
//! `crate::test_utils::TestState` fixture, which sits above this layer.

pub use orchestrator_persistence::repository::scheduler::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use crate::test_utils::TestState;
    use orchestrator_persistence::test_support;

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
        test_support::writer(db)
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
