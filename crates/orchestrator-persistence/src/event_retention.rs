//! Retention queries over the `events` table: what is old enough to go, how
//! much of it there is, and the rows themselves.
//!
//! `core::event_cleanup` owned these statements and also wrote the JSONL
//! archive from inside the writer's callback, which is why every filesystem
//! error there had to be dressed up as a `rusqlite` conversion failure to
//! satisfy the closure's return type. FR-130 B10 moved the statements here and
//! left the archive writing in core, where an I/O error can just be an I/O
//! error.
//!
//! Two values are interpolated rather than bound: SQLite accepts no parameter
//! inside `datetime()`'s modifier string, nor after `LIMIT` in this position.
//! Both are `u32` in the signatures, which is what keeps that safe.

use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::dto::EventDto;

/// Task statuses whose events are eligible for retention cleanup.
///
/// A task still running may yet produce a reader for its own history, so only
/// terminal tasks qualify.
const TERMINAL_STATUSES: &str = "'completed','failed','cancelled'";

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

/// One event row selected for archival, with the columns the archive records.
#[derive(Debug, Clone)]
pub struct ArchivableEvent {
    /// SQLite rowid, the handle the delete uses.
    pub rowid: i64,
    /// Owning task.
    pub task_id: String,
    /// Owning task item, when the event has one.
    pub task_item_id: Option<String>,
    /// Event type name.
    pub event_type: String,
    /// Raw JSON payload, carried through unparsed.
    pub payload_json: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Promoted `step` field, when the payload carried one.
    pub step: Option<String>,
    /// Promoted `step_scope` field, when the payload carried one.
    pub step_scope: Option<String>,
    /// Promoted `cycle` field, when the payload carried one.
    pub cycle: Option<i64>,
}

/// Deletes at most `batch_limit` events older than `retention_days` whose owning
/// task is terminal, and reports how many went.
///
/// Batched so one invocation cannot hold the write lock for an unbounded time.
pub async fn delete_old_terminal_events(
    db: &AsyncDatabase,
    retention_days: u32,
    batch_limit: u32,
) -> Result<u64> {
    db.writer()
        .call(move |conn| {
            let sql = format!(
                "DELETE FROM events WHERE rowid IN (\
                   SELECT events.rowid FROM events \
                   INNER JOIN tasks ON events.task_id = tasks.id \
                   WHERE events.created_at < datetime('now', '-{retention_days} days') \
                     AND tasks.status IN ({TERMINAL_STATUSES}) \
                   LIMIT {batch_limit}\
                 )"
            );
            Ok(conn.execute(&sql, [])? as u64)
        })
        .await
        .map_err(flatten_err)
}

/// Counts the events [`delete_old_terminal_events`] would be eligible to delete,
/// ignoring its batch limit.
pub async fn count_old_terminal_events(db: &AsyncDatabase, retention_days: u32) -> Result<u64> {
    db.reader()
        .call(move |conn| {
            let sql = format!(
                "SELECT COUNT(*) FROM events \
                 INNER JOIN tasks ON events.task_id = tasks.id \
                 WHERE events.created_at < datetime('now', '-{retention_days} days') \
                   AND tasks.status IN ({TERMINAL_STATUSES})"
            );
            let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
            Ok(count as u64)
        })
        .await
        .map_err(flatten_err)
}

/// Lists one task's most recent events, newest first, optionally narrowed to
/// event types starting with `type_prefix`.
pub async fn list_task_events(
    db: &AsyncDatabase,
    task_id: String,
    type_prefix: Option<String>,
    limit: u32,
) -> Result<Vec<EventDto>> {
    db.reader()
        .call(move |conn| {
            list_task_events_blocking(conn, &task_id, type_prefix.as_deref(), limit)
                .map_err(Into::into)
        })
        .await
        .map_err(flatten_err)
}

fn list_task_events_blocking(
    conn: &Connection,
    task_id: &str,
    type_prefix: Option<&str>,
    limit: u32,
) -> rusqlite::Result<Vec<EventDto>> {
    let columns = "SELECT id, task_id, task_item_id, event_type, payload_json, created_at \
                   FROM events";
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match type_prefix {
        Some(prefix) => (
            format!(
                "{columns} WHERE task_id = ?1 AND event_type LIKE ?2 ORDER BY id DESC LIMIT {limit}"
            ),
            vec![
                Box::new(task_id.to_string()),
                Box::new(format!("{prefix}%")),
            ],
        ),
        None => (
            format!("{columns} WHERE task_id = ?1 ORDER BY id DESC LIMIT {limit}"),
            vec![Box::new(task_id.to_string())],
        ),
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
}

/// Computes aggregate statistics for the events table.
pub async fn event_stats(db: &AsyncDatabase) -> Result<EventStats> {
    db.reader()
        .call(|conn| event_stats_blocking(conn).map_err(Into::into))
        .await
        .map_err(flatten_err)
}

fn event_stats_blocking(conn: &Connection) -> rusqlite::Result<EventStats> {
    let total_rows: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
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
}

/// Selects at most `batch_limit` events eligible for archival — the same
/// predicate [`delete_old_terminal_events`] deletes by.
pub async fn select_archivable_events(
    db: &AsyncDatabase,
    retention_days: u32,
    batch_limit: u32,
) -> Result<Vec<ArchivableEvent>> {
    db.writer()
        .call(move |conn| {
            let sql = format!(
                "SELECT events.rowid, events.task_id, events.task_item_id, \
                        events.event_type, events.payload_json, events.created_at, \
                        events.step, events.step_scope, events.cycle \
                 FROM events \
                 INNER JOIN tasks ON events.task_id = tasks.id \
                 WHERE events.created_at < datetime('now', '-{retention_days} days') \
                   AND tasks.status IN ({TERMINAL_STATUSES}) \
                 LIMIT {batch_limit}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(ArchivableEvent {
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
            Ok(rows)
        })
        .await
        .map_err(flatten_err)
}

/// Deletes the named events by rowid and reports how many rows went.
///
/// An empty list deletes nothing and says so, rather than building
/// `IN ()` — which SQLite rejects outright.
pub async fn delete_events_by_rowid(db: &AsyncDatabase, rowids: Vec<i64>) -> Result<u64> {
    if rowids.is_empty() {
        return Ok(0);
    }
    db.writer()
        .call(move |conn| {
            // The values are `i64` read back out of `rowid`, so rendering them
            // into the statement cannot carry anything but digits.
            let rendered: Vec<String> = rowids.iter().map(|id| id.to_string()).collect();
            let sql = format!("DELETE FROM events WHERE rowid IN ({})", rendered.join(","));
            Ok(conn.execute(&sql, [])? as u64)
        })
        .await
        .map_err(flatten_err)
}
