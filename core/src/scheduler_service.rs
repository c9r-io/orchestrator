use crate::config_load::now_ts;
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde_json::json;
use std::path::PathBuf;

pub fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    state.db_writer.set_task_status(task_id, "pending", false)?;
    touch_worker_wake_signal(state)?;
    insert_event(
        state,
        task_id,
        None,
        "scheduler_enqueued",
        json!({"task_id":task_id}),
    )?;
    Ok(())
}

pub fn next_pending_task_id(state: &InnerState) -> Result<Option<String>> {
    let conn = crate::db::open_conn(&state.db_path)?;
    let mut stmt = conn
        .prepare("SELECT id FROM tasks WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(row.get(0)?));
    }
    Ok(None)
}

pub fn claim_next_pending_task(state: &InnerState) -> Result<Option<String>> {
    let mut conn = crate::db::open_conn(&state.db_path)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    let task_id: Option<String> = tx
        .query_row(
            "SELECT id FROM tasks WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let Some(task_id) = task_id else {
        tx.commit()?;
        return Ok(None);
    };

    let updated = tx.execute(
        "UPDATE tasks SET status = 'running', completed_at = NULL, updated_at = ?2 WHERE id = ?1 AND status = 'pending'",
        rusqlite::params![task_id, now_ts()],
    )?;
    tx.commit()?;
    if updated == 1 {
        Ok(Some(task_id))
    } else {
        Ok(None)
    }
}

pub fn pending_task_count(state: &InnerState) -> Result<i64> {
    let conn = crate::db::open_conn(&state.db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn worker_stop_signal_path(state: &InnerState) -> PathBuf {
    state.app_root.join("data").join("worker.stop")
}

pub fn worker_wake_signal_path(state: &InnerState) -> PathBuf {
    state.app_root.join("data").join("worker.wakeup")
}

pub fn touch_worker_wake_signal(state: &InnerState) -> Result<()> {
    let path = worker_wake_signal_path(state);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, now_ts())?;
    Ok(())
}

pub fn clear_worker_stop_signal(state: &InnerState) -> Result<()> {
    let path = worker_stop_signal_path(state);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn signal_worker_stop(state: &InnerState) -> Result<()> {
    let path = worker_stop_signal_path(state);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "stop")?;
    let _ = touch_worker_wake_signal(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use rusqlite::params;

    #[test]
    fn claim_next_pending_task_sets_running() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/scheduler_service_test.md");
        std::fs::write(&qa_file, "# scheduler service test\n").expect("seed qa file");
        let created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");

        let claimed = claim_next_pending_task(&state).expect("claim pending task");
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

    #[test]
    fn claim_next_pending_task_is_single_winner() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/scheduler_service_test.md");
        std::fs::write(&qa_file, "# scheduler service test\n").expect("seed qa file");
        let _created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");
        let state_a = state.clone();
        let state_b = state.clone();

        let t1 = std::thread::spawn(move || claim_next_pending_task(&state_a).expect("claim a"));
        let t2 = std::thread::spawn(move || claim_next_pending_task(&state_b).expect("claim b"));
        let r1 = t1.join().expect("thread a");
        let r2 = t2.join().expect("thread b");

        let winners = [r1, r2].into_iter().filter(|v| v.is_some()).count();
        assert_eq!(winners, 1);
    }

    /// Helper to seed a qa file and create a task, returning the state and task id.
    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/svc_test.md");
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

    #[test]
    fn enqueue_task_sets_pending_and_creates_wake_signal() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        // First claim the task so it becomes "running"
        let claimed = claim_next_pending_task(&state).expect("claim");
        assert_eq!(claimed.as_deref(), Some(task_id.as_str()));

        // Now enqueue it again
        enqueue_task(&state, &task_id).expect("enqueue task");

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

        // Wake signal file should exist
        let wake_path = worker_wake_signal_path(&state);
        assert!(wake_path.exists(), "wake signal file should exist");
    }

    #[test]
    fn next_pending_task_id_returns_pending() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        let next = next_pending_task_id(&state).expect("next pending");
        assert_eq!(next.as_deref(), Some(task_id.as_str()));
    }

    #[test]
    fn next_pending_task_id_returns_none_when_no_pending() {
        let mut fixture = TestState::new();
        let (state, _task_id) = seed_task(&mut fixture);

        // Claim the only task so none are pending
        let _ = claim_next_pending_task(&state).expect("claim");

        let next = next_pending_task_id(&state).expect("next pending after claim");
        assert!(next.is_none(), "should be no pending tasks");
    }

    #[test]
    fn pending_task_count_returns_correct_count() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        // No tasks yet
        let count = pending_task_count(&state).expect("count 0");
        assert_eq!(count, 0);

        // Seed a qa file and create 2 tasks
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/count_test.md");
        std::fs::write(&qa_file, "# count test\n").expect("seed qa file");

        create_task_impl(&state, CreateTaskPayload::default()).expect("create task 1");
        create_task_impl(&state, CreateTaskPayload::default()).expect("create task 2");

        let count = pending_task_count(&state).expect("count 2");
        assert_eq!(count, 2);

        // Claim one
        let _ = claim_next_pending_task(&state).expect("claim one");
        let count = pending_task_count(&state).expect("count after claim");
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

        // Wake signal should also be touched
        let wake_path = worker_wake_signal_path(&state);
        assert!(wake_path.exists(), "wake signal should be touched by stop");
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

    #[test]
    fn worker_signal_paths_are_under_data_dir() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let stop_path = worker_stop_signal_path(&state);
        let wake_path = worker_wake_signal_path(&state);

        assert!(stop_path.starts_with(state.app_root.join("data")));
        assert!(wake_path.starts_with(state.app_root.join("data")));
        assert!(stop_path.ends_with("worker.stop"));
        assert!(wake_path.ends_with("worker.wakeup"));
    }
}
