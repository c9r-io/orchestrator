use crate::config_load::now_ts;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

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
        "UPDATE agent_sessions SET state = ?2, updated_at = ?3, ended_at = COALESCE(?4, ended_at), exit_code = COALESCE(?5, exit_code) WHERE id = ?1",
        params![session_id, state, now, ended_at, exit_code],
    )?;
    Ok(())
}

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
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
        ended_at: r.get(19)?,
        exit_code: r.get(20)?,
    })
}

const SESSION_COLUMNS: &str = "id, task_id, task_item_id, step_id, phase, agent_id, state, pid, pty_backend, cwd, command, input_fifo_path, stdout_path, stderr_path, transcript_path, output_json_path, writer_client_id, created_at, updated_at, ended_at, exit_code";

pub fn load_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    conn.query_row(
        &format!("SELECT {} FROM agent_sessions WHERE id = ?1", SESSION_COLUMNS),
        params![session_id],
        row_to_session,
    )
    .optional()
    .context("load session")
}

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

pub fn list_task_sessions(conn: &Connection, task_id: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        &format!(
            "SELECT {}
             FROM agent_sessions
             WHERE task_id = ?1
             ORDER BY created_at DESC",
            SESSION_COLUMNS
        ),
    )?;
    let rows = stmt
        .query_map(params![task_id], row_to_session)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

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

pub fn attach_reader(conn: &Connection, session_id: &str, client_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO session_attachments (session_id, client_id, mode, attached_at, detached_at, reason) VALUES (?1, ?2, 'reader', ?3, NULL, NULL)",
        params![session_id, client_id, now_ts()],
    )?;
    Ok(())
}

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
        update_session_state(&conn, "sess-1", "detached", Some(7), false)
            .expect("detach session");

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

        assert!(load_session(&conn, "missing")
            .expect("load missing session")
            .is_none());
    }

    #[test]
    fn active_session_lookup_and_listing_filter_by_task() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(
            &conn,
            &make_session("sess-old", "task-1", "qa", "exited"),
        )
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
        insert_session(
            &conn,
            &make_session("sess-other", "task-2", "qa", "active"),
        )
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
}
