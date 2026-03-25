//! TTL-based event cleanup, optional archival, and statistics.
//!
//! Provides functions to purge old events for completed/failed/cancelled tasks,
//! optionally archiving them to JSONL before deletion.

use crate::async_database::AsyncDatabase;
use crate::dto::EventDto;
use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use tracing::info;

/// Aggregate statistics about the events table.
#[derive(Debug, Clone)]
pub struct EventStats {
    /// Total number of rows in the events table.
    pub total_rows: u64,
    /// Earliest `created_at` timestamp, if any events exist.
    pub earliest: Option<String>,
    /// Latest `created_at` timestamp, if any events exist.
    pub latest: Option<String>,
    /// Event counts grouped by the owning task's status.
    pub by_task_status: Vec<(String, u64)>,
}

/// Terminal task statuses whose events are eligible for cleanup.
const TERMINAL_STATUSES: &str = "'completed','failed','cancelled'";

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
    let days = retention_days;
    let limit = batch_limit;
    let deleted: u64 = db
        .writer()
        .call(move |conn| {
            let sql = format!(
                "DELETE FROM events WHERE rowid IN (\
                   SELECT events.rowid FROM events \
                   INNER JOIN tasks ON events.task_id = tasks.id \
                   WHERE events.created_at < datetime('now', '-{days} days') \
                     AND tasks.status IN ({TERMINAL_STATUSES}) \
                   LIMIT {limit}\
                 )"
            );
            let count = conn.execute(&sql, [])?;
            Ok(count as u64)
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if deleted > 0 {
        info!(deleted, retention_days, "event cleanup: deleted old events");
    }
    Ok(deleted)
}

/// Count events that would be deleted by `cleanup_old_events` without actually
/// deleting them (dry-run).
pub async fn count_pending_cleanup(db: &AsyncDatabase, retention_days: u32) -> Result<u64> {
    let days = retention_days;
    let count: u64 = db
        .reader()
        .call(move |conn| {
            let sql = format!(
                "SELECT COUNT(*) FROM events \
                 INNER JOIN tasks ON events.task_id = tasks.id \
                 WHERE events.created_at < datetime('now', '-{days} days') \
                   AND tasks.status IN ({TERMINAL_STATUSES})"
            );
            let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
            Ok(count as u64)
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(count)
}

/// Gather statistics about the events table.
/// List events for a specific task, optionally filtered by event type prefix.
pub async fn list_task_events(
    db: &AsyncDatabase,
    task_id: &str,
    event_type_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<EventDto>> {
    let task_id = task_id.to_string();
    let type_filter = event_type_filter.map(|s| s.to_string());
    let limit = if limit == 0 { 50 } else { limit };
    let events = db
        .reader()
        .call(move |conn| {
            let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(
                ref prefix,
            ) = type_filter
            {
                (
                    format!(
                        "SELECT id, task_id, task_item_id, event_type, payload_json, created_at \
                             FROM events WHERE task_id = ?1 AND event_type LIKE ?2 \
                             ORDER BY id DESC LIMIT {limit}"
                    ),
                    vec![Box::new(task_id.clone()), Box::new(format!("{prefix}%"))],
                )
            } else {
                (
                    format!(
                        "SELECT id, task_id, task_item_id, event_type, payload_json, created_at \
                             FROM events WHERE task_id = ?1 \
                             ORDER BY id DESC LIMIT {limit}"
                    ),
                    vec![Box::new(task_id.clone())],
                )
            };
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                    let payload_str: String = row.get(4)?;
                    let payload: Value = serde_json::from_str(&payload_str).unwrap_or(Value::Null);
                    Ok(EventDto {
                        id: row.get(0)?,
                        task_id: row.get(1)?,
                        task_item_id: row.get(2)?,
                        event_type: row.get(3)?,
                        payload,
                        created_at: row.get(5)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(events)
}

/// Compute aggregate statistics for the events table.
pub async fn event_stats(db: &AsyncDatabase) -> Result<EventStats> {
    let stats = db
        .reader()
        .call(|conn| {
            let total_rows: i64 =
                conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
            let earliest: Option<String> = conn
                .query_row("SELECT MIN(created_at) FROM events", [], |row| row.get(0))
                .unwrap_or(None);
            let latest: Option<String> = conn
                .query_row("SELECT MAX(created_at) FROM events", [], |row| row.get(0))
                .unwrap_or(None);

            let mut stmt = conn.prepare(
                "SELECT COALESCE(t.status, 'unknown'), COUNT(*) \
                 FROM events e \
                 LEFT JOIN tasks t ON e.task_id = t.id \
                 GROUP BY t.status \
                 ORDER BY COUNT(*) DESC",
            )?;
            let by_task_status: Vec<(String, u64)> = stmt
                .query_map([], |row| {
                    let status: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((status, count as u64))
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(EventStats {
                total_rows: total_rows as u64,
                earliest,
                latest,
                by_task_status,
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(stats)
}

/// Archive events eligible for cleanup to JSONL files, then delete them.
///
/// Events are written to `{archive_dir}/{task_id}/{date}.jsonl` with one JSON
/// object per line. Returns the number of events archived and deleted.
pub async fn archive_events(
    db: &AsyncDatabase,
    archive_dir: &Path,
    retention_days: u32,
    batch_limit: u32,
) -> Result<u64> {
    let dir = archive_dir.to_path_buf();
    let days = retention_days;
    let limit = batch_limit;
    let archived: u64 = db
        .writer()
        .call(move |conn| {
            // Select events to archive
            let sql = format!(
                "SELECT events.rowid, events.task_id, events.task_item_id, \
                        events.event_type, events.payload_json, events.created_at, \
                        events.step, events.step_scope, events.cycle \
                 FROM events \
                 INNER JOIN tasks ON events.task_id = tasks.id \
                 WHERE events.created_at < datetime('now', '-{days} days') \
                   AND tasks.status IN ({TERMINAL_STATUSES}) \
                 LIMIT {limit}"
            );
            let mut stmt = conn.prepare(&sql)?;

            struct ArchiveRow {
                rowid: i64,
                task_id: String,
                task_item_id: Option<String>,
                event_type: String,
                payload_json: String,
                created_at: String,
                step: Option<String>,
                step_scope: Option<String>,
                cycle: Option<i64>,
            }

            let rows: Vec<ArchiveRow> = stmt
                .query_map([], |row| {
                    Ok(ArchiveRow {
                        rowid: row.get(0)?,
                        task_id: row.get(1)?,
                        task_item_id: row.get(2)?,
                        event_type: row.get(3)?,
                        payload_json: row.get(4)?,
                        created_at: row.get(5)?,
                        step: row.get(6)?,
                        step_scope: row.get(7)?,
                        cycle: row.get(8)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            if rows.is_empty() {
                return Ok(0u64);
            }

            // Group by task_id and write JSONL
            use std::collections::HashMap;
            use std::io::Write;
            let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
            let mut rowids = Vec::with_capacity(rows.len());
            for row in &rows {
                let (
                    rowid,
                    task_id,
                    task_item_id,
                    event_type,
                    payload_json,
                    created_at,
                    step,
                    step_scope,
                    cycle,
                ) = (
                    &row.rowid,
                    &row.task_id,
                    &row.task_item_id,
                    &row.event_type,
                    &row.payload_json,
                    &row.created_at,
                    &row.step,
                    &row.step_scope,
                    &row.cycle,
                );
                rowids.push(*rowid);
                // Extract date from created_at (first 10 chars: YYYY-MM-DD)
                let date = if created_at.len() >= 10 {
                    &created_at[..10]
                } else {
                    created_at.as_str()
                };
                let line = serde_json::json!({
                    "task_id": task_id,
                    "task_item_id": task_item_id,
                    "event_type": event_type,
                    "payload_json": payload_json,
                    "created_at": created_at,
                    "step": step,
                    "step_scope": step_scope,
                    "cycle": cycle,
                });
                let key = format!("{task_id}/{date}");
                grouped.entry(key).or_default().push(line.to_string());
            }
            for (key, lines) in &grouped {
                let path = dir.join(format!("{key}.jsonl"));
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                }
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                for line in lines {
                    writeln!(f, "{line}")
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                }
            }

            // Delete archived events by rowid
            let placeholders: Vec<String> = rowids.iter().map(|id| id.to_string()).collect();
            let delete_sql = format!(
                "DELETE FROM events WHERE rowid IN ({})",
                placeholders.join(",")
            );
            conn.execute(&delete_sql, [])?;

            Ok(rows.len() as u64)
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if archived > 0 {
        info!(
            archived,
            retention_days, "event cleanup: archived and deleted events"
        );
    }
    Ok(archived)
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
}
