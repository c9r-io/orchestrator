use crate::events::insert_event;
use crate::persistence::repository::{SchedulerRepository, SqliteSchedulerRepository};
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;

/// Marks a task as pending and wakes the background worker.
pub async fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    state
        .db_writer
        .set_task_status(task_id, "pending", false)
        .await?;
    state.worker_notify.notify_waiters();
    insert_event(
        state,
        task_id,
        None,
        "scheduler_enqueued",
        json!({"task_id":task_id}),
    )
    .await?;
    Ok(())
}

/// Returns the next pending task identifier without claiming it.
pub async fn next_pending_task_id(state: &InnerState) -> Result<Option<String>> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .next_pending_task_id()
        .await
}

/// Claims the next pending task and transitions it to running.
pub async fn claim_next_pending_task(state: &InnerState) -> Result<Option<String>> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .claim_next_pending_task()
        .await
}

/// Returns the number of tasks currently in the pending state.
pub async fn pending_task_count(state: &InnerState) -> Result<i64> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .pending_task_count()
        .await
}

/// Returns the marker-file path used to request worker shutdown.
pub fn worker_stop_signal_path(state: &InnerState) -> PathBuf {
    state.app_root.join("data").join("worker.stop")
}

/// Service-layer wrapper around [`enqueue_task`] with error classification.
///
/// This exists so that core modules (trigger_engine) can enqueue tasks
/// without depending on the `orchestrator-scheduler` service layer.
pub async fn enqueue_task_as_service(
    state: &InnerState,
    task_id: &str,
) -> crate::error::Result<()> {
    enqueue_task(state, task_id)
        .await
        .map_err(|err| crate::error::classify_task_error("task.enqueue", err))
}

/// Removes the worker stop marker if it exists.
pub fn clear_worker_stop_signal(state: &InnerState) -> Result<()> {
    let path = worker_stop_signal_path(state);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Writes the worker stop marker and wakes the worker loop.
pub fn signal_worker_stop(state: &InnerState) -> Result<()> {
    let path = worker_stop_signal_path(state);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "stop")?;
    state.worker_notify.notify_waiters();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use rusqlite::params;

    #[tokio::test]
    async fn claim_next_pending_task_sets_running() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/scheduler_service_test.md");
        std::fs::write(&qa_file, "# scheduler service test\n").expect("seed qa file");
        let created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        // Task starts in 'created' status; enqueue to make it 'pending' and claimable
        enqueue_task(&state, &created.id)
            .await
            .expect("enqueue task");

        let claimed = claim_next_pending_task(&state)
            .await
            .expect("claim pending task");
        assert_eq!(claimed.as_deref(), Some(created.id.as_str()));

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        let status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![created.id],
                |row| row.get(0),
            )
            .expect("query status");
        assert_eq!(status, "running");
    }

    #[tokio::test]
    async fn claim_next_pending_task_is_single_winner() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/scheduler_service_test.md");
        std::fs::write(&qa_file, "# scheduler service test\n").expect("seed qa file");
        let _created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");
        enqueue_task(&state, &_created.id)
            .await
            .expect("enqueue task");
        let state_a = state.clone();
        let state_b = state.clone();

        let t1 =
            tokio::spawn(async move { claim_next_pending_task(&state_a).await.expect("claim a") });
        let t2 =
            tokio::spawn(async move { claim_next_pending_task(&state_b).await.expect("claim b") });
        let r1 = t1.await.expect("thread a");
        let r2 = t2.await.expect("thread b");

        let winners = [r1, r2].into_iter().filter(|v| v.is_some()).count();
        assert_eq!(winners, 1);
    }

    /// Helper to seed a qa file and create a task, returning the state and task id.
    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state.app_root.join("workspace/default/docs/qa/svc_test.md");
        std::fs::write(&qa_file, "# svc test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("svc-test".to_string()),
                ..Default::default()
            },
        )
        .expect("create task");
        (state, created.id)
    }

    /// Helper to seed a qa file, create a task, and enqueue it (making it claimable).
    async fn seed_and_enqueue_task(
        fixture: &mut TestState,
    ) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let (state, task_id) = seed_task(fixture);
        enqueue_task(&state, &task_id).await.expect("enqueue task");
        (state, task_id)
    }

    #[tokio::test]
    async fn enqueue_task_sets_pending() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_and_enqueue_task(&mut fixture).await;

        // First claim the task so it becomes "running"
        let claimed = claim_next_pending_task(&state).await.expect("claim");
        assert_eq!(claimed.as_deref(), Some(task_id.as_str()));

        // Now enqueue it again
        enqueue_task(&state, &task_id).await.expect("enqueue task");

        // Verify it is pending again
        let conn = crate::db::open_conn(&state.db_path).expect("open db");
        let status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query status");
        assert_eq!(status, "pending");
    }

    #[tokio::test]
    async fn next_pending_task_id_returns_pending() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_and_enqueue_task(&mut fixture).await;

        let next = next_pending_task_id(&state).await.expect("next pending");
        assert_eq!(next.as_deref(), Some(task_id.as_str()));
    }

    #[tokio::test]
    async fn next_pending_task_id_returns_none_when_no_pending() {
        let mut fixture = TestState::new();
        let (state, _task_id) = seed_and_enqueue_task(&mut fixture).await;

        // Claim the only task so none are pending
        let _ = claim_next_pending_task(&state).await.expect("claim");

        let next = next_pending_task_id(&state)
            .await
            .expect("next pending after claim");
        assert!(next.is_none(), "should be no pending tasks");
    }

    #[tokio::test]
    async fn pending_task_count_returns_correct_count() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // No tasks yet
        let count = pending_task_count(&state).await.expect("count 0");
        assert_eq!(count, 0);

        // Seed a qa file and create 2 tasks, then enqueue them
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/count_test.md");
        std::fs::write(&qa_file, "# count test\n").expect("seed qa file");

        let t1 = create_task_impl(&state, CreateTaskPayload::default()).expect("create task 1");
        let t2 = create_task_impl(&state, CreateTaskPayload::default()).expect("create task 2");
        enqueue_task(&state, &t1.id).await.expect("enqueue task 1");
        enqueue_task(&state, &t2.id).await.expect("enqueue task 2");

        let count = pending_task_count(&state).await.expect("count 2");
        assert_eq!(count, 2);

        // Claim one
        let _ = claim_next_pending_task(&state).await.expect("claim one");
        let count = pending_task_count(&state).await.expect("count after claim");
        assert_eq!(count, 1);
    }

    #[test]
    fn signal_worker_stop_creates_stop_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let stop_path = worker_stop_signal_path(&state);
        assert!(!stop_path.exists(), "stop file should not exist yet");

        signal_worker_stop(&state).expect("signal stop");
        assert!(stop_path.exists(), "stop file should be created");

        let contents = std::fs::read_to_string(&stop_path).expect("read stop file");
        assert_eq!(contents, "stop");
    }

    #[tokio::test]
    async fn signal_worker_stop_notifies_waiters() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let notify = state.worker_notify.clone();

        let waiter = tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_millis(250), notify.notified())
                .await
                .is_ok()
        });

        tokio::task::yield_now().await;
        signal_worker_stop(&state).expect("signal stop");

        assert!(
            waiter.await.expect("waiter join"),
            "stop signal should wake waiting workers"
        );
    }

    #[test]
    fn clear_worker_stop_signal_removes_stop_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // Create the stop signal first
        signal_worker_stop(&state).expect("signal stop");
        let stop_path = worker_stop_signal_path(&state);
        assert!(stop_path.exists());

        // Clear it
        clear_worker_stop_signal(&state).expect("clear stop signal");
        assert!(!stop_path.exists(), "stop file should be removed");
    }

    #[test]
    fn clear_worker_stop_signal_noop_when_no_file() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // Should not error even if file doesn't exist
        clear_worker_stop_signal(&state).expect("clear nonexistent stop signal");
    }

    #[tokio::test]
    async fn claim_next_prioritizes_restart_pending() {
        let mut fixture = TestState::new();
        let (state, pending_task_id) = seed_and_enqueue_task(&mut fixture).await;

        // Create a second task and manually set it to restart_pending
        let qa_file2 = state
            .app_root
            .join("workspace/default/docs/qa/svc_test2.md");
        std::fs::write(&qa_file2, "# svc test 2\n").expect("seed qa file 2");
        let created2 = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("restart-test".to_string()),
                ..Default::default()
            },
        )
        .expect("create second task");

        // Set second task to restart_pending
        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE tasks SET status='restart_pending' WHERE id = ?1",
            params![created2.id],
        )
        .expect("set restart_pending");

        // Claim should pick up restart_pending before pending
        let claimed = claim_next_pending_task(&state)
            .await
            .expect("claim restart_pending task");
        assert_eq!(
            claimed.as_deref(),
            Some(created2.id.as_str()),
            "restart_pending should be claimed before pending"
        );

        // Now claiming again should pick up the remaining pending task
        let claimed2 = claim_next_pending_task(&state)
            .await
            .expect("claim pending task");
        assert_eq!(claimed2.as_deref(), Some(pending_task_id.as_str()));
    }

    #[test]
    fn worker_signal_paths_are_under_data_dir() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let stop_path = worker_stop_signal_path(&state);

        assert!(stop_path.starts_with(state.app_root.join("data")));
        assert!(stop_path.ends_with("worker.stop"));
    }
}
