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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    async fn insert_task(db: &AsyncDatabase, task_id: &str, status: &str) {
        let id = task_id.to_owned();
        let st = status.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, \
                     project_id, workspace_id, workflow_id, workspace_root, \
                     qa_targets_json, ticket_dir, created_at, updated_at) \
                     VALUES (?1, ?1, ?2, '', '[]', 'auto', 'default', 'default', 'basic', \
                     '/tmp', '[]', '/tmp/tickets', datetime('now'), datetime('now'))",
                    rusqlite::params![id, st],
                )?;
                Ok(())
            })
            .await
            .expect("insert_task");
    }

    async fn age_task(db: &AsyncDatabase, task_id: &str) {
        let id = task_id.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    "UPDATE tasks SET updated_at = datetime('now', '-30 days') WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
            .expect("age_task");
    }

    #[tokio::test]
    async fn retention_zero_returns_immediately() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");

        let result = cleanup_old_logs(&state.async_database, tmp.path(), 0)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 0);
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.dirs_removed, 0);
    }

    #[tokio::test]
    async fn no_terminal_tasks_no_cleanup() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        insert_task(db, "running-1", "running").await;
        insert_task(db, "pending-1", "pending").await;
        age_task(db, "running-1").await;
        age_task(db, "pending-1").await;

        // Create log dirs that should NOT be cleaned.
        let log_dir = tmp.path().join("running-1");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(log_dir.join("stdout.log"), "should remain").unwrap();

        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 0);
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.dirs_removed, 0);
        // Log file must still exist.
        assert!(log_dir.join("stdout.log").exists());
    }

    #[tokio::test]
    async fn old_completed_task_logs_cleaned() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        insert_task(db, "done-1", "completed").await;
        age_task(db, "done-1").await;

        // Create log files.
        let task_dir = tmp.path().join("done-1");
        std::fs::create_dir_all(&task_dir).unwrap();
        let content_a = b"log-content-aaaa";
        let content_b = b"log-content-bb";
        std::fs::write(task_dir.join("stdout.log"), content_a).unwrap();
        std::fs::write(task_dir.join("stderr.log"), content_b).unwrap();

        // Also add a subdirectory with a file inside.
        let sub = task_dir.join("session-0");
        std::fs::create_dir_all(&sub).unwrap();
        let content_c = b"sub-file";
        std::fs::write(sub.join("trace.log"), content_c).unwrap();

        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 2); // two top-level files
        let expected_bytes =
            content_a.len() as u64 + content_b.len() as u64 + content_c.len() as u64;
        assert_eq!(result.bytes_freed, expected_bytes);
        // dirs_removed: 1 for the subdirectory + 1 for the task directory itself
        assert_eq!(result.dirs_removed, 2);
        assert!(!task_dir.exists());
    }

    #[tokio::test]
    async fn running_task_logs_not_cleaned() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        insert_task(db, "active-1", "running").await;
        age_task(db, "active-1").await;

        let task_dir = tmp.path().join("active-1");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("stdout.log"), "running output").unwrap();

        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 0);
        assert!(task_dir.join("stdout.log").exists());
    }

    #[tokio::test]
    async fn recent_completed_task_not_cleaned() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        // Task is completed but updated_at is "now" (within retention).
        insert_task(db, "recent-1", "completed").await;

        let task_dir = tmp.path().join("recent-1");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("stdout.log"), "recent output").unwrap();

        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 0);
        assert!(task_dir.join("stdout.log").exists());
    }

    #[tokio::test]
    async fn cleanup_handles_missing_log_dir() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        insert_task(db, "ghost-1", "failed").await;
        age_task(db, "ghost-1").await;

        // Do NOT create the log directory — it simply doesn't exist.
        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        assert_eq!(result.files_deleted, 0);
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(result.dirs_removed, 0);
    }

    #[tokio::test]
    async fn cleanup_removes_subdirectories() {
        let mut ts = TestState::new();
        let state = ts.build();
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = &state.async_database;

        insert_task(db, "nested-1", "cancelled").await;
        age_task(db, "nested-1").await;

        let task_dir = tmp.path().join("nested-1");
        // Create nested subdirectories.
        let sub_a = task_dir.join("worktree-a");
        let sub_b = task_dir.join("worktree-b");
        let sub_nested = sub_a.join("deep");
        std::fs::create_dir_all(&sub_nested).unwrap();
        std::fs::create_dir_all(&sub_b).unwrap();

        let file_a = b"aaa";
        let file_b = b"bbbbb";
        let file_deep = b"dd";
        std::fs::write(sub_a.join("a.log"), file_a).unwrap();
        std::fs::write(sub_b.join("b.log"), file_b).unwrap();
        std::fs::write(sub_nested.join("deep.log"), file_deep).unwrap();

        let result = cleanup_old_logs(db, tmp.path(), 7)
            .await
            .expect("cleanup_old_logs");

        // Two top-level subdirectories removed + the task directory itself = 3.
        assert_eq!(result.dirs_removed, 3);
        let expected_bytes = file_a.len() as u64 + file_b.len() as u64 + file_deep.len() as u64;
        assert_eq!(result.bytes_freed, expected_bytes);
        assert!(!task_dir.exists());
    }
}
