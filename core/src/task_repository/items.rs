use crate::config_load::now_ts;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

use super::command_run::NewCommandRun;
use super::types::TaskRepositoryConn;

pub fn update_task_item_status(
    conn: &TaskRepositoryConn,
    task_item_id: &str,
    status: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![task_item_id, status, now_ts()],
    )?;
    Ok(())
}

pub fn mark_task_item_running(conn: &TaskRepositoryConn, task_item_id: &str) -> Result<()> {
    let now = now_ts();
    conn.execute(
        "UPDATE task_items SET status = 'running', started_at = COALESCE(started_at, ?2), completed_at = NULL, updated_at = ?3 WHERE id = ?1",
        params![task_item_id, now.clone(), now],
    )?;
    Ok(())
}

pub fn set_task_item_terminal_status(
    conn: &TaskRepositoryConn,
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

pub fn delete_task_and_collect_log_paths(
    conn: &TaskRepositoryConn,
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
        anyhow::bail!("task not found: {}", task_id);
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

pub fn insert_command_run(conn: &TaskRepositoryConn, run: &NewCommandRun) -> Result<()> {
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted, session_id, machine_output_source, output_json_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
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
            run.output_json_path
        ],
    )?;
    Ok(())
}
