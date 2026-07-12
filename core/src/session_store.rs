use crate::async_database::AsyncDatabase;
use crate::config_load::now_ts;
use crate::persistence::repository::{SessionRepository, SqliteSessionRepository};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Persisted interactive session row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    /// Session identifier.
    pub id: String,
    /// Parent task identifier.
    pub task_id: String,
    /// Optional task-item identifier.
    pub task_item_id: Option<String>,
    /// Step identifier associated with the session.
    pub step_id: String,
    /// Phase name associated with the session.
    pub phase: String,
    /// Agent identifier that owns the session.
    pub agent_id: String,
    /// Session state string.
    pub state: String,
    /// PTY child PID.
    pub pid: i64,
    /// PTY backend identifier.
    pub pty_backend: String,
    /// Working directory for the child process.
    pub cwd: String,
    /// Rendered command line.
    pub command: String,
    /// FIFO path used for input streaming.
    pub input_fifo_path: String,
    /// Captured stdout path.
    pub stdout_path: String,
    /// Captured stderr path.
    pub stderr_path: String,
    /// Transcript file path.
    pub transcript_path: String,
    /// Optional structured output JSON spill path.
    pub output_json_path: Option<String>,
    /// Client currently holding the writer lease.
    pub writer_client_id: Option<String>,
    /// Trusted actor holding the writer lease.
    pub writer_actor: Option<String>,
    /// Writer lease expiration timestamp.
    pub writer_lease_expires_at: Option<String>,
    /// Last writer heartbeat timestamp.
    pub writer_last_heartbeat_at: Option<String>,
    /// Monotonic token fencing stale writers.
    pub writer_fencing_token: i64,
    /// Optimistic concurrency version.
    pub state_version: i64,
    /// OS process creation fingerprint used to reject PID reuse.
    pub process_fingerprint: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Optional end timestamp.
    pub ended_at: Option<String>,
    /// Optional process exit code.
    pub exit_code: Option<i64>,
}

/// Borrowed insert payload for a new interactive session.
pub struct NewSession<'a> {
    /// Session identifier.
    pub id: &'a str,
    /// Parent task identifier.
    pub task_id: &'a str,
    /// Optional task-item identifier.
    pub task_item_id: Option<&'a str>,
    /// Step identifier associated with the session.
    pub step_id: &'a str,
    /// Phase name associated with the session.
    pub phase: &'a str,
    /// Agent identifier that owns the session.
    pub agent_id: &'a str,
    /// Initial session state.
    pub state: &'a str,
    /// PTY child PID.
    pub pid: i64,
    /// PTY backend identifier.
    pub pty_backend: &'a str,
    /// Working directory for the child process.
    pub cwd: &'a str,
    /// Rendered command line.
    pub command: &'a str,
    /// FIFO path used for input streaming.
    pub input_fifo_path: &'a str,
    /// Captured stdout path.
    pub stdout_path: &'a str,
    /// Captured stderr path.
    pub stderr_path: &'a str,
    /// Transcript file path.
    pub transcript_path: &'a str,
    /// Optional structured output JSON spill path.
    pub output_json_path: Option<&'a str>,
}

/// Inserts a new interactive session row.
pub fn insert_session(conn: &Connection, s: &NewSession<'_>) -> Result<()> {
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

/// Updates session state, exit code, and optional end time.
pub fn update_session_state(
    conn: &Connection,
    session_id: &str,
    state: &str,
    exit_code: Option<i64>,
    ended: bool,
) -> Result<()> {
    let now = now_ts();
    let ended_at = if ended { Some(now.clone()) } else { None };
    conn.execute(
        "UPDATE agent_sessions SET state = ?2, state_version=state_version+1, updated_at = ?3, ended_at = COALESCE(?4, ended_at), exit_code = COALESCE(?5, exit_code) WHERE id = ?1",
        params![session_id, state, now, ended_at, exit_code],
    )?;
    Ok(())
}

/// Updates the PID associated with an existing session.
pub fn update_session_pid(conn: &Connection, session_id: &str, pid: i64) -> Result<()> {
    conn.execute(
        "UPDATE agent_sessions SET pid = ?2, updated_at = ?3 WHERE id = ?1",
        params![session_id, pid, now_ts()],
    )?;
    Ok(())
}

fn row_to_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
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
        writer_actor: r.get(17)?,
        writer_lease_expires_at: r.get(18)?,
        writer_last_heartbeat_at: r.get(19)?,
        writer_fencing_token: r.get(20)?,
        state_version: r.get(21)?,
        process_fingerprint: r.get(22)?,
        created_at: r.get(23)?,
        updated_at: r.get(24)?,
        ended_at: r.get(25)?,
        exit_code: r.get(26)?,
    })
}

const SESSION_COLUMNS: &str = "id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, writer_actor, writer_lease_expires_at, writer_last_heartbeat_at, writer_fencing_token, state_version, process_fingerprint, created_at, updated_at, ended_at, exit_code";

/// Result of a writer lease acquisition or refresh.
#[derive(Debug, Clone)]
pub struct WriterLease {
    /// Monotonically increasing fencing token.
    pub fencing_token: i64,
    /// RFC3339 lease expiry.
    pub expires_at: String,
}

/// Lists sessions with optional task, agent and state filters.
pub fn list_sessions(
    conn: &Connection,
    task_id: Option<&str>,
    agent_id: Option<&str>,
    state: Option<&str>,
) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLUMNS} FROM agent_sessions
         WHERE (?1 IS NULL OR task_id=?1) AND (?2 IS NULL OR agent_id=?2)
           AND (?3 IS NULL OR state=?3) ORDER BY created_at DESC LIMIT 500"
    ))?;
    Ok(stmt
        .query_map(params![task_id, agent_id, state], row_to_session)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Resolves a diagnostic PID to all matching persisted sessions.
pub fn list_sessions_by_pid(conn: &Connection, pid: i64) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE pid=?1 ORDER BY created_at DESC"
    ))?;
    Ok(stmt
        .query_map([pid], row_to_session)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

/// Captures a portable process-creation fingerprint for PID reuse protection.
///
/// The value is diagnostic metadata only and is never exposed as authority to clients.
pub fn capture_process_fingerprint(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let close = stat.rfind(')')?;
        let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
        // Field 22 in proc_pid_stat; the post-comm slice begins at field 3.
        let start_ticks = fields.get(19)?;
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        return Some(format!("{pid}:{}:{}", boot_id.trim(), start_ticks));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let started = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!started.is_empty()).then(|| format!("{pid}:{started}"))
    }
}

/// Stores a PID together with its creation fingerprint.
pub fn update_session_process(
    conn: &Connection,
    session_id: &str,
    pid: i64,
    fingerprint: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE agent_sessions SET pid=?2, process_fingerprint=?3, state_version=state_version+1, updated_at=?4 WHERE id=?1",
        params![session_id, pid, fingerprint, now_ts()],
    )?;
    Ok(())
}

/// Atomically acquires an expired/free writer lease and returns its fencing token.
pub fn acquire_writer_lease(
    conn: &Connection,
    session_id: &str,
    actor: &str,
    client_id: &str,
    ttl_secs: u64,
) -> Result<Option<WriterLease>> {
    let now = chrono::Utc::now();
    let now_s = now.to_rfc3339();
    let expires = (now + chrono::Duration::seconds(ttl_secs.max(1) as i64)).to_rfc3339();
    let changed = conn.execute(
        "UPDATE agent_sessions SET writer_client_id=?2, writer_actor=?3,
         writer_lease_expires_at=?4, writer_last_heartbeat_at=?5,
         writer_fencing_token=writer_fencing_token+1, state='active',
         state_version=state_version+1, updated_at=?5
         WHERE id=?1 AND state IN ('active','detached')
           AND (writer_client_id IS NULL OR writer_lease_expires_at IS NULL OR writer_lease_expires_at<=?5 OR writer_client_id=?2)",
        params![session_id, client_id, actor, expires, now_s],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    let token: i64 = conn.query_row(
        "SELECT writer_fencing_token FROM agent_sessions WHERE id=?1",
        [session_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO session_attachments(session_id,client_id,mode,attached_at) VALUES(?1,?2,'writer',?3)",
        params![session_id, client_id, now_s],
    )?;
    Ok(Some(WriterLease {
        fencing_token: token,
        expires_at: expires,
    }))
}

/// Extends the current writer lease when client and fencing token match.
pub fn heartbeat_writer(
    conn: &Connection,
    session_id: &str,
    client_id: &str,
    fencing_token: i64,
    ttl_secs: u64,
) -> Result<Option<String>> {
    let now = chrono::Utc::now();
    let now_s = now.to_rfc3339();
    let expires = (now + chrono::Duration::seconds(ttl_secs.max(1) as i64)).to_rfc3339();
    let changed = conn.execute(
        "UPDATE agent_sessions SET writer_lease_expires_at=?4, writer_last_heartbeat_at=?5, updated_at=?5
         WHERE id=?1 AND writer_client_id=?2 AND writer_fencing_token=?3
           AND writer_lease_expires_at>?5 AND state IN ('active','detached')",
        params![session_id, client_id, fencing_token, expires, now_s],
    )?;
    Ok((changed == 1).then_some(expires))
}

/// Returns whether a writer token is current and unexpired.
pub fn validate_writer(
    conn: &Connection,
    session_id: &str,
    client_id: &str,
    fencing_token: i64,
) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM agent_sessions WHERE id=?1 AND writer_client_id=?2
         AND writer_fencing_token=?3 AND writer_lease_expires_at>?4 AND state='active'",
        params![session_id, client_id, fencing_token, now_ts()],
        |row| row.get(0),
    )?;
    Ok(count == 1)
}

/// Releases an exact writer lease; stale tokens cannot release a newer owner.
pub fn release_writer(
    conn: &Connection,
    session_id: &str,
    client_id: &str,
    fencing_token: i64,
    reason: &str,
) -> Result<bool> {
    let now = now_ts();
    let changed = conn.execute(
        "UPDATE agent_sessions SET writer_client_id=NULL, writer_actor=NULL,
         writer_lease_expires_at=NULL, writer_last_heartbeat_at=NULL, state='detached',
         state_version=state_version+1, updated_at=?4
         WHERE id=?1 AND writer_client_id=?2 AND writer_fencing_token=?3",
        params![session_id, client_id, fencing_token, now],
    )?;
    if changed == 1 {
        conn.execute(
            "UPDATE session_attachments SET detached_at=?3,reason=?4 WHERE session_id=?1 AND client_id=?2 AND mode='writer' AND detached_at IS NULL",
            params![session_id, client_id, now, reason],
        )?;
    }
    Ok(changed == 1)
}

/// Expires stale writer leases and returns the affected session IDs.
pub fn expire_writer_leases(conn: &Connection) -> Result<Vec<String>> {
    let now = now_ts();
    let mut stmt = conn.prepare(
        "SELECT id FROM agent_sessions WHERE writer_client_id IS NOT NULL AND writer_lease_expires_at<=?1",
    )?;
    let ids = stmt
        .query_map([&now], |row| row.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    drop(stmt);
    conn.execute(
        "UPDATE agent_sessions SET writer_client_id=NULL,writer_actor=NULL,
         writer_lease_expires_at=NULL,writer_last_heartbeat_at=NULL,state='detached',
         state_version=state_version+1,updated_at=?1
         WHERE writer_client_id IS NOT NULL AND writer_lease_expires_at<=?1",
        [&now],
    )?;
    Ok(ids)
}

/// Reconciles non-terminal persisted sessions with OS process identity and transport state.
pub fn reconcile_sessions(conn: &Connection) -> Result<Vec<(String, String)>> {
    let rows = list_sessions(conn, None, None, None)?;
    let mut changes = Vec::new();
    for row in rows.into_iter().filter(|row| {
        matches!(
            row.state.as_str(),
            "opening" | "active" | "detached" | "draining"
        )
    }) {
        let identity_live = row.pid > 0
            && row.process_fingerprint.is_some()
            && capture_process_fingerprint(row.pid as u32) == row.process_fingerprint;
        let transport_exists = std::path::Path::new(&row.input_fifo_path).exists();
        let evidence_exists = std::path::Path::new(&row.transcript_path).exists()
            || std::path::Path::new(&row.stdout_path).exists();
        let target = if identity_live && transport_exists {
            if row.state == "draining" {
                "draining"
            } else if row.writer_client_id.is_some() {
                "active"
            } else {
                "detached"
            }
        } else if !identity_live && evidence_exists {
            "closed"
        } else {
            "failed"
        };
        if target != row.state {
            update_session_state(
                conn,
                &row.id,
                target,
                row.exit_code,
                matches!(target, "closed" | "failed"),
            )?;
            changes.push((row.id, target.to_owned()));
        }
    }
    let expired = expire_writer_leases(conn)?;
    changes.extend(
        expired
            .into_iter()
            .map(|id| (id, "lease_expired".to_owned())),
    );
    Ok(changes)
}

/// Loads a session row by session identifier.
pub fn load_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    conn.query_row(
        &format!(
            "SELECT {} FROM agent_sessions WHERE id = ?1",
            SESSION_COLUMNS
        ),
        params![session_id],
        row_to_session,
    )
    .optional()
    .context("load session")
}

/// Loads the latest active or detached session for a task step.
pub fn load_active_session_for_task_step(
    conn: &Connection,
    task_id: &str,
    step_id: &str,
) -> Result<Option<SessionRow>> {
    conn.query_row(
        &format!(
            "SELECT {}
             FROM agent_sessions
             WHERE task_id = ?1 AND step_id = ?2 AND state IN ('active','detached')
             ORDER BY created_at DESC
             LIMIT 1",
            SESSION_COLUMNS
        ),
        params![task_id, step_id],
        row_to_session,
    )
    .optional()
    .context("load active session for task step")
}

/// Lists all sessions for a task ordered from newest to oldest.
pub fn list_task_sessions(conn: &Connection, task_id: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}
             FROM agent_sessions
             WHERE task_id = ?1
             ORDER BY created_at DESC",
        SESSION_COLUMNS
    ))?;
    let rows = stmt
        .query_map(params![task_id], row_to_session)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Attempts to acquire the writer lease for a session.
pub fn acquire_writer(conn: &Connection, session_id: &str, client_id: &str) -> Result<bool> {
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

/// Attaches a read-only client to a session.
pub fn attach_reader(conn: &Connection, session_id: &str, client_id: &str) -> Result<()> {
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM session_attachments WHERE session_id=?1 AND mode='reader' AND detached_at IS NULL",
        [session_id],
        |row| row.get(0),
    )?;
    if active >= 8 {
        anyhow::bail!("session reader limit reached");
    }
    conn.execute(
        "INSERT INTO session_attachments (session_id, client_id, mode, attached_at, detached_at, reason) VALUES (?1, ?2, 'reader', ?3, NULL, NULL)",
        params![session_id, client_id, now_ts()],
    )?;
    Ok(())
}

/// Deletes old terminal sessions and returns the number removed.
pub fn cleanup_stale_sessions(conn: &Connection, max_age_hours: u64) -> Result<usize> {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(max_age_hours as i64);
    let cutoff = cutoff.to_rfc3339();
    conn.execute(
        "DELETE FROM session_control_actions WHERE session_id IN (SELECT id FROM agent_sessions WHERE state IN ('exited','closed','failed') AND updated_at < ?1)",
        [&cutoff],
    )?;
    conn.execute(
        "DELETE FROM session_attachments WHERE session_id IN (SELECT id FROM agent_sessions WHERE state IN ('exited','closed','failed') AND updated_at < ?1)",
        [&cutoff],
    )?;
    let deleted = conn.execute(
        "DELETE FROM agent_sessions WHERE state IN ('exited', 'closed', 'failed') AND updated_at < ?1",
        [&cutoff],
    )?;
    Ok(deleted)
}

/// Releases a reader or writer attachment for a client.
pub fn release_attachment(
    conn: &Connection,
    session_id: &str,
    client_id: &str,
    reason: &str,
) -> Result<()> {
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

/// Owned version of `NewSession` for async closures (`'static + Send`).
pub struct OwnedNewSession {
    /// Session identifier.
    pub id: String,
    /// Parent task identifier.
    pub task_id: String,
    /// Optional task-item identifier.
    pub task_item_id: Option<String>,
    /// Step identifier associated with the session.
    pub step_id: String,
    /// Phase name associated with the session.
    pub phase: String,
    /// Agent identifier that owns the session.
    pub agent_id: String,
    /// Initial session state.
    pub state: String,
    /// PTY child PID.
    pub pid: i64,
    /// PTY backend identifier.
    pub pty_backend: String,
    /// Working directory for the child process.
    pub cwd: String,
    /// Rendered command line.
    pub command: String,
    /// FIFO path used for input streaming.
    pub input_fifo_path: String,
    /// Captured stdout path.
    pub stdout_path: String,
    /// Captured stderr path.
    pub stderr_path: String,
    /// Transcript file path.
    pub transcript_path: String,
    /// Optional structured output JSON spill path.
    pub output_json_path: Option<String>,
}

impl<'a> From<&NewSession<'a>> for OwnedNewSession {
    fn from(s: &NewSession<'a>) -> Self {
        Self {
            id: s.id.to_owned(),
            task_id: s.task_id.to_owned(),
            task_item_id: s.task_item_id.map(|v| v.to_owned()),
            step_id: s.step_id.to_owned(),
            phase: s.phase.to_owned(),
            agent_id: s.agent_id.to_owned(),
            state: s.state.to_owned(),
            pid: s.pid,
            pty_backend: s.pty_backend.to_owned(),
            cwd: s.cwd.to_owned(),
            command: s.command.to_owned(),
            input_fifo_path: s.input_fifo_path.to_owned(),
            stdout_path: s.stdout_path.to_owned(),
            stderr_path: s.stderr_path.to_owned(),
            transcript_path: s.transcript_path.to_owned(),
            output_json_path: s.output_json_path.map(|v| v.to_owned()),
        }
    }
}

/// Async facade around a [`SessionRepository`] implementation.
pub struct AsyncSessionStore {
    repository: Arc<dyn SessionRepository>,
}

impl AsyncSessionStore {
    /// Creates a SQLite-backed async session store.
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self::with_repository(Arc::new(SqliteSessionRepository::new(async_db)))
    }

    /// Creates an async session store from a repository implementation.
    pub fn with_repository(repository: Arc<dyn SessionRepository>) -> Self {
        Self { repository }
    }

    /// Inserts a new session row.
    pub async fn insert_session(&self, s: OwnedNewSession) -> Result<()> {
        self.repository.insert_session(s).await
    }

    /// Updates session state, exit code, and optional end time.
    pub async fn update_session_state(
        &self,
        session_id: &str,
        state: &str,
        exit_code: Option<i64>,
        ended: bool,
    ) -> Result<()> {
        self.repository
            .update_session_state(session_id, state, exit_code, ended)
            .await
    }

    /// Updates the PID associated with a session.
    pub async fn update_session_pid(&self, session_id: &str, pid: i64) -> Result<()> {
        self.repository.update_session_pid(session_id, pid).await
    }

    /// Loads a session row by identifier.
    pub async fn load_session(&self, session_id: &str) -> Result<Option<SessionRow>> {
        self.repository.load_session(session_id).await
    }

    /// Loads the latest active or detached session for a task step.
    pub async fn load_active_session_for_task_step(
        &self,
        task_id: &str,
        step_id: &str,
    ) -> Result<Option<SessionRow>> {
        self.repository
            .load_active_session_for_task_step(task_id, step_id)
            .await
    }

    /// Lists all sessions for a task.
    pub async fn list_task_sessions(&self, task_id: &str) -> Result<Vec<SessionRow>> {
        self.repository.list_task_sessions(task_id).await
    }

    /// Attempts to acquire the writer lease for a session.
    pub async fn acquire_writer(&self, session_id: &str, client_id: &str) -> Result<bool> {
        self.repository.acquire_writer(session_id, client_id).await
    }

    /// Attaches a read-only client to a session.
    pub async fn attach_reader(&self, session_id: &str, client_id: &str) -> Result<()> {
        self.repository.attach_reader(session_id, client_id).await
    }

    /// Deletes stale terminal sessions and returns the number removed.
    pub async fn cleanup_stale_sessions(&self, max_age_hours: u64) -> Result<usize> {
        self.repository.cleanup_stale_sessions(max_age_hours).await
    }

    /// Releases a reader or writer attachment for a client.
    pub async fn release_attachment(
        &self,
        session_id: &str,
        client_id: &str,
        reason: &str,
    ) -> Result<()> {
        self.repository
            .release_attachment(session_id, client_id, reason)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_schema, open_conn};
    use tempfile::TempDir;

    fn make_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("sessions.db");
        init_schema(&db_path).expect("init schema");
        (dir, db_path)
    }

    fn make_session<'a>(
        id: &'a str,
        task_id: &'a str,
        step_id: &'a str,
        state: &'a str,
    ) -> NewSession<'a> {
        NewSession {
            id,
            task_id,
            task_item_id: Some("item-1"),
            step_id,
            phase: "qa",
            agent_id: "agent-a",
            state,
            pid: 100,
            pty_backend: "pty",
            cwd: "/tmp",
            command: "echo hi",
            input_fifo_path: "/tmp/in.fifo",
            stdout_path: "/tmp/stdout.log",
            stderr_path: "/tmp/stderr.log",
            transcript_path: "/tmp/transcript.log",
            output_json_path: Some("/tmp/output.json"),
        }
    }

    #[test]
    fn insert_load_and_update_session_lifecycle() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        let session = make_session("sess-1", "task-1", "qa", "active");
        insert_session(&conn, &session).expect("insert session");

        let inserted = load_session(&conn, "sess-1")
            .expect("load session")
            .expect("session should exist");
        assert_eq!(inserted.task_item_id.as_deref(), Some("item-1"));
        assert_eq!(
            inserted.output_json_path.as_deref(),
            Some("/tmp/output.json")
        );
        assert_eq!(inserted.state, "active");
        assert_eq!(inserted.pid, 100);
        assert_eq!(inserted.ended_at, None);
        assert_eq!(inserted.exit_code, None);

        update_session_pid(&conn, "sess-1", 4242).expect("update pid");
        update_session_state(&conn, "sess-1", "detached", Some(7), false).expect("detach session");

        let detached = load_session(&conn, "sess-1")
            .expect("reload session")
            .expect("session should still exist");
        assert_eq!(detached.pid, 4242);
        assert_eq!(detached.state, "detached");
        assert_eq!(detached.exit_code, Some(7));
        assert_eq!(detached.ended_at, None);

        update_session_state(&conn, "sess-1", "exited", None, true).expect("exit session");
        let exited = load_session(&conn, "sess-1")
            .expect("reload exited session")
            .expect("session should still exist");
        assert_eq!(exited.state, "exited");
        assert_eq!(exited.exit_code, Some(7));
        assert!(exited.ended_at.is_some());

        assert!(
            load_session(&conn, "missing")
                .expect("load missing session")
                .is_none()
        );
    }

    #[test]
    fn active_session_lookup_and_listing_filter_by_task() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-old", "task-1", "qa", "exited"))
            .expect("insert exited session");
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_session(
            &conn,
            &make_session("sess-active", "task-1", "qa", "active"),
        )
        .expect("insert active session");
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert_session(
            &conn,
            &make_session("sess-detached", "task-1", "qa", "detached"),
        )
        .expect("insert detached session");
        insert_session(&conn, &make_session("sess-other", "task-2", "qa", "active"))
            .expect("insert other task session");

        let active = load_active_session_for_task_step(&conn, "task-1", "qa")
            .expect("query active session")
            .expect("task should have an active session");
        assert_eq!(active.id, "sess-detached");
        assert_eq!(active.state, "detached");

        let task_1_sessions = list_task_sessions(&conn, "task-1").expect("list sessions");
        let task_1_ids: Vec<&str> = task_1_sessions.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(task_1_ids.len(), 3);
        assert!(task_1_ids.contains(&"sess-old"));
        assert!(task_1_ids.contains(&"sess-active"));
        assert!(task_1_ids.contains(&"sess-detached"));

        assert!(
            load_active_session_for_task_step(&conn, "task-1", "missing-step")
                .expect("query missing step")
                .is_none()
        );
    }

    #[test]
    fn cleanup_stale_sessions_removes_old_exited_keeps_recent() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");

        // Insert an "exited" session and manually backdate updated_at
        insert_session(&conn, &make_session("old-exited", "task-1", "qa", "exited"))
            .expect("insert old exited");
        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(100)).to_rfc3339();
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["old-exited", old_ts],
        )
        .expect("backdate old session");

        // Insert an "active" session that is also old — should NOT be deleted
        insert_session(&conn, &make_session("old-active", "task-1", "qa", "active"))
            .expect("insert old active");
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["old-active", old_ts],
        )
        .expect("backdate active session");

        // Insert a recent "exited" session — should NOT be deleted
        insert_session(&conn, &make_session("new-exited", "task-1", "qa", "exited"))
            .expect("insert new exited");

        let deleted = cleanup_stale_sessions(&conn, 72).expect("cleanup");
        assert_eq!(deleted, 1);

        // Verify correct session was deleted
        assert!(load_session(&conn, "old-exited").expect("load").is_none());
        assert!(load_session(&conn, "old-active").expect("load").is_some());
        assert!(load_session(&conn, "new-exited").expect("load").is_some());
    }

    #[test]
    fn writer_and_reader_attachments_round_trip() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-1", "task-1", "qa", "active"))
            .expect("insert session");

        assert!(acquire_writer(&conn, "sess-1", "writer-1").expect("acquire initial writer"));
        assert!(acquire_writer(&conn, "sess-1", "writer-1").expect("re-acquire same writer"));
        assert!(!acquire_writer(&conn, "sess-1", "writer-2").expect("reject second writer"));

        attach_reader(&conn, "sess-1", "reader-1").expect("attach reader");
        release_attachment(&conn, "sess-1", "reader-1", "done").expect("detach reader");
        release_attachment(&conn, "sess-1", "writer-1", "handoff").expect("detach writer");

        let session = load_session(&conn, "sess-1")
            .expect("reload session")
            .expect("session should exist");
        assert_eq!(session.writer_client_id, None);

        let writer_attachments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments WHERE session_id = ?1 AND mode = 'writer'",
                params!["sess-1"],
                |row| row.get(0),
            )
            .expect("count writer attachments");
        let detached_attachments: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments WHERE session_id = ?1 AND detached_at IS NOT NULL",
                params!["sess-1"],
                |row| row.get(0),
            )
            .expect("count detached attachments");

        assert_eq!(writer_attachments, 2);
        assert_eq!(detached_attachments, 3);
    }

    #[test]
    fn writer_lease_fencing_rejects_stale_tokens() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-fence", "task-1", "qa", "active"))
            .expect("insert session");

        let first = acquire_writer_lease(&conn, "sess-fence", "actor-a", "client-a", 30)
            .expect("acquire first")
            .expect("first lease");
        assert!(validate_writer(&conn, "sess-fence", "client-a", first.fencing_token).unwrap());
        assert!(
            release_writer(
                &conn,
                "sess-fence",
                "client-a",
                first.fencing_token,
                "handoff"
            )
            .unwrap()
        );

        let second = acquire_writer_lease(&conn, "sess-fence", "actor-b", "client-b", 30)
            .expect("acquire second")
            .expect("second lease");
        assert!(second.fencing_token > first.fencing_token);
        assert!(!validate_writer(&conn, "sess-fence", "client-a", first.fencing_token).unwrap());
        assert!(
            !release_writer(
                &conn,
                "sess-fence",
                "client-a",
                first.fencing_token,
                "stale"
            )
            .unwrap()
        );
        assert!(validate_writer(&conn, "sess-fence", "client-b", second.fencing_token).unwrap());
    }

    #[test]
    fn process_fingerprint_changes_authority_from_pid_to_identity() {
        let pid = std::process::id();
        let fingerprint = capture_process_fingerprint(pid).expect("current process fingerprint");
        assert!(fingerprint.starts_with(&format!("{pid}:")));
        assert_eq!(
            capture_process_fingerprint(pid).as_deref(),
            Some(fingerprint.as_str())
        );
        assert!(capture_process_fingerprint(u32::MAX).is_none());
    }

    #[test]
    fn reader_limit_and_expired_writer_are_bounded() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(
            &conn,
            &make_session("sess-bounds", "task-1", "qa", "active"),
        )
        .expect("insert session");
        for index in 0..8 {
            attach_reader(&conn, "sess-bounds", &format!("reader-{index}"))
                .expect("reader within bound");
        }
        assert!(attach_reader(&conn, "sess-bounds", "reader-9").is_err());

        let lease = acquire_writer_lease(&conn, "sess-bounds", "actor", "writer", 30)
            .expect("acquire writer")
            .expect("writer lease");
        conn.execute(
            "UPDATE agent_sessions SET writer_lease_expires_at='1970-01-01T00:00:00Z' WHERE id='sess-bounds'",
            [],
        )
        .expect("expire lease");
        assert_eq!(expire_writer_leases(&conn).unwrap(), vec!["sess-bounds"]);
        assert!(!validate_writer(&conn, "sess-bounds", "writer", lease.fencing_token).unwrap());
    }

    #[tokio::test]
    async fn async_session_store_exercises_all_wrapper_methods() {
        let (_dir, db_path) = make_db();
        let async_db = Arc::new(AsyncDatabase::open(&db_path).await.expect("open async db"));
        let store = AsyncSessionStore::new(async_db);

        let session = make_session("sess-async", "task-1", "qa", "active");
        store
            .insert_session(OwnedNewSession::from(&session))
            .await
            .expect("insert session");

        let loaded = store
            .load_session("sess-async")
            .await
            .expect("load session")
            .expect("session exists");
        assert_eq!(loaded.id, "sess-async");
        assert_eq!(loaded.state, "active");

        let active = store
            .load_active_session_for_task_step("task-1", "qa")
            .await
            .expect("load active session")
            .expect("active session exists");
        assert_eq!(active.id, "sess-async");

        let listed = store
            .list_task_sessions("task-1")
            .await
            .expect("list sessions");
        assert_eq!(listed.len(), 1);

        assert!(
            store
                .acquire_writer("sess-async", "writer-1")
                .await
                .expect("acquire writer")
        );
        assert!(
            !store
                .acquire_writer("sess-async", "writer-2")
                .await
                .expect("reject second writer")
        );

        store
            .attach_reader("sess-async", "reader-1")
            .await
            .expect("attach reader");
        store
            .update_session_pid("sess-async", 5150)
            .await
            .expect("update pid");
        store
            .update_session_state("sess-async", "failed", Some(9), true)
            .await
            .expect("update session state");
        store
            .release_attachment("sess-async", "reader-1", "done")
            .await
            .expect("release reader");
        store
            .release_attachment("sess-async", "writer-1", "done")
            .await
            .expect("release writer");

        let exited = store
            .load_session("sess-async")
            .await
            .expect("reload exited session")
            .expect("session still exists");
        assert_eq!(exited.pid, 5150);
        assert_eq!(exited.state, "failed");
        assert_eq!(exited.exit_code, Some(9));
        assert!(exited.ended_at.is_some());
        assert!(exited.writer_client_id.is_none());

        let conn = open_conn(&db_path).expect("open sync conn");
        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(100)).to_rfc3339();
        conn.execute(
            "UPDATE agent_sessions SET updated_at = ?2 WHERE id = ?1",
            params!["sess-async", old_ts],
        )
        .expect("backdate session");

        let deleted = store
            .cleanup_stale_sessions(72)
            .await
            .expect("cleanup stale sessions");
        assert_eq!(deleted, 1);
        assert!(
            store
                .load_session("sess-async")
                .await
                .expect("load deleted session")
                .is_none()
        );
        assert!(
            store
                .load_session("missing")
                .await
                .expect("load missing session")
                .is_none()
        );
    }
}
