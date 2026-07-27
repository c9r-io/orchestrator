//! Writing a new task, its items and its creation-time events as one commit.
//!
//! `core::task_ops` has two creation paths — a workflow task and a direct
//! `run:` step — that differ only in four column values. Both used to open a
//! connection, start a transaction, and interleave `INSERT` statements with
//! event rows built from domain policy. FR-130 B9 moved the transaction here and
//! left the policy there: this module receives rows that are already decided,
//! and its whole contribution is that they land together or not at all.

use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::types::DbEventRecord;
use super::write_ops::insert_event;
use crate::sqlite::open_conn;

/// One row of `tasks`, with every value already resolved by the caller.
#[derive(Debug, Clone)]
pub struct NewTaskRow {
    /// Task identifier.
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Task goal text.
    pub goal: String,
    /// JSON array of the target files persisted with the task.
    pub target_files_json: String,
    /// Owning project.
    pub project_id: String,
    /// Owning workspace.
    pub workspace_id: String,
    /// Owning workflow, or an `_ephemeral:` label for direct step runs.
    pub workflow_id: String,
    /// Absolute workspace root, rendered to a string.
    pub workspace_root: String,
    /// JSON array of the workspace's QA targets.
    pub qa_targets_json: String,
    /// Ticket directory relative to the workspace root.
    pub ticket_dir: String,
    /// Serialized execution plan.
    pub execution_plan_json: String,
    /// Loop mode label (`once`, and the workflow-configured values).
    pub loop_mode: String,
    /// Creation timestamp, also written as the initial `updated_at`.
    pub created_at: String,
    /// Parent task when this one was spawned by another.
    pub parent_task_id: Option<String>,
    /// Why this task was spawned, when it was.
    pub spawn_reason: Option<String>,
    /// Serialized step filter; empty when the whole plan runs.
    pub step_filter_json: String,
    /// Serialized initial pipeline variables; empty when there are none.
    pub initial_vars_json: String,
    /// Artifacts directory, rendered to a string.
    pub artifacts_dir: String,
}

/// Inserts a task, one `task_items` row per path, and any creation-time events,
/// in a single transaction.
///
/// Atomicity is the reason this is one function rather than three. The events
/// are FR-094 observability that describes *how* the item list was derived, so a
/// commit that carried the items without them would leave a task nobody can
/// explain; the caller could not restore that guarantee by calling three
/// functions in a row.
///
/// Returns the generated task-item identifiers in the order the paths were
/// given.
pub fn insert_task_with_items(
    db_path: &Path,
    task: &NewTaskRow,
    item_paths: &[String],
    events: &[DbEventRecord],
) -> Result<Vec<String>> {
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;
    insert_task_row(&tx, task)?;
    let mut item_ids = Vec::with_capacity(item_paths.len());
    for (index, path) in item_paths.iter().enumerate() {
        let item_id = Uuid::new_v4().to_string();
        insert_task_item_row(&tx, &item_id, &task.id, index, path, &task.created_at)?;
        item_ids.push(item_id);
    }
    for event in events {
        insert_event(&tx, event)?;
    }
    tx.commit()?;
    Ok(item_ids)
}

fn insert_task_row(conn: &Connection, task: &NewTaskRow) -> Result<()> {
    conn.execute(
        "INSERT INTO tasks (id, name, status, started_at, completed_at, goal, target_files_json, mode, project_id, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, resume_token, created_at, updated_at, parent_task_id, spawn_reason, spawn_depth, step_filter_json, initial_vars_json, artifacts_dir) VALUES (?1, ?2, 'created', NULL, NULL, ?3, ?4, '', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, NULL, ?13, ?13, ?14, ?15, 0, ?16, ?17, ?18)",
        params![
            task.id,
            task.name,
            task.goal,
            task.target_files_json,
            task.project_id,
            task.workspace_id,
            task.workflow_id,
            task.workspace_root,
            task.qa_targets_json,
            task.ticket_dir,
            task.execution_plan_json,
            task.loop_mode,
            task.created_at,
            task.parent_task_id,
            task.spawn_reason,
            task.step_filter_json,
            task.initial_vars_json,
            task.artifacts_dir,
        ],
    )?;
    Ok(())
}

fn insert_task_item_row(
    conn: &Connection,
    item_id: &str,
    task_id: &str,
    index: usize,
    path: &str,
    created_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5)",
        params![item_id, task_id, (index as i64) + 1, path, created_at],
    )?;
    Ok(())
}

/// Resets one task item to `pending`, drops its command runs, and returns the
/// task it belongs to.
///
/// The item is named by an exact id or a unique prefix. Resolution happens
/// before the transaction opens, so an ambiguous prefix costs nothing and a
/// resolved one is used consistently by both statements. Command runs go because
/// compensation would otherwise re-finalize the item from stale results.
pub fn reset_task_item(db_path: &Path, id_or_prefix: &str, now: &str) -> Result<String> {
    let conn = open_conn(db_path)?;
    let resolved_id = resolve_task_item_id(&conn, id_or_prefix)?;
    let task_id: String = conn.query_row(
        "SELECT task_id FROM task_items WHERE id = ?1",
        params![resolved_id],
        |row| row.get(0),
    )?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE task_items SET status = 'pending', ticket_files_json = '[]', ticket_content_json = '[]', fix_required = 0, fixed = 0, last_error = '', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![resolved_id, now],
    )?;
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id = ?1",
        params![resolved_id],
    )?;
    tx.commit()?;
    Ok(task_id)
}

/// Resolves a task-item id from an exact match or a unique prefix.
///
/// An exact match wins outright: a full id that happens to prefix a longer one
/// must not be reported as ambiguous.
pub(crate) fn resolve_task_item_id(conn: &Connection, id_or_prefix: &str) -> Result<String> {
    use rusqlite::OptionalExtension;
    let exact: Option<String> = conn
        .query_row(
            "SELECT id FROM task_items WHERE id = ?1",
            params![id_or_prefix],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = exact {
        return Ok(id);
    }
    let pattern = format!("{id_or_prefix}%");
    let mut stmt = conn.prepare("SELECT id FROM task_items WHERE id LIKE ?1")?;
    let matches: Vec<String> = stmt
        .query_map(params![pattern], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    match matches.len() {
        1 => Ok(matches
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("unexpected empty matches"))?),
        0 => anyhow::bail!("task item not found: {id_or_prefix}"),
        _ => anyhow::bail!("multiple task items match prefix '{id_or_prefix}': {matches:?}"),
    }
}
