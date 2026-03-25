//! Auto-cleanup of terminated tasks and their associated data.
//!
//! Batch-deletes tasks in terminal state (completed/failed/cancelled) that
//! are older than a configurable retention period. Cascade-deletes all
//! related items, runs, events, and log files.

use crate::async_database::AsyncDatabase;
use crate::task_repository::items::delete_task_and_collect_log_paths;
use anyhow::Result;
use std::path::Path;
use tracing::info;

/// Clean up terminated tasks older than `retention_days`.
///
/// Cascade-deletes task_items, command_runs, events, and physically removes
/// log files. Returns the number of tasks deleted.
pub async fn cleanup_old_tasks(
    db: &AsyncDatabase,
    logs_dir: &Path,
    retention_days: u32,
    batch_limit: u32,
) -> Result<u64> {
    if retention_days == 0 {
        return Ok(0);
    }

    let limit = if batch_limit == 0 { 50 } else { batch_limit };

    // Find candidate task IDs.
    let task_ids: Vec<String> = db
        .reader()
        .call(move |conn| {
            let sql = format!(
                "SELECT id FROM tasks \
                 WHERE status IN ('completed','failed','cancelled') \
                   AND updated_at < datetime('now', '-{retention_days} days') \
                 LIMIT {limit}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let ids: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(ids)
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if task_ids.is_empty() {
        return Ok(0);
    }

    let mut deleted = 0u64;
    let logs_dir = logs_dir.to_path_buf();

    for task_id in &task_ids {
        let tid = task_id.clone();
        let log_paths: Vec<String> = db
            .writer()
            .call(move |conn| {
                delete_task_and_collect_log_paths(conn, &tid)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Physically remove log files.
        for path_str in &log_paths {
            let path = Path::new(path_str);
            if path.is_file() {
                let _ = std::fs::remove_file(path);
            }
        }

        // Remove the task log directory if it exists.
        let task_log_dir = logs_dir.join(task_id);
        if task_log_dir.is_dir() {
            let _ = std::fs::remove_dir_all(&task_log_dir);
        }

        deleted += 1;
    }

    if deleted > 0 {
        info!(
            tasks = deleted,
            retention_days, "task auto-cleanup completed"
        );
    }

    Ok(deleted)
}
