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

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

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

/// A task the history limit selected but did not remove, and what still names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedTask {
    /// The task that stayed.
    pub task_id: String,
    /// `table.column` for every reference still holding a row, schema order.
    pub blocked_by: Vec<String>,
}

/// What one history-limit sweep did.
///
/// Deleted and skipped are reported separately because they are different
/// facts, not two readings of one number: a sweep that deletes nothing because
/// there was nothing to delete and a sweep that deletes nothing because every
/// candidate is pinned are the same count and opposite situations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HistoryCleanupOutcome {
    /// Tasks removed with their items, command runs and events.
    pub deleted: usize,
    /// Tasks left in place, each with the references that kept them.
    pub skipped: Vec<SkippedTask>,
    /// Log files the caller still has to unlink.
    pub log_paths: Vec<String>,
}

/// The columns that reference `tasks(id)` and would refuse a delete.
///
/// Read from the schema rather than listed here. Ten tables reference
/// `tasks(id)`; `task_graph_runs` and `task_graph_snapshots` declare
/// `ON DELETE CASCADE` and so never refuse anything, and SQLite clears them
/// itself. Of the eight that remain, the task cascade clears exactly one —
/// `task_items`, together with the `command_runs` hanging off it and the
/// `events` rows, which carry no foreign key at all — and that one name is the
/// only literal below.
///
/// A table added later that references `tasks(id)` without a cascade appears
/// in this list on its own. That is the point: a hand-written list of the
/// seven would be correct today and silently short by one the next time
/// somebody adds a table, which is the shape this repository keeps finding.
fn blocking_references(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        r#"SELECT m.name, f."from"
             FROM sqlite_master m
             JOIN pragma_foreign_key_list(m.name) f
            WHERE m.type = 'table'
              AND f."table" = 'tasks'
              AND UPPER(COALESCE(f.on_delete, '')) <> 'CASCADE'
              AND m.name <> 'task_items'
            ORDER BY m.name, f."from""#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Which of `references` currently hold a row naming `task_id`.
fn references_holding(
    conn: &Connection,
    references: &[(String, String)],
    task_id: &str,
) -> Result<Vec<String>> {
    let mut holding = Vec::new();
    for (table, column) in references {
        // Both identifiers come from `sqlite_master` in this same database, not
        // from a caller, and neither can be bound as a parameter.
        let found = conn
            .query_row(
                &format!(r#"SELECT 1 FROM "{table}" WHERE "{column}" = ?1 LIMIT 1"#),
                params![task_id],
                |_| Ok(()),
            )
            .optional()?;
        if found.is_some() {
            holding.push(format!("{table}.{column}"));
        }
    }
    Ok(holding)
}

/// Deletes the tasks a history limit selected, reporting what stayed and why.
///
/// Each task goes through `task_repository`'s cascade — the one
/// `task_cleanup` uses — rather than through a second delete written here, so
/// there is one statement sequence for removing a task and not two. The
/// cascade runs in its own transaction per task, so a task refused partway
/// leaves none of its rows removed and the sweep continues to the next.
///
/// A task still referenced by a table the cascade does not clear is skipped
/// whole and named in the outcome, never stripped of its items and left
/// standing. A failure that is *not* a child row propagates instead of being
/// recorded as a skip: it is not a retention decision and must not read as one.
pub async fn delete_tasks_within_history_limit(
    db: &AsyncDatabase,
    ids: Vec<String>,
) -> Result<HistoryCleanupOutcome> {
    if ids.is_empty() {
        return Ok(HistoryCleanupOutcome::default());
    }
    db.writer()
        .call(move |conn| {
            (|| -> Result<HistoryCleanupOutcome> {
                let references = blocking_references(conn)?;
                let mut outcome = HistoryCleanupOutcome::default();
                for task_id in &ids {
                    match crate::task_repository::delete_task_and_collect_log_paths(conn, task_id) {
                        Ok(log_paths) => {
                            outcome.deleted += 1;
                            outcome.log_paths.extend(log_paths);
                        }
                        Err(error) => {
                            let blocked_by = references_holding(conn, &references, task_id)?;
                            if blocked_by.is_empty() {
                                return Err(error).with_context(|| {
                                    format!(
                                        "delete task {task_id} for history limit: \
                                         nothing references it, so this is not a retention skip"
                                    )
                                });
                            }
                            outcome.skipped.push(SkippedTask {
                                task_id: task_id.clone(),
                                blocked_by,
                            });
                        }
                    }
                }
                Ok(outcome)
            })()
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}
