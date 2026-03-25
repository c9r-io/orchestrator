//! TTL-based log file cleanup for terminated tasks.
//!
//! Scans the logs directory and removes stdout/stderr files belonging to
//! tasks that have been in a terminal state for longer than the retention
//! period.

use crate::async_database::AsyncDatabase;
use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

/// Result of a log cleanup sweep.
pub struct LogCleanupResult {
    /// Number of log files deleted.
    pub files_deleted: u64,
    /// Total bytes freed.
    pub bytes_freed: u64,
    /// Number of empty task directories removed.
    pub dirs_removed: u64,
}

/// Clean up log files for terminated tasks older than `retention_days`.
///
/// Returns the number of files deleted, bytes freed, and empty directories
/// removed.
pub async fn cleanup_old_logs(
    db: &AsyncDatabase,
    logs_dir: &Path,
    retention_days: u32,
) -> Result<LogCleanupResult> {
    if retention_days == 0 {
        return Ok(LogCleanupResult {
            files_deleted: 0,
            bytes_freed: 0,
            dirs_removed: 0,
        });
    }

    // Find terminal tasks older than retention period.
    let task_ids: Vec<String> = db
        .reader()
        .call(move |conn| {
            let sql = format!(
                "SELECT id FROM tasks \
                 WHERE status IN ('completed','failed','cancelled') \
                   AND updated_at < datetime('now', '-{retention_days} days')"
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

    let mut files_deleted: u64 = 0;
    let mut bytes_freed: u64 = 0;
    let mut dirs_removed: u64 = 0;

    for task_id in &task_ids {
        let task_log_dir = logs_dir.join(task_id);
        if !task_log_dir.is_dir() {
            continue;
        }

        // Delete all files in the task log directory.
        match std::fs::read_dir(&task_log_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                        if std::fs::remove_file(&path).is_ok() {
                            files_deleted += 1;
                            bytes_freed += size;
                        }
                    }
                    // Also clean subdirectories (sessions, worktrees, etc.)
                    if path.is_dir() {
                        if let Ok(meta) = dir_size(&path) {
                            bytes_freed += meta;
                        }
                        if std::fs::remove_dir_all(&path).is_ok() {
                            dirs_removed += 1;
                        }
                    }
                }
            }
            Err(e) => {
                warn!(dir = %task_log_dir.display(), error = %e, "failed to read task log directory");
                continue;
            }
        }

        // Remove the now-empty task directory.
        if std::fs::remove_dir(&task_log_dir).is_ok() {
            dirs_removed += 1;
        }
    }

    if files_deleted > 0 {
        info!(
            files = files_deleted,
            bytes = bytes_freed,
            dirs = dirs_removed,
            tasks = task_ids.len(),
            "log cleanup completed"
        );
    }

    Ok(LogCleanupResult {
        files_deleted,
        bytes_freed,
        dirs_removed,
    })
}

/// Calculate total size of a directory recursively.
fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_size(&entry.path())?;
        }
    }
    Ok(total)
}
