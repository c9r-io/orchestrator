use crate::now_ts;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use std::collections::HashSet;

use super::command_run::NewCommandRun;
use super::references;
use rusqlite::Connection;

/// Splits a `table.column` attribution back into its two identifiers.
///
/// `references_holding` formats them and this reads them; both live here rather
/// than being reconstructed by callers.
fn split_reference(held: &str) -> (&str, &str) {
    match held.split_once('.') {
        Some((table, column)) => (table, column),
        None => (held, ""),
    }
}

pub fn update_task_item_status(conn: &Connection, task_item_id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![task_item_id, status, now_ts()],
    )?;
    Ok(())
}

pub fn update_task_item_pipeline_vars(
    conn: &Connection,
    task_item_id: &str,
    pipeline_vars_json: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE task_items SET dynamic_vars_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![task_item_id, pipeline_vars_json, now_ts()],
    )?;
    Ok(())
}

pub fn mark_task_item_running(conn: &Connection, task_item_id: &str) -> Result<()> {
    let now = now_ts();
    conn.execute(
        "UPDATE task_items SET status = 'running', started_at = COALESCE(started_at, ?2), completed_at = NULL, updated_at = ?3 WHERE id = ?1",
        params![task_item_id, now.clone(), now],
    )?;
    Ok(())
}

pub fn set_task_item_terminal_status(
    conn: &Connection,
    task_item_id: &str,
    status: &str,
) -> Result<()> {
    let now = now_ts();
    conn.execute(
        "UPDATE task_items SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = ?4, updated_at = ?5 WHERE id = ?1",
        params![task_item_id, status, now.clone(), now.clone(), now],
    )?;
    Ok(())
}

/// Deletes a task and everything hanging off it, returning the log file paths
/// the caller still has to unlink.
///
/// The row cascade and the filesystem cleanup are split deliberately: this
/// layer owns the database and knows nothing about where logs live, and the
/// caller owns the files and must not be asked to reproduce the cascade.
///
/// Every reference to the task is disposed of here, by the ruling
/// [`references::disposition_for`] records, so that all three delete paths —
/// an operator's `task delete`, the age-based retention sweep and the trigger
/// history limit — get the same answer by construction rather than by three
/// implementations agreeing. A reference with no ruling refuses the delete
/// before anything is mutated and comes back as [`TaskDeleteBlocked`]: a task
/// whose rows are half gone is worse than either outcome, and the check is
/// ahead of the writes rather than relying on the transaction to undo them.
pub(crate) fn delete_task_and_collect_log_paths(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<String>> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if exists.is_none() {
        anyhow::bail!("task not found: {task_id}");
    }

    // Derived from the schema on every call, so a table added later is disposed
    // of — or refuses — without this function being edited.
    let references = references::blocking_references(conn)?;
    let holding = references::references_holding(conn, &references, task_id)?;
    let blocked_by: Vec<String> = holding
        .iter()
        .filter(|held| {
            let (table, column) = split_reference(held);
            references::disposition_for(table, column) == references::Disposition::BlockAndReport
        })
        .cloned()
        .collect();
    if !blocked_by.is_empty() {
        return Err(references::TaskDeleteBlocked {
            task_id: task_id.to_string(),
            blocked_by,
        }
        .into());
    }

    let mut log_paths = HashSet::new();
    let mut runs_stmt = conn.prepare(
        "SELECT cr.stdout_path, cr.stderr_path
         FROM command_runs cr
         JOIN task_items ti ON ti.id = cr.task_item_id
         WHERE ti.task_id = ?1",
    )?;
    for row in runs_stmt.query_map(params![task_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (stdout_path, stderr_path) = row?;
        if !stdout_path.trim().is_empty() {
            log_paths.insert(stdout_path);
        }
        if !stderr_path.trim().is_empty() {
            log_paths.insert(stderr_path);
        }
    }

    let tx = conn.unchecked_transaction()?;

    // Dispose of every reference before the task row goes. Identifiers come
    // from `sqlite_master` by way of `blocking_references`, never from a
    // caller, and cannot be bound as parameters.
    for (table, column) in &references {
        match references::disposition_for(table, column) {
            references::Disposition::DeleteWithTask => {
                tx.execute(
                    &format!(r#"DELETE FROM "{table}" WHERE "{column}" = ?1"#),
                    params![task_id],
                )?;
            }
            references::Disposition::NullTheReference => {
                tx.execute(
                    &format!(r#"UPDATE "{table}" SET "{column}" = NULL WHERE "{column}" = ?1"#),
                    params![task_id],
                )?;
            }
            // Unreachable for a reference that holds a row — the precheck above
            // returned already. A ruling-less reference holding nothing needs
            // no statement.
            references::Disposition::BlockAndReport => {}
        }
    }

    tx.execute("DELETE FROM events WHERE task_id = ?1", params![task_id])?;
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
        params![task_id],
    )?;
    tx.execute(
        "DELETE FROM task_items WHERE task_id = ?1",
        params![task_id],
    )?;
    tx.execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
    tx.commit()?;
    Ok(log_paths.into_iter().collect())
}

pub fn insert_command_run(conn: &Connection, run: &NewCommandRun) -> Result<()> {
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted, session_id, machine_output_source, output_json_path, command_template, command_rule_index) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            run.id,
            run.task_item_id,
            run.phase,
            run.command,
            run.cwd,
            run.workspace_id,
            run.agent_id,
            run.exit_code,
            run.stdout_path,
            run.stderr_path,
            run.output_json,
            run.artifacts_json,
            run.confidence,
            run.quality_score,
            run.validation_status,
            run.started_at,
            run.ended_at,
            run.interrupted,
            run.session_id,
            run.machine_output_source,
            run.output_json_path,
            run.command_template,
            run.command_rule_index
        ],
    )?;
    Ok(())
}
