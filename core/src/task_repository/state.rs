use crate::config_load::now_ts;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::types::TaskRepositoryConn;

pub fn set_task_status(
    conn: &TaskRepositoryConn,
    task_id: &str,
    status: &str,
    set_completed: bool,
) -> Result<()> {
    let now = now_ts();
    if set_completed {
        conn.execute(
            "UPDATE tasks SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = ?4, updated_at = ?5 WHERE id = ?1",
            params![task_id, status, now.clone(), now.clone(), now],
        )?;
    } else if status == "running" {
        conn.execute(
            "UPDATE tasks SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = NULL, updated_at = ?4 WHERE id = ?1",
            params![task_id, status, now.clone(), now],
        )?;
    } else if matches!(status, "pending" | "paused" | "interrupted" | "restart_pending") {
        conn.execute(
            "UPDATE tasks SET status = ?2, completed_at = NULL, updated_at = ?3 WHERE id = ?1",
            params![task_id, status, now],
        )?;
    } else {
        conn.execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_id, status, now],
        )?;
    }
    Ok(())
}

pub fn prepare_task_for_start_batch(conn: &TaskRepositoryConn, task_id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let status: Option<String> = tx
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?;

    if status.is_none() {
        anyhow::bail!("task not found: {}", task_id);
    }

    if matches!(status.as_deref(), Some("running")) {
        anyhow::bail!(
            "task {} is already running — cannot start a second instance. \
             Use 'task pause' first, or wait for it to finish.",
            task_id
        );
    }

    if matches!(status.as_deref(), Some("restart_pending")) {
        // Resume without resetting items — preserve exact pre-restart state
        tx.execute(
            "UPDATE tasks SET status = 'running', completed_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![task_id, now_ts()],
        )?;
        tx.commit()?;
        return Ok(());
    }

    if matches!(status.as_deref(), Some("failed")) {
        tx.execute(
            "UPDATE task_items SET status='pending', ticket_files_json='[]', ticket_content_json='[]', fix_required=0, fixed=0, last_error='', completed_at=NULL, updated_at=?2 WHERE task_id=?1 AND status='unresolved'",
            params![task_id, now_ts()],
        )?;
    }

    tx.execute(
        "UPDATE tasks SET status = 'running', started_at = COALESCE(started_at, ?2), completed_at = NULL, updated_at = ?3 WHERE id = ?1",
        params![task_id, now_ts(), now_ts()],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn update_task_cycle_state(
    conn: &TaskRepositoryConn,
    task_id: &str,
    current_cycle: u32,
    init_done: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET current_cycle = ?2, init_done = ?3, updated_at = ?4 WHERE id = ?1",
        params![
            task_id,
            current_cycle as i64,
            if init_done { 1 } else { 0 },
            now_ts()
        ],
    )?;
    Ok(())
}
