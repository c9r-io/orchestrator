use crate::config_load::now_ts;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use rusqlite::Connection;

pub fn set_task_status(
    conn: &Connection,
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
    } else if matches!(
        status,
        "pending" | "paused" | "interrupted" | "restart_pending"
    ) {
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

pub fn prepare_task_for_start_batch(conn: &Connection, task_id: &str) -> Result<()> {
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
    conn: &Connection,
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

/// Recover all orphaned running items across all tasks.
/// Resets running items to `pending` and their parent tasks to `restart_pending`.
/// Returns `Vec<(task_id, Vec<item_id>)>` for audit.
pub fn recover_orphaned_running_items(conn: &Connection) -> Result<Vec<(String, Vec<String>)>> {
    let tx = conn.unchecked_transaction()?;
    let now = now_ts();

    // Find all running items grouped by task
    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, task_id FROM task_items WHERE status = 'running' ORDER BY task_id",
        )?;
        let mapped = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mapped
    };

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Group by task_id
    let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (item_id, task_id) in &rows {
        grouped
            .entry(task_id.clone())
            .or_default()
            .push(item_id.clone());
    }

    // Reset items to pending
    tx.execute(
        "UPDATE task_items SET status = 'pending', started_at = NULL, completed_at = NULL, updated_at = ?1 WHERE status = 'running'",
        params![now],
    )?;

    // Set parent tasks from running to restart_pending
    for task_id in grouped.keys() {
        tx.execute(
            "UPDATE tasks SET status = 'restart_pending', completed_at = NULL, updated_at = ?2 WHERE id = ?1 AND status = 'running'",
            params![task_id, now],
        )?;
    }

    tx.commit()?;
    Ok(grouped.into_iter().collect())
}

/// Recover orphaned running items for a single task.
/// Returns the list of recovered item IDs.
pub fn recover_orphaned_running_items_for_task(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<String>> {
    let tx = conn.unchecked_transaction()?;
    let now = now_ts();

    let item_ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM task_items WHERE task_id = ?1 AND status = 'running'",
        )?;
        let mapped = stmt
            .query_map(params![task_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mapped
    };

    if item_ids.is_empty() {
        return Ok(Vec::new());
    }

    tx.execute(
        "UPDATE task_items SET status = 'pending', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE task_id = ?1 AND status = 'running'",
        params![task_id, now],
    )?;

    tx.execute(
        "UPDATE tasks SET status = 'restart_pending', completed_at = NULL, updated_at = ?2 WHERE id = ?1 AND status = 'running'",
        params![task_id, now],
    )?;

    tx.commit()?;
    Ok(item_ids)
}

/// Recover stalled running items older than the given threshold.
/// Returns `Vec<(task_id, Vec<item_id>)>` for audit.
pub fn recover_stalled_running_items(
    conn: &Connection,
    stall_threshold_secs: u64,
) -> Result<Vec<(String, Vec<String>)>> {
    let tx = conn.unchecked_transaction()?;
    let now = now_ts();

    // Compute cutoff timestamp
    let cutoff = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::seconds(stall_threshold_secs as i64))
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();

    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, task_id FROM task_items WHERE status = 'running' AND started_at < ?1 ORDER BY task_id",
        )?;
        let mapped = stmt
            .query_map(params![cutoff], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mapped
    };

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (item_id, task_id) in &rows {
        grouped
            .entry(task_id.clone())
            .or_default()
            .push(item_id.clone());
    }

    // Reset stalled items
    for item_id in grouped.values().flatten() {
        tx.execute(
            "UPDATE task_items SET status = 'pending', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![item_id, now],
        )?;
    }

    // Set parent tasks to restart_pending
    for task_id in grouped.keys() {
        tx.execute(
            "UPDATE tasks SET status = 'restart_pending', completed_at = NULL, updated_at = ?2 WHERE id = ?1 AND status = 'running'",
            params![task_id, now],
        )?;
    }

    tx.commit()?;
    Ok(grouped.into_iter().collect())
}
