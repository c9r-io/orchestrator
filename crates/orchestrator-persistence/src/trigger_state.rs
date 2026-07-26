//! The `trigger_state` table and the task reads the trigger engine makes.
//!
//! `core::trigger_engine` owns the schedule: when a cron entry is due, whether
//! a throttle window has elapsed, which task statuses count as still running,
//! what a trigger's tasks are named, and how many of them a history limit
//! keeps. This module holds the statements those decisions read and write.
//!
//! The engine's reads are advisory — nothing here is fenced, and nothing here
//! is in a transaction, which is also true of the code this replaced. A
//! trigger that fires twice because two daemons read the same state is a
//! duplicate task, not a corrupt one; the deterministic-identity fences that
//! prevent duplicates live in the paths that create tasks, not here.

use anyhow::Result;
use rusqlite::{OptionalExtension, params, params_from_iter};

use crate::async_database::{AsyncDatabase, flatten_err};

/// One trigger fire to record against `(trigger_name, project)`.
#[derive(Debug, Clone)]
pub struct TriggerFire {
    /// Trigger resource name.
    pub trigger_name: String,
    /// Project scope.
    pub project: String,
    /// Task this fire created.
    pub task_id: String,
    /// Status the fire reached.
    pub status: String,
    /// Fire time.
    pub now: String,
}

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

/// Reads when a trigger last fired in a project, as stored.
pub async fn read_last_fired(
    db: &AsyncDatabase,
    trigger_name: String,
    project: String,
) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT last_fired_at FROM trigger_state
                 WHERE trigger_name = ?1 AND project = ?2",
                params![trigger_name, project],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(tokio_rusqlite::Error::from)
        })
        .await
        .map_err(flatten_err)
}

/// Reads the task a trigger last created in a project.
pub async fn read_last_task(
    db: &AsyncDatabase,
    trigger_name: String,
    project: String,
) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT last_task_id FROM trigger_state
                 WHERE trigger_name = ?1 AND project = ?2",
                params![trigger_name, project],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(tokio_rusqlite::Error::from)
        })
        .await
        .map_err(flatten_err)
}

/// Reads the status of the task a trigger last created, in one statement.
///
/// `None` covers three different absences the caller treats alike: the trigger
/// has never fired here, it fired without recording a task, or the task it
/// recorded has since been deleted. Which statuses mean "still running" is the
/// caller's to decide.
pub async fn read_last_task_status(
    db: &AsyncDatabase,
    trigger_name: String,
    project: String,
) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT t.status FROM trigger_state s
                 JOIN tasks t ON t.id = s.last_task_id
                 WHERE s.trigger_name = ?1 AND s.project = ?2",
                params![trigger_name, project],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(tokio_rusqlite::Error::from)
        })
        .await
        .map_err(flatten_err)
}

/// Reads the workflow a task was created under.
pub async fn read_task_workflow(db: &AsyncDatabase, task_id: String) -> Result<Option<String>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT workflow_id FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(tokio_rusqlite::Error::from)
        })
        .await
        .map_err(flatten_err)
}

/// Records a fire, creating the trigger's row the first time and counting up
/// afterwards. The count is incremented in SQL rather than read and rewritten,
/// so two daemons firing the same trigger cannot lose one of the two.
pub async fn record_fire(db: &AsyncDatabase, fire: TriggerFire) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "INSERT INTO trigger_state
                 (trigger_name, project, last_fired_at, fire_count, last_task_id, last_status,
                  created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?6)
                 ON CONFLICT(trigger_name, project) DO UPDATE SET
                   last_fired_at = ?3,
                   fire_count = fire_count + 1,
                   last_task_id = ?4,
                   last_status = ?5,
                   updated_at = ?6",
                params![
                    fire.trigger_name,
                    fire.project,
                    fire.now,
                    fire.task_id,
                    fire.status,
                    fire.now,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Lists the tasks of one name, project and status that fall outside the
/// newest `keep`, oldest first among those kept out.
///
/// The offset is applied by SQLite rather than by reading every historical row
/// and discarding most of them.
pub async fn tasks_beyond_retention(
    db: &AsyncDatabase,
    task_name: String,
    project: String,
    status: String,
    keep: usize,
) -> Result<Vec<String>> {
    db.reader()
        .call(move |conn| {
            (|| -> Result<Vec<String>> {
                let mut stmt = conn.prepare(
                    "SELECT id FROM tasks
                     WHERE name = ?1 AND project_id = ?2 AND status = ?3
                     ORDER BY created_at DESC LIMIT -1 OFFSET ?4",
                )?;
                let rows = stmt
                    .query_map(params![task_name, project, status, keep as i64], |row| {
                        row.get::<_, String>(0)
                    })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Deletes the named task rows and reports how many went.
///
/// This is a bare `DELETE FROM tasks`, which is what it replaced. It does not
/// clear the child rows that reference a task, and the schema does not cascade
/// for all of them, so a task that has any is refused rather than deleted,
/// which is why a trigger history limit has never applied to a task that
/// actually ran. Recorded in DD-148's known limits.
pub async fn delete_tasks(db: &AsyncDatabase, ids: Vec<String>) -> Result<usize> {
    // Saved work, not a guard: SQLite accepts `IN ()` and matches nothing, so
    // removing this early return changes no answer. Measured, not assumed.
    if ids.is_empty() {
        return Ok(0);
    }
    db.writer()
        .call(move |conn| {
            (|| -> Result<usize> {
                let placeholders = (1..=ids.len())
                    .map(|index| format!("?{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                Ok(conn.execute(
                    &format!("DELETE FROM tasks WHERE id IN ({placeholders})"),
                    params_from_iter(ids.iter()),
                )?)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}
