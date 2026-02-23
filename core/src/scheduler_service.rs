use crate::config_load::now_ts;
use crate::events::insert_event;
use crate::scheduler::set_task_status;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde_json::json;
use std::path::PathBuf;

pub fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    set_task_status(state, task_id, "pending", false)?;
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
}
