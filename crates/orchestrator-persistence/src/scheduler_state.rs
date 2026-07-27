//! The reads and writes the scheduler makes about task and item state.
//!
//! What lives here is the statements. What deliberately does not is the
//! scheduling decisions they inform — when a cycle counts as stalled, which
//! steps may be skipped after a restart, how deep a spawn chain may go. Those
//! stay in `orchestrator_scheduler`, which reads these and decides.
//!
//! `crates/orchestrator-scheduler/src/scheduler/task_state.rs` is the file
//! DD-147 named as decisive: "eight production driver references inside
//! scheduling logic. It is forbidden. FR-136 states that if even this file may
//! stay where it is, the form chosen is really B; it may not." This module is
//! where those eight went.
//!
//! The statements are the ones that ran in the scheduler before FR-141 B3,
//! transcribed rather than rewritten.

use anyhow::Result;
use rusqlite::params;
use std::collections::HashSet;

use crate::async_database::{AsyncDatabase, flatten_err};
// The item shape is orchestrator-config's, not a second copy of it. That makes
// this the third place the persistence crate reaches that leaf data crate,
// after ConfigOverview::config and RunResult::output; the crate doc says so.
use orchestrator_config::config::NewDynamicItem;

/// One row for `task_execution_metrics`, minus the command-run count this
/// module derives.
#[derive(Debug, Clone)]
pub struct TaskExecutionMetricInput {
    /// Task the metric describes.
    pub task_id: String,
    /// Task status at the time of the sample.
    pub status: String,
    /// Cycle the task was on.
    pub current_cycle: i64,
    /// Items still unresolved.
    pub unresolved_items: i64,
    /// Items in total.
    pub total_items: i64,
    /// Items that failed.
    pub failed_items: i64,
    /// Sample timestamp.
    pub created_at: String,
}

/// Counts the task's command runs and records one execution metric from it.
///
/// The count and the insert are one call because the counted value is a column
/// of the inserted row; splitting them would let the metric record a total the
/// database never held at any single instant.
pub async fn record_task_execution_metric(
    db: &AsyncDatabase,
    metric: TaskExecutionMetricInput,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            let command_runs: i64 = conn.query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
                params![metric.task_id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO task_execution_metrics (task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    metric.task_id,
                    metric.status,
                    metric.current_cycle,
                    metric.unresolved_items,
                    metric.total_items,
                    metric.failed_items,
                    command_runs,
                    metric.created_at
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Marks one item blocked.
pub async fn mark_item_blocked(db: &AsyncDatabase, task_id: String, item_id: String) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE task_items SET status = 'blocked' WHERE id = ?1 AND task_id = ?2",
                params![item_id, task_id],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Returns every blocked item of a task to `unresolved`, and how many moved.
pub async fn reset_blocked_items(db: &AsyncDatabase, task_id: String) -> Result<u64> {
    db.writer()
        .call(move |conn| {
            let count = conn.execute(
                "UPDATE task_items SET status = 'unresolved' WHERE task_id = ?1 AND status = 'blocked'",
                params![task_id],
            )?;
            Ok(count as u64)
        })
        .await
        .map_err(flatten_err)
}

/// The most recent `cycle_started` timestamps for a task, newest first.
pub async fn recent_cycle_timestamps(
    db: &AsyncDatabase,
    task_id: String,
    limit: u32,
) -> Result<Vec<String>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT created_at FROM events WHERE task_id = ?1 AND event_type = 'cycle_started' ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![task_id, limit], |row| row.get(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(flatten_err)
}

/// Whether a `self_restart_ready` event is still unacknowledged.
pub async fn has_unacked_self_restart(db: &AsyncDatabase, task_id: String) -> Result<bool> {
    db.reader()
        .call(move |conn| {
            let has_unacked_restart: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM events
                     WHERE task_id = ?1 AND event_type = 'self_restart_ready'
                     AND id > COALESCE(
                         (SELECT MAX(id) FROM events WHERE task_id = ?1 AND event_type = 'restart_resumed'),
                         0
                     )",
                    params![task_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            Ok(has_unacked_restart)
        })
        .await
        .map_err(flatten_err)
}

/// The step ids already finished in one cycle of a task.
pub async fn completed_steps_in_cycle(
    db: &AsyncDatabase,
    task_id: String,
    cycle: u32,
) -> Result<HashSet<String>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT json_extract(payload_json, '$.step')
                 FROM events
                 WHERE task_id = ?1
                   AND event_type = 'step_finished'
                   AND cycle = ?2
                   AND json_extract(payload_json, '$.step') IS NOT NULL",
            )?;
            let rows = stmt
                .query_map(params![task_id, cycle], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
        .await
        .map_err(flatten_err)
}

/// Marks a still-running command run as killed by the system.
pub async fn mark_command_run_killed(
    db: &AsyncDatabase,
    run_id: String,
    ended_at: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE command_runs SET exit_code = -9, ended_at = ?2 WHERE id = ?1 AND exit_code = -1",
                params![run_id, ended_at],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Records how deep in a spawn chain a task sits.
pub async fn set_spawn_depth(db: &AsyncDatabase, task_id: String, depth: i64) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE tasks SET spawn_depth = ?1 WHERE id = ?2",
                params![depth, task_id],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// The cycle a task is on, or `None` when the task is unknown.
pub async fn current_cycle_for_task(db: &AsyncDatabase, task_id: String) -> Result<Option<i64>> {
    db.reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT current_cycle FROM tasks WHERE id = ?1",
                    params![task_id],
                    |row| row.get::<_, i64>(0),
                )
                .ok())
        })
        .await
        .map_err(flatten_err)
}

/// The project a task belongs to, or `None` when the task is unknown.
///
/// Separate from [`crate::task_repository::queries::project_id_for_task`]
/// because the scheduler treats a missing task as "no project" on an event
/// path, while the daemon treats it as a not-found error on a request path.
/// Collapsing them would make one of the two callers wrong.
pub async fn project_id_for_task_opt(
    db: &AsyncDatabase,
    task_id: String,
) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT project_id FROM tasks WHERE id = ?1",
                    params![task_id],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .await
        .map_err(flatten_err)
}

/// The raw `self_restart_ready` payload for a task, newest first.
///
/// The JSON is returned unparsed: which field names count as the new and old
/// binary hashes is the restart protocol's business, and it has changed once
/// already.
pub async fn latest_self_restart_payload(
    db: &AsyncDatabase,
    task_id: String,
) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT payload_json FROM events WHERE task_id = ?1 AND event_type = 'self_restart_ready' ORDER BY created_at DESC LIMIT 1",
                    params![task_id],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .await
        .map_err(flatten_err)
}

/// Inserts dynamically generated items for a task, returning how many were made.
///
/// `replace` first removes the task's existing `dynamic` items; `static` ones
/// are never touched. New items are appended after the highest existing
/// `order_no`, which is read inside the same call so two concurrent generations
/// cannot both append at the same position.
pub async fn create_dynamic_task_items(
    db: &AsyncDatabase,
    task_id: String,
    items: Vec<NewDynamicItem>,
    replace: bool,
    now: String,
) -> Result<usize> {
    db.writer()
        .call(move |conn| {
            if replace {
                // Remove existing non-static items
                conn.execute(
                    "DELETE FROM task_items WHERE task_id = ?1 AND source = 'dynamic'",
                    params![task_id],
                )?;
            }

            // Get the current max order_no
            let max_order: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(order_no), 0) FROM task_items WHERE task_id = ?1",
                    params![task_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let mut created = 0;
            for (idx, item) in items.iter().enumerate() {
                let id = uuid::Uuid::new_v4().to_string();
                let order_no = max_order + (idx as i64) + 1;
                let dynamic_vars_json = if item.vars.is_empty() {
                    None
                } else {
                    Some(
                        serde_json::to_string(&item.vars)
                            .map_err(|error| tokio_rusqlite::Error::Other(error.into()))?,
                    )
                };

                conn.execute(
                    "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at, dynamic_vars_json, label, source) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5, ?6, ?7, 'dynamic')",
                    params![
                        id,
                        task_id,
                        order_no,
                        item.item_id,
                        now,
                        dynamic_vars_json,
                        item.label,
                    ],
                )?;
                created += 1;
            }
            Ok(created)
        })
        .await
        .map_err(flatten_err)
}
