//! Auto-cleanup of terminated tasks and their associated data.
//!
//! Batch-deletes tasks in terminal state (completed/failed/cancelled) that
//! are older than a configurable retention period. Cascade-deletes all
//! related items, runs, events, and log files.

use crate::async_database::AsyncDatabase;
use crate::task_repository::{AsyncSqliteTaskRepository, TaskDeleteBlocked};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

/// Clean up terminated tasks older than `retention_days`.
///
/// Cascade-deletes task_items, command_runs, events, disposes of every other
/// reference to the task by the ruling recorded in `task_repository`, and
/// physically removes log files. Returns the number of tasks deleted.
///
/// A task held by a reference with no recorded disposition is skipped whole and
/// named in the log, and the sweep continues to the next task — the same answer
/// the trigger history limit gives, because both go through the same routine.
/// Aborting the batch instead would let one undisposable task stop every later
/// task in the batch from ever being cleaned up.
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

    // Both queries go through the repository rather than through a borrowed
    // connection. What stays in this module is the half the database has no
    // opinion about: which files and directories to unlink, and how many tasks
    // that came to.
    let repo = AsyncSqliteTaskRepository::new(Arc::new(db.clone()));

    let task_ids = repo
        .list_terminal_tasks_older_than(retention_days, limit)
        .await?;

    if task_ids.is_empty() {
        return Ok(0);
    }

    let mut deleted = 0u64;
    let mut skipped = 0u64;
    let logs_dir = logs_dir.to_path_buf();

    for task_id in &task_ids {
        let log_paths = match repo.delete_task_and_collect_log_paths(task_id).await {
            Ok(log_paths) => log_paths,
            Err(error) => match error.downcast::<TaskDeleteBlocked>() {
                Ok(blocked) => {
                    // Named at `warn!` rather than counted: `skipped=3` is a
                    // number with no next action, and the table that held the
                    // task is the fact worth carrying.
                    warn!(
                        task_id = %blocked.task_id,
                        blocked_by = %blocked.blocked_by.join(", "),
                        "task auto-cleanup skipped a task held by an undisposed reference"
                    );
                    skipped += 1;
                    continue;
                }
                Err(error) => return Err(error),
            },
        };

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

    if deleted > 0 || skipped > 0 {
        info!(
            tasks = deleted,
            skipped, retention_days, "task auto-cleanup completed"
        );
    }

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;
    use orchestrator_persistence::test_support;

    async fn insert_task(db: &AsyncDatabase, task_id: &str, status: &str) {
        let id = task_id.to_owned();
        let st = status.to_owned();
        test_support::writer(db)
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
        test_support::writer(db)
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
        test_support::reader(db)
            .call(|conn| {
                let c: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
                Ok(c as u64)
            })
            .await
            .expect("count_tasks")
    }

    async fn task_exists(db: &AsyncDatabase, task_id: &str) -> bool {
        let id = task_id.to_owned();
        test_support::reader(db)
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

    /// Adds a table referencing `tasks(id)` that nobody has ruled on, and pins
    /// `task_id` with it.
    async fn pin_with_unruled_reference(db: &AsyncDatabase, task_id: &str) {
        let id = task_id.to_owned();
        test_support::writer(db)
            .call(move |conn| {
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS later_addition (
                         id TEXT PRIMARY KEY,
                         task_id TEXT NOT NULL,
                         FOREIGN KEY(task_id) REFERENCES tasks(id)
                     );",
                )?;
                conn.execute(
                    "INSERT INTO later_addition (id, task_id) VALUES (?1, ?1)",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await
            .expect("pin with an unruled reference");
    }

    /// FR-168: retention disposes of ruled references rather than aborting.
    ///
    /// The pinned task is seeded *first* so it is the first row the sweep
    /// reaches. Before FR-168 the whole batch died on it and the two tasks
    /// behind it were never cleaned up — and would never have been, on any
    /// later run either, because the sweep hit the same row every time. The
    /// ordering is the fixture: with the pinned task last, a sweep that aborts
    /// and a sweep that skips return the same count.
    #[tokio::test]
    async fn retention_skips_the_unruled_and_cleans_the_rest() {
        let mut ts = TestState::new();
        let state = ts.build();
        let logs_dir = tempfile::tempdir().unwrap();

        for tid in ["t-pinned", "t-after-1", "t-after-2"] {
            insert_task(&state.async_database, tid, "completed").await;
            age_task(&state.async_database, tid, 30).await;
        }
        pin_with_unruled_reference(&state.async_database, "t-pinned").await;

        let deleted = cleanup_old_tasks(&state.async_database, logs_dir.path(), 7, 100)
            .await
            .expect("an unruled reference is a retention outcome, not a sweep failure");

        assert_eq!(
            deleted, 2,
            "the sweep did not get past the task it could not delete"
        );
        assert!(
            task_exists(&state.async_database, "t-pinned").await,
            "a task held by an unruled reference was destroyed anyway"
        );
        for tid in ["t-after-1", "t-after-2"] {
            assert!(
                !task_exists(&state.async_database, tid).await,
                "{tid} was behind the pinned task and never got cleaned up"
            );
        }
    }

    /// FR-168: retention and an explicit delete give the same answer.
    ///
    /// Asserted as *observed behaviour on one fixture* rather than by checking
    /// that both call the same function. A call-graph assertion passes on two
    /// paths that share a routine and then disagree about what to do with its
    /// error, which is exactly the state this FR found.
    #[tokio::test]
    async fn retention_and_explicit_delete_agree_on_the_same_fixture() {
        use orchestrator_persistence::task_repository::AsyncSqliteTaskRepository;

        // Same fixture, twice: once swept by retention, once deleted outright.
        let mut retention_state = TestState::new();
        let retention = retention_state.build();
        let logs_dir = tempfile::tempdir().unwrap();
        insert_task(&retention.async_database, "t-same", "completed").await;
        age_task(&retention.async_database, "t-same", 30).await;
        pin_with_unruled_reference(&retention.async_database, "t-same").await;

        let mut delete_state = TestState::new();
        let explicit = delete_state.build();
        insert_task(&explicit.async_database, "t-same", "completed").await;
        age_task(&explicit.async_database, "t-same", 30).await;
        pin_with_unruled_reference(&explicit.async_database, "t-same").await;

        cleanup_old_tasks(&retention.async_database, logs_dir.path(), 7, 100)
            .await
            .expect("retention treats it as an outcome");
        let repo = AsyncSqliteTaskRepository::new(explicit.async_database.clone());
        let explicit_result = repo.delete_task_and_collect_log_paths("t-same").await;

        assert!(
            task_exists(&retention.async_database, "t-same").await,
            "retention destroyed a task held by an unruled reference"
        );
        assert!(
            explicit_result.is_err(),
            "an explicit delete destroyed what retention refused to touch"
        );
        assert!(
            task_exists(&explicit.async_database, "t-same").await,
            "the refused explicit delete removed the task anyway"
        );

        // The disagreement that would matter is one path succeeding where the
        // other refuses. Both left the task standing, on the same fixture.
        assert!(
            explicit_result
                .as_ref()
                .err()
                .map(|e| e.to_string().contains("later_addition.task_id"))
                .unwrap_or(false),
            "the explicit refusal did not name the reference: {explicit_result:?}"
        );
    }
}
