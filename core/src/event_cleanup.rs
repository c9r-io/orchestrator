//! TTL-based event cleanup, optional archival, and statistics.
//!
//! The retention *statements* live in
//! `orchestrator_persistence::event_retention` (FR-130 B10). What stayed here is
//! the archive: grouping rows by task and date, rendering a JSONL line, and
//! writing files. That work used to run inside the SQLite writer's closure, so
//! every `create_dir_all` and `writeln!` failure had to be wrapped as a driver
//! type-conversion failure to satisfy the closure's return type — a disk error
//! reported as a type-conversion error. Out here it is just `anyhow`.

use crate::async_database::AsyncDatabase;
use crate::dto::EventDto;
use anyhow::{Context, Result};
use orchestrator_persistence::event_retention as retention;
use std::path::Path;
use tracing::info;

pub use orchestrator_persistence::event_retention::EventStats;

/// Delete events older than `retention_days` whose owning task is in a terminal
/// status. At most `batch_limit` rows are deleted per invocation to avoid long
/// write-lock durations.
///
/// Returns the number of rows deleted.
pub async fn cleanup_old_events(
    db: &AsyncDatabase,
    retention_days: u32,
    batch_limit: u32,
) -> Result<u64> {
    let deleted = retention::delete_old_terminal_events(db, retention_days, batch_limit).await?;
    if deleted > 0 {
        info!(deleted, retention_days, "event cleanup: deleted old events");
    }
    Ok(deleted)
}

/// Count events that would be deleted by `cleanup_old_events` without actually
/// deleting them (dry-run).
pub async fn count_pending_cleanup(db: &AsyncDatabase, retention_days: u32) -> Result<u64> {
    retention::count_old_terminal_events(db, retention_days).await
}

/// List events for a specific task, optionally filtered by event type prefix.
pub async fn list_task_events(
    db: &AsyncDatabase,
    task_id: &str,
    event_type_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<EventDto>> {
    let limit = if limit == 0 { 50 } else { limit };
    retention::list_task_events(
        db,
        task_id.to_string(),
        event_type_filter.map(str::to_string),
        limit,
    )
    .await
}

/// Compute aggregate statistics for the events table.
pub async fn event_stats(db: &AsyncDatabase) -> Result<EventStats> {
    retention::event_stats(db).await
}

/// Archive events eligible for cleanup to JSONL files, then delete them.
///
/// Events are written to `{archive_dir}/{task_id}/{date}.jsonl` with one JSON
/// object per line. Returns the number of events archived and deleted.
///
/// Selection and deletion are two calls rather than one, and the files are
/// written between them. That ordering is deliberate: a crash after the write
/// leaves the events still in the table, so the next run archives them again —
/// duplicate lines in an append-only file, which a reader can dedupe. The other
/// order would lose them.
pub async fn archive_events(
    db: &AsyncDatabase,
    archive_dir: &Path,
    retention_days: u32,
    batch_limit: u32,
) -> Result<u64> {
    let rows = retention::select_archivable_events(db, retention_days, batch_limit).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let rowids: Vec<i64> = rows.iter().map(|row| row.rowid).collect();
    write_archive_files(archive_dir, &rows)?;
    retention::delete_events_by_rowid(db, rowids).await?;
    let archived = rows.len() as u64;
    info!(
        archived,
        retention_days, "event cleanup: archived and deleted events"
    );
    Ok(archived)
}

/// Groups archivable events by `{task_id}/{date}` and appends one JSON line per
/// event to the matching file.
fn write_archive_files(archive_dir: &Path, rows: &[retention::ArchivableEvent]) -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Write;

    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        // The date is the first 10 characters of an RFC 3339 timestamp. A
        // shorter string is not one, and grouping it whole is better than
        // slicing into a character boundary that may not exist.
        let date = if row.created_at.len() >= 10 {
            &row.created_at[..10]
        } else {
            row.created_at.as_str()
        };
        let line = serde_json::json!({
            "task_id": row.task_id,
            "task_item_id": row.task_item_id,
            "event_type": row.event_type,
            "payload_json": row.payload_json,
            "created_at": row.created_at,
            "step": row.step,
            "step_scope": row.step_scope,
            "cycle": row.cycle,
        });
        grouped
            .entry(format!("{}/{date}", row.task_id))
            .or_default()
            .push(line.to_string());
    }

    for (key, lines) in &grouped {
        let path = archive_dir.join(format!("{key}.jsonl"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create event archive directory {}", parent.display()))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open event archive {}", path.display()))?;
        for line in lines {
            writeln!(file, "{line}")
                .with_context(|| format!("append to event archive {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    /// Helper: insert a task row directly.
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

    /// Helper: insert an event with a specific created_at timestamp.
    async fn insert_event(db: &AsyncDatabase, task_id: &str, event_type: &str, created_at: &str) {
        let tid = task_id.to_owned();
        let et = event_type.to_owned();
        let ca = created_at.to_owned();
        db.writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO events (task_id, event_type, payload_json, created_at) \
                     VALUES (?1, ?2, '{}', ?3)",
                    rusqlite::params![tid, et, ca],
                )?;
                Ok(())
            })
            .await
            .expect("insert_event");
    }

    /// Helper: count all events.
    async fn count_events(db: &AsyncDatabase) -> u64 {
        db.reader()
            .call(|conn| {
                let c: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
                Ok(c as u64)
            })
            .await
            .expect("count_events")
    }

    #[tokio::test]
    async fn cleanup_deletes_only_terminal_old_events() {
        let mut ts = TestState::new();
        let state = ts.build();

        // completed task with old event — should be cleaned
        insert_task(&state.async_database, "t-done", "completed").await;
        insert_event(
            &state.async_database,
            "t-done",
            "step_start",
            "2020-01-01T00:00:00",
        )
        .await;

        // running task with old event — should NOT be cleaned
        insert_task(&state.async_database, "t-running", "running").await;
        insert_event(
            &state.async_database,
            "t-running",
            "step_start",
            "2020-01-01T00:00:00",
        )
        .await;

        // completed task with recent event — should NOT be cleaned (within retention)
        insert_task(&state.async_database, "t-recent", "completed").await;
        insert_event(
            &state.async_database,
            "t-recent",
            "step_start",
            "2099-01-01T00:00:00",
        )
        .await;

        assert_eq!(count_events(&state.async_database).await, 3);

        let deleted = cleanup_old_events(&state.async_database, 1, 1000)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(count_events(&state.async_database).await, 2);
    }

    #[tokio::test]
    async fn cleanup_respects_batch_limit() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-done", "completed").await;
        for i in 0..5 {
            insert_event(
                &state.async_database,
                "t-done",
                &format!("ev_{i}"),
                "2020-01-01T00:00:00",
            )
            .await;
        }
        assert_eq!(count_events(&state.async_database).await, 5);

        let deleted = cleanup_old_events(&state.async_database, 1, 2)
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(count_events(&state.async_database).await, 3);
    }

    #[tokio::test]
    async fn count_pending_cleanup_returns_correct_count() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-fail", "failed").await;
        insert_event(&state.async_database, "t-fail", "e1", "2020-01-01T00:00:00").await;
        insert_event(&state.async_database, "t-fail", "e2", "2020-01-02T00:00:00").await;

        insert_task(&state.async_database, "t-run", "running").await;
        insert_event(&state.async_database, "t-run", "e3", "2020-01-01T00:00:00").await;

        let count = count_pending_cleanup(&state.async_database, 1)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn event_stats_returns_expected_values() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t1", "completed").await;
        insert_event(&state.async_database, "t1", "a", "2024-01-01T00:00:00").await;
        insert_event(&state.async_database, "t1", "b", "2024-06-01T00:00:00").await;

        insert_task(&state.async_database, "t2", "running").await;
        insert_event(&state.async_database, "t2", "c", "2024-03-01T00:00:00").await;

        let stats = event_stats(&state.async_database).await.unwrap();
        assert_eq!(stats.total_rows, 3);
        assert_eq!(stats.earliest.as_deref(), Some("2024-01-01T00:00:00"));
        assert_eq!(stats.latest.as_deref(), Some("2024-06-01T00:00:00"));
        assert!(stats.by_task_status.len() >= 2);
    }

    #[tokio::test]
    async fn archive_events_writes_jsonl_and_deletes() {
        let mut ts = TestState::new();
        let state = ts.build();
        let archive_dir =
            std::env::temp_dir().join(format!("archive-test-{}", uuid::Uuid::new_v4()));

        insert_task(&state.async_database, "t-arch", "cancelled").await;
        insert_event(&state.async_database, "t-arch", "e1", "2020-06-15T10:00:00").await;
        insert_event(&state.async_database, "t-arch", "e2", "2020-06-15T11:00:00").await;

        assert_eq!(count_events(&state.async_database).await, 2);

        let archived = archive_events(&state.async_database, &archive_dir, 1, 1000)
            .await
            .unwrap();
        assert_eq!(archived, 2);
        assert_eq!(count_events(&state.async_database).await, 0);

        // Verify JSONL file exists and has 2 lines
        let jsonl_path = archive_dir.join("t-arch/2020-06-15.jsonl");
        assert!(jsonl_path.exists(), "JSONL file should exist");
        let content = std::fs::read_to_string(&jsonl_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&archive_dir);
    }

    #[tokio::test]
    async fn cleanup_with_zero_retention_deletes_recent_terminal_events() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-done", "completed").await;
        // An event from just a second ago — retention_days=0 means "older than now"
        // so any past event qualifies
        insert_event(
            &state.async_database,
            "t-done",
            "step_start",
            "2025-01-01T00:00:00",
        )
        .await;

        insert_task(&state.async_database, "t-running", "running").await;
        insert_event(
            &state.async_database,
            "t-running",
            "step_start",
            "2025-01-01T00:00:00",
        )
        .await;

        assert_eq!(count_events(&state.async_database).await, 2);

        // retention_days=0 means "older than now minus 0 days" — any past event qualifies
        let deleted = cleanup_old_events(&state.async_database, 0, 1000)
            .await
            .unwrap();
        // The completed task's event should be deleted; running task's should remain
        assert_eq!(deleted, 1);
        assert_eq!(count_events(&state.async_database).await, 1);
    }

    #[tokio::test]
    async fn event_stats_on_empty_database() {
        let mut ts = TestState::new();
        let state = ts.build();

        let stats = event_stats(&state.async_database).await.unwrap();
        assert_eq!(stats.total_rows, 0);
        assert_eq!(stats.earliest, None);
        assert_eq!(stats.latest, None);
        assert!(stats.by_task_status.is_empty());
    }

    #[tokio::test]
    async fn archive_events_with_no_eligible_events() {
        let mut ts = TestState::new();
        let state = ts.build();
        let archive_dir =
            std::env::temp_dir().join(format!("archive-empty-{}", uuid::Uuid::new_v4()));

        // Running task — not terminal, so nothing to archive
        insert_task(&state.async_database, "t-run", "running").await;
        insert_event(&state.async_database, "t-run", "e1", "2020-01-01T00:00:00").await;

        let archived = archive_events(&state.async_database, &archive_dir, 1, 1000)
            .await
            .unwrap();
        assert_eq!(archived, 0);
        assert_eq!(count_events(&state.async_database).await, 1);

        // Archive dir should not have been created since no events were archived
        assert!(!archive_dir.exists());
    }

    #[tokio::test]
    async fn archive_events_groups_by_date() {
        let mut ts = TestState::new();
        let state = ts.build();
        let archive_dir =
            std::env::temp_dir().join(format!("archive-dates-{}", uuid::Uuid::new_v4()));

        insert_task(&state.async_database, "t-multi", "completed").await;
        // Events on two different dates
        insert_event(
            &state.async_database,
            "t-multi",
            "e1",
            "2020-06-15T10:00:00",
        )
        .await;
        insert_event(
            &state.async_database,
            "t-multi",
            "e2",
            "2020-06-16T12:00:00",
        )
        .await;
        insert_event(
            &state.async_database,
            "t-multi",
            "e3",
            "2020-06-15T14:00:00",
        )
        .await;

        let archived = archive_events(&state.async_database, &archive_dir, 1, 1000)
            .await
            .unwrap();
        assert_eq!(archived, 3);
        assert_eq!(count_events(&state.async_database).await, 0);

        // Two separate date files
        let path_15 = archive_dir.join("t-multi/2020-06-15.jsonl");
        let path_16 = archive_dir.join("t-multi/2020-06-16.jsonl");
        assert!(path_15.exists(), "JSONL for 2020-06-15 should exist");
        assert!(path_16.exists(), "JSONL for 2020-06-16 should exist");

        let content_15 = std::fs::read_to_string(&path_15).unwrap();
        let lines_15: Vec<&str> = content_15.trim().lines().collect();
        assert_eq!(lines_15.len(), 2, "Two events on 2020-06-15");

        let content_16 = std::fs::read_to_string(&path_16).unwrap();
        let lines_16: Vec<&str> = content_16.trim().lines().collect();
        assert_eq!(lines_16.len(), 1, "One event on 2020-06-16");

        let _ = std::fs::remove_dir_all(&archive_dir);
    }

    #[tokio::test]
    async fn count_pending_cleanup_with_zero_results() {
        let mut ts = TestState::new();
        let state = ts.build();

        // No tasks or events at all
        let count = count_pending_cleanup(&state.async_database, 1)
            .await
            .unwrap();
        assert_eq!(count, 0);

        // Add a running task with old events — still zero eligible
        insert_task(&state.async_database, "t-run", "running").await;
        insert_event(&state.async_database, "t-run", "e1", "2020-01-01T00:00:00").await;
        let count = count_pending_cleanup(&state.async_database, 1)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn cleanup_deletes_failed_and_cancelled_tasks() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-fail", "failed").await;
        insert_event(
            &state.async_database,
            "t-fail",
            "err",
            "2020-01-01T00:00:00",
        )
        .await;

        insert_task(&state.async_database, "t-cancel", "cancelled").await;
        insert_event(
            &state.async_database,
            "t-cancel",
            "cancel_ev",
            "2020-01-01T00:00:00",
        )
        .await;

        insert_task(&state.async_database, "t-pending", "pending").await;
        insert_event(
            &state.async_database,
            "t-pending",
            "pending_ev",
            "2020-01-01T00:00:00",
        )
        .await;

        assert_eq!(count_events(&state.async_database).await, 3);

        let deleted = cleanup_old_events(&state.async_database, 1, 1000)
            .await
            .unwrap();
        // failed + cancelled = 2 deleted; pending remains
        assert_eq!(deleted, 2);
        assert_eq!(count_events(&state.async_database).await, 1);
    }

    #[tokio::test]
    async fn cleanup_with_no_events_returns_zero() {
        let mut ts = TestState::new();
        let state = ts.build();

        let deleted = cleanup_old_events(&state.async_database, 1, 1000)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn list_task_events_without_filter() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-list", "running").await;
        insert_event(
            &state.async_database,
            "t-list",
            "step_start",
            "2024-01-01T00:00:00",
        )
        .await;
        insert_event(
            &state.async_database,
            "t-list",
            "step_end",
            "2024-01-02T00:00:00",
        )
        .await;

        let events = list_task_events(&state.async_database, "t-list", None, 50)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
        // Results are ordered by id DESC, so most recent first
        assert_eq!(events[0].event_type, "step_end");
        assert_eq!(events[1].event_type, "step_start");
    }

    #[tokio::test]
    async fn list_task_events_with_type_filter() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-filter", "running").await;
        insert_event(
            &state.async_database,
            "t-filter",
            "step_start",
            "2024-01-01T00:00:00",
        )
        .await;
        insert_event(
            &state.async_database,
            "t-filter",
            "step_end",
            "2024-01-02T00:00:00",
        )
        .await;
        insert_event(
            &state.async_database,
            "t-filter",
            "error_occurred",
            "2024-01-03T00:00:00",
        )
        .await;

        let events = list_task_events(&state.async_database, "t-filter", Some("step"), 50)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.event_type.starts_with("step")));
    }

    #[tokio::test]
    async fn list_task_events_with_zero_limit_defaults_to_50() {
        let mut ts = TestState::new();
        let state = ts.build();

        insert_task(&state.async_database, "t-zero", "running").await;
        insert_event(
            &state.async_database,
            "t-zero",
            "ev1",
            "2024-01-01T00:00:00",
        )
        .await;

        // limit=0 should default to 50 internally and still return the event
        let events = list_task_events(&state.async_database, "t-zero", None, 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn list_task_events_for_nonexistent_task() {
        let mut ts = TestState::new();
        let state = ts.build();

        let events = list_task_events(&state.async_database, "no-such-task", None, 50)
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn archive_events_across_multiple_tasks() {
        let mut ts = TestState::new();
        let state = ts.build();
        let archive_dir =
            std::env::temp_dir().join(format!("archive-multi-{}", uuid::Uuid::new_v4()));

        insert_task(&state.async_database, "t-a", "completed").await;
        insert_task(&state.async_database, "t-b", "failed").await;

        insert_event(&state.async_database, "t-a", "e1", "2020-03-10T08:00:00").await;
        insert_event(&state.async_database, "t-b", "e2", "2020-03-10T09:00:00").await;

        let archived = archive_events(&state.async_database, &archive_dir, 1, 1000)
            .await
            .unwrap();
        assert_eq!(archived, 2);
        assert_eq!(count_events(&state.async_database).await, 0);

        // Each task gets its own subdirectory
        assert!(archive_dir.join("t-a/2020-03-10.jsonl").exists());
        assert!(archive_dir.join("t-b/2020-03-10.jsonl").exists());

        let _ = std::fs::remove_dir_all(&archive_dir);
    }
}
