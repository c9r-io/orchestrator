use crate::config_load::now_ts;
use crate::db::open_conn;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub step_id: String,
    pub phase: String,
    pub agent_id: String,
    pub state: String,
    pub pid: i64,
    pub pty_backend: String,
    pub cwd: String,
    pub command: String,
    pub input_fifo_path: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub transcript_path: String,
    pub output_json_path: Option<String>,
    pub writer_client_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
}

pub struct NewSession<'a> {
    pub id: &'a str,
    pub task_id: &'a str,
    pub task_item_id: Option<&'a str>,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub agent_id: &'a str,
    pub state: &'a str,
    pub pid: i64,
    pub pty_backend: &'a str,
    pub cwd: &'a str,
    pub command: &'a str,
    pub input_fifo_path: &'a str,
    pub stdout_path: &'a str,
    pub stderr_path: &'a str,
    pub transcript_path: &'a str,
    pub output_json_path: Option<&'a str>,
}

pub fn insert_session(db_path: &Path, s: &NewSession<'_>) -> Result<()> {
    let conn = open_conn(db_path)?;
    let now = now_ts();
    conn.execute(
        "INSERT INTO agent_sessions (id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, created_at, updated_at, ended_at, exit_code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, NULL, ?17, ?17, NULL, NULL)",
        params![
            s.id,
            s.task_id,
            s.task_item_id,
            s.step_id,
            s.phase,
            s.agent_id,
            s.state,
            s.pid,
            s.pty_backend,
            s.cwd,
            s.command,
            s.input_fifo_path,
            s.stdout_path,
            s.stderr_path,
            s.transcript_path,
            s.output_json_path,
            now
        ],
    )?;
    Ok(())
}

pub fn update_session_state(
    db_path: &Path,
    session_id: &str,
    state: &str,
    exit_code: Option<i64>,
    ended: bool,
) -> Result<()> {
    let conn = open_conn(db_path)?;
    let now = now_ts();
    let ended_at = if ended { Some(now.clone()) } else { None };
    conn.execute(
        "UPDATE agent_sessions SET state = ?2, updated_at = ?3, ended_at = COALESCE(?4, ended_at), exit_code = COALESCE(?5, exit_code) WHERE id = ?1",
        params![session_id, state, now, ended_at, exit_code],
    )?;
    Ok(())
}

pub fn update_session_pid(db_path: &Path, session_id: &str, pid: i64) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "UPDATE agent_sessions SET pid = ?2, updated_at = ?3 WHERE id = ?1",
        params![session_id, pid, now_ts()],
    )?;
    Ok(())
}

pub fn load_session(db_path: &Path, session_id: &str) -> Result<Option<SessionRow>> {
    let conn = open_conn(db_path)?;
    conn.query_row(
        "SELECT id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, created_at, updated_at, ended_at, exit_code FROM agent_sessions WHERE id = ?1",
        params![session_id],
        |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                task_id: r.get(1)?,
                task_item_id: r.get(2)?,
                step_id: r.get(3)?,
                phase: r.get(4)?,
                agent_id: r.get(5)?,
                state: r.get(6)?,
                pid: r.get(7)?,
                pty_backend: r.get(8)?,
                cwd: r.get(9)?,
                command: r.get(10)?,
                input_fifo_path: r.get(11)?,
                stdout_path: r.get(12)?,
                stderr_path: r.get(13)?,
                transcript_path: r.get(14)?,
                output_json_path: r.get(15)?,
                writer_client_id: r.get(16)?,
                created_at: r.get(17)?,
                updated_at: r.get(18)?,
                ended_at: r.get(19)?,
                exit_code: r.get(20)?,
            })
        },
    )
    .optional()
    .context("load session")
}

pub fn load_active_session_for_task_step(
    db_path: &Path,
    task_id: &str,
    step_id: &str,
) -> Result<Option<SessionRow>> {
    let conn = open_conn(db_path)?;
    conn.query_row(
        "SELECT id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, created_at, updated_at, ended_at, exit_code
         FROM agent_sessions
         WHERE task_id = ?1 AND step_id = ?2 AND state IN ('active','detached')
         ORDER BY created_at DESC
         LIMIT 1",
        params![task_id, step_id],
        |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                task_id: r.get(1)?,
                task_item_id: r.get(2)?,
                step_id: r.get(3)?,
                phase: r.get(4)?,
                agent_id: r.get(5)?,
                state: r.get(6)?,
                pid: r.get(7)?,
                pty_backend: r.get(8)?,
                cwd: r.get(9)?,
                command: r.get(10)?,
                input_fifo_path: r.get(11)?,
                stdout_path: r.get(12)?,
                stderr_path: r.get(13)?,
                transcript_path: r.get(14)?,
                output_json_path: r.get(15)?,
                writer_client_id: r.get(16)?,
                created_at: r.get(17)?,
                updated_at: r.get(18)?,
                ended_at: r.get(19)?,
                exit_code: r.get(20)?,
            })
        },
    )
    .optional()
    .context("load active session for task step")
}

pub fn list_task_sessions(db_path: &Path, task_id: &str) -> Result<Vec<SessionRow>> {
    let conn = open_conn(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, created_at, updated_at, ended_at, exit_code
         FROM agent_sessions
         WHERE task_id = ?1
         ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![task_id], |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                task_id: r.get(1)?,
                task_item_id: r.get(2)?,
                step_id: r.get(3)?,
                phase: r.get(4)?,
                agent_id: r.get(5)?,
                state: r.get(6)?,
                pid: r.get(7)?,
                pty_backend: r.get(8)?,
                cwd: r.get(9)?,
                command: r.get(10)?,
                input_fifo_path: r.get(11)?,
                stdout_path: r.get(12)?,
                stderr_path: r.get(13)?,
                transcript_path: r.get(14)?,
                output_json_path: r.get(15)?,
                writer_client_id: r.get(16)?,
                created_at: r.get(17)?,
                updated_at: r.get(18)?,
                ended_at: r.get(19)?,
                exit_code: r.get(20)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn acquire_writer(db_path: &Path, session_id: &str, client_id: &str) -> Result<bool> {
    let conn = open_conn(db_path)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT writer_client_id FROM agent_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if let Some(owner) = existing {
        if !owner.is_empty() && owner != client_id {
            return Ok(false);
        }
    }
    conn.execute(
        "UPDATE agent_sessions SET writer_client_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![session_id, client_id, now_ts()],
    )?;
    conn.execute(
        "INSERT INTO session_attachments (session_id, client_id, mode, attached_at, detached_at, reason) VALUES (?1, ?2, 'writer', ?3, NULL, NULL)",
        params![session_id, client_id, now_ts()],
    )?;
    Ok(true)
}

pub fn attach_reader(db_path: &Path, session_id: &str, client_id: &str) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "INSERT INTO session_attachments (session_id, client_id, mode, attached_at, detached_at, reason) VALUES (?1, ?2, 'reader', ?3, NULL, NULL)",
        params![session_id, client_id, now_ts()],
    )?;
    Ok(())
}

pub fn release_attachment(db_path: &Path, session_id: &str, client_id: &str, reason: &str) -> Result<()> {
    let conn = open_conn(db_path)?;
    conn.execute(
        "UPDATE session_attachments SET detached_at = ?3, reason = ?4 WHERE session_id = ?1 AND client_id = ?2 AND detached_at IS NULL",
        params![session_id, client_id, now_ts(), reason],
    )?;
    conn.execute(
        "UPDATE agent_sessions SET writer_client_id = NULL, updated_at = ?2 WHERE id = ?1 AND writer_client_id = ?3",
        params![session_id, now_ts(), client_id],
    )?;
    Ok(())
}
