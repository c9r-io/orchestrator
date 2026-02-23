use crate::events::insert_event;
use crate::scheduler::set_task_status;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::path::PathBuf;

pub fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    set_task_status(state, task_id, "pending", false)?;
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
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(row.get(0)?));
    }
    Ok(None)
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
    Ok(())
}

