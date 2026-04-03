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

    async fn age_task(db: &AsyncDatabase, task_id: &str, days: u32) {
        let id = task_id.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    &format!(
                        "UPDATE tasks SET updated_at = datetime('now', '-{days} days') WHERE id = ?1"
                    ),
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
            .expect("age_task");
    }

    async fn count_tasks(db: &AsyncDatabase) -> u64 {
        db.reader()
            .call(|conn| {
                let c: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
                Ok(c as u64)
            })
            .await
            .expect("count_tasks")
    }

    async fn task_exists(db: &AsyncDatabase, task_id: &str) -> bool {
        let id = task_id.to_owned();
        db.reader()
            .call(move |conn| {
                let c: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )?;
                Ok(c > 0)
            })
            .await
            .expect("task_exists")
    }

    #[tokio::test]
    async fn retention_zero_returns_zero() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        insert_task(&state.async_database, "t1", "completed").await;
        age_task(&state.async_database, "t1", 30).await;

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 0, 10)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        // Task should still exist — nothing was cleaned.
        assert!(task_exists(&state.async_database, "t1").await);
    }

    #[tokio::test]
    async fn no_terminal_tasks_returns_zero() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        insert_task(&state.async_database, "t-running", "running").await;
        age_task(&state.async_database, "t-running", 30).await;

        insert_task(&state.async_database, "t-pending", "pending").await;
        age_task(&state.async_database, "t-pending", 30).await;

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 100)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(count_tasks(&state.async_database).await, 2);
    }

    #[tokio::test]
    async fn old_completed_task_deleted() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        insert_task(&state.async_database, "t-old", "completed").await;
        age_task(&state.async_database, "t-old", 30).await;

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 100)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(!task_exists(&state.async_database, "t-old").await);
    }

    #[tokio::test]
    async fn recent_completed_task_not_deleted() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        // Task is completed but was updated just now — within retention window.
        insert_task(&state.async_database, "t-recent", "completed").await;

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 100)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        assert!(task_exists(&state.async_database, "t-recent").await);
    }

    #[tokio::test]
    async fn batch_limit_respected() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        for i in 0..3 {
            let tid = format!("t-batch-{i}");
            insert_task(&state.async_database, &tid, "failed").await;
            age_task(&state.async_database, &tid, 30).await;
        }

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 2)
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        // One task should remain.
        assert_eq!(count_tasks(&state.async_database).await, 1);
    }

    #[tokio::test]
    async fn batch_limit_zero_defaults_to_fifty() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        insert_task(&state.async_database, "t-default", "cancelled").await;
        age_task(&state.async_database, "t-default", 30).await;

        // batch_limit=0 should not fail — it defaults to 50 internally.
        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 0)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(!task_exists(&state.async_database, "t-default").await);
    }

    #[tokio::test]
    async fn log_dir_cleaned_up() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        let task_id = "t-logdir";
        insert_task(&state.async_database, task_id, "completed").await;
        age_task(&state.async_database, task_id, 30).await;

        // Create a log directory with files that should be removed.
        let task_log_dir = logs_dir.path().join(task_id);
        std::fs::create_dir_all(&task_log_dir).unwrap();
        std::fs::write(task_log_dir.join("stdout.log"), "some output").unwrap();
        std::fs::write(task_log_dir.join("stderr.log"), "some errors").unwrap();
        assert!(task_log_dir.exists());

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 100)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(
            !task_log_dir.exists(),
            "task log directory should be removed after cleanup"
        );
    }
}
