//! Row access for the `events` table.
//!
//! Split out of `core::events` in FR-130 Phase B. The division is deliberate and
//! is where the seam in that module already was: everything here returns rows
//! and strings, and everything the caller does with them — parsing
//! `payload_json`, inferring a step scope, deciding what a missing field means —
//! stays in core, because none of it is a database concern.
//!
//! Which event types count as step-related is also the caller's decision, so it
//! is a parameter rather than a literal in a `WHERE` clause here. The alternative
//! puts a domain policy inside SQL, where changing it means editing this crate.

use anyhow::Result;
use rusqlite::{Connection, params, params_from_iter};

/// One `events` row, before any interpretation of its payload.
#[derive(Debug, Clone)]
pub struct StepEventRow {
    /// Event type label.
    pub event_type: String,
    /// Raw payload JSON, uninterpreted.
    pub payload_json: String,
    /// Creation timestamp as stored.
    pub created_at: String,
    /// Task-item identifier for item-scoped events.
    pub task_item_id: Option<String>,
    /// Promoted `step` column, when the row has one.
    pub step: Option<String>,
    /// Promoted `step_scope` column, when the row has one.
    pub step_scope: Option<String>,
}

/// Returns the payload of the most recent spawn or start event for a task.
///
/// `None` covers both "no such row" and a read that failed, matching the
/// behaviour of the call site this replaced: a missing log path is a normal
/// answer for a task that has not started a step yet, not an error to surface.
pub(crate) fn latest_step_spawn_payload(conn: &Connection, task_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT payload_json FROM events
         WHERE task_id = ?1 AND event_type IN ('step_spawned', 'step_started')
         ORDER BY id DESC LIMIT 1",
        params![task_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Returns every row for a task whose `event_type` is in `event_types`, oldest first.
pub(crate) fn step_event_rows(
    conn: &Connection,
    task_id: &str,
    event_types: &[&str],
) -> Result<Vec<StepEventRow>> {
    if event_types.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = (2..event_types.len() + 2)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT event_type, payload_json, created_at, task_item_id, step, step_scope FROM events
         WHERE task_id = ?1 AND event_type IN ({placeholders})
         ORDER BY id ASC"
    );

    let mut bindings: Vec<&str> = Vec::with_capacity(event_types.len() + 1);
    bindings.push(task_id);
    bindings.extend_from_slice(event_types);

    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map(params_from_iter(bindings), |row| {
            Ok(StepEventRow {
                event_type: row.get(0)?,
                payload_json: row.get(1)?,
                created_at: row.get(2)?,
                task_item_id: row.get(3)?,
                step: row.get(4)?,
                step_scope: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Async form of [`latest_step_spawn_payload`], on the shared reader connection.
pub async fn latest_step_spawn_payload_async(
    db: &crate::async_database::AsyncDatabase,
    task_id: &str,
) -> Result<Option<String>> {
    let task_id = task_id.to_owned();
    db.reader()
        .call(move |conn| Ok(latest_step_spawn_payload(conn, &task_id)))
        .await
        .map_err(crate::async_database::flatten_err)
}

/// Async form of [`step_event_rows`], on the shared reader connection.
pub async fn step_event_rows_async(
    db: &crate::async_database::AsyncDatabase,
    task_id: &str,
    event_types: &[&str],
) -> Result<Vec<StepEventRow>> {
    let task_id = task_id.to_owned();
    let event_types: Vec<String> = event_types.iter().map(|kind| kind.to_string()).collect();
    db.reader()
        .call(move |conn| {
            let borrowed: Vec<&str> = event_types.iter().map(String::as_str).collect();
            step_event_rows(conn, &task_id, &borrowed)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(crate::async_database::flatten_err)
}

/// The latest step-spawn payload for a task, read through a connection this
/// module opens.
pub fn latest_step_spawn_payload_by_path(
    db_path: &std::path::Path,
    task_id: &str,
) -> anyhow::Result<Option<String>> {
    let conn = crate::sqlite::open_conn(db_path)?;
    Ok(latest_step_spawn_payload(&conn, task_id))
}

/// Step-event rows for a task, read through a connection this module opens.
pub fn step_event_rows_by_path(
    db_path: &std::path::Path,
    task_id: &str,
    event_types: &[&str],
) -> anyhow::Result<Vec<StepEventRow>> {
    let conn = crate::sqlite::open_conn(db_path)?;
    step_event_rows(&conn, task_id, event_types)
}
