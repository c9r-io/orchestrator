use crate::async_database::AsyncDatabase;
use crate::now_ts;
use crate::repository::{SessionRepository, SqliteSessionRepository};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result of comparing a persisted process fingerprint with the current OS process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessIdentityStatus {
    /// The PID is live and its creation fingerprint matches the persisted value.
    VerifiedLive,
    /// No process currently exists for the persisted PID.
    Dead,
    /// The numeric PID is live but belongs to a different process incarnation.
    Mismatch,
    /// The PID is live, but this platform cannot produce a trustworthy fingerprint.
    Unsupported,
}

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
pub(crate) fn insert_session(conn: &Connection, s: &NewSession<'_>) -> Result<()> {
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
pub(crate) fn update_session_state(
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
pub(crate) fn update_session_pid(conn: &Connection, session_id: &str, pid: i64) -> Result<()> {
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
pub(crate) fn list_sessions(
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
pub(crate) fn list_sessions_by_pid(conn: &Connection, pid: i64) -> Result<Vec<SessionRow>> {
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
        Some(format!("{pid}:{}:{}", boot_id.trim(), start_ticks))
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

/// Reads the parent PID of a running process, or `None` if it cannot be read.
///
/// Mirrors [`capture_process_fingerprint`]'s platform split for the same reason:
/// `/proc` on Linux, `ps` elsewhere.
pub fn process_parent_pid(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let close = stat.rfind(')')?;
        // Fields after comm begin at state; ppid is the one that follows it.
        let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
        fields.get(1)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
}

/// Determines whether a live session process still belongs to this daemon.
///
/// A session is spawned as a direct child of the daemon, so while the daemon
/// lives its sessions have it as their parent. When the daemon dies its sessions
/// are reparented to `init` and there is no longer anything that can drive
/// them: their stdout capture was wired to file descriptors of a process that no
/// longer exists, and no future daemon adopts them.
///
/// This is the discriminator the FR's primary scenario actually needs. "The
/// transport has disappeared" does not fire there: the input FIFO is a file
/// under `logs/sessions/<id>/`, and it outlives the daemon perfectly well, so an
/// orphan produced by `SIGKILL`ing the daemon looks exactly like a session
/// waiting politely for its next writer. Parentage is what separates them.
pub fn process_is_owned_by(pid: i64, owner_pid: u32) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    process_parent_pid(pid as u32).is_some_and(|parent| parent == owner_pid)
}

/// Determines whether a PID currently refers to a live process without granting authority.
pub fn process_exists(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: signal 0 performs an existence/permission check and does not signal the process.
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Compares the live process incarnation with a persisted fingerprint.
pub fn process_identity_status(pid: i64, expected: Option<&str>) -> ProcessIdentityStatus {
    if pid <= 0 || !process_exists(pid as u32) {
        return ProcessIdentityStatus::Dead;
    }
    let Some(expected) = expected else {
        return ProcessIdentityStatus::Unsupported;
    };
    match capture_process_fingerprint(pid as u32) {
        Some(actual) if actual == expected => ProcessIdentityStatus::VerifiedLive,
        Some(_) => ProcessIdentityStatus::Mismatch,
        None => ProcessIdentityStatus::Unsupported,
    }
}

/// Determines whether a PID leads its own process group.
///
/// `kill(-pid, …)` addresses the *group* whose id equals `pid`. Sessions are
/// spawned with `process_group(0)`, so for them pid and pgid coincide and the
/// negated form reaches the whole subtree. That equality is an assumption, not a
/// guarantee: a fingerprint proves the PID is the same process it always was, it
/// says nothing about group membership. If a recorded PID is not its own leader,
/// the negated signal lands on somebody else's group entirely — so this is
/// checked separately rather than inferred from identity.
pub fn is_process_group_leader(pid: i64) -> bool {
    if pid <= 0 || pid > i32::MAX as i64 {
        return false;
    }
    // SAFETY: getpgid is a POSIX query with no side effects; a negative return
    // reports an error and simply fails the comparison.
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    pgid == pid as libc::pid_t
}

/// Why a process-group reclamation was refused.
///
/// Every variant means no signal was sent. The cost asymmetry is deliberate: a
/// missed reclamation leaks one process, whereas signalling the wrong group can
/// kill unrelated work, so anything short of positive proof refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimRefusal {
    /// The PID no longer refers to a running process; nothing to reclaim.
    ProcessGone,
    /// The PID is live but is a different incarnation — the PID was reused.
    IdentityMismatch,
    /// The PID is live but this platform cannot produce a trustworthy fingerprint.
    IdentityUnsupported,
    /// The PID is live and verified but does not lead its own process group.
    NotGroupLeader,
}

impl ReclaimRefusal {
    /// Stable identifier for logs, events and QA assertions.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProcessGone => "process_gone",
            Self::IdentityMismatch => "identity_mismatch",
            Self::IdentityUnsupported => "identity_unsupported",
            Self::NotGroupLeader => "not_group_leader",
        }
    }
}

/// How forcefully to reclaim a process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclaimSignal {
    /// A single `SIGKILL`, for an orphan whose transport has already gone.
    Immediate,
    /// `SIGTERM`, a grace period, then `SIGKILL` for whatever survives.
    Graceful,
}

/// What a successful reclamation actually sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReclaimSignalsSent {
    /// A `SIGTERM` was delivered to the group.
    pub sigterm: bool,
    /// A `SIGKILL` was delivered to the group.
    pub sigkill: bool,
    /// The group was gone before any `SIGKILL` became necessary.
    pub exited_on_sigterm: bool,
}

const RECLAIM_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
const RECLAIM_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Signals the process group led by `pid`, refusing unless it is provably safe.
///
/// Three preconditions, all of which must hold before any signal is sent:
///
/// 1. `process_identity_status` says `VerifiedLive`. This is re-evaluated here
///    rather than inherited from an earlier reconciliation pass — between the
///    pass and the signal the process can exit and its PID be reused, and the
///    whole point of the fingerprint is defeated by checking it early and acting
///    late.
/// 2. The PID leads its own process group. See [`is_process_group_leader`].
/// 3. The caller opted in (the `session_reclaim_enabled` policy, enforced above
///    this layer, which has no access to configuration).
///
/// The signal goes to `-pid`, the whole group, not to `pid`. A session may have
/// spawned children; killing only the leader leaves them running and reparented,
/// which is how the orphans in FR-159 came to have dead leaders and live
/// descendants in the first place.
pub fn reclaim_process_group(
    pid: i64,
    expected_fingerprint: Option<&str>,
    signal: ReclaimSignal,
) -> std::result::Result<ReclaimSignalsSent, ReclaimRefusal> {
    match process_identity_status(pid, expected_fingerprint) {
        ProcessIdentityStatus::VerifiedLive => {}
        ProcessIdentityStatus::Dead => return Err(ReclaimRefusal::ProcessGone),
        ProcessIdentityStatus::Mismatch => return Err(ReclaimRefusal::IdentityMismatch),
        ProcessIdentityStatus::Unsupported => return Err(ReclaimRefusal::IdentityUnsupported),
    }
    if !is_process_group_leader(pid) {
        return Err(ReclaimRefusal::NotGroupLeader);
    }

    let group = -(pid as i32);
    match signal {
        ReclaimSignal::Immediate => {
            // SAFETY: kill(-pid, …) signals a process group. The preconditions
            // above establish that this group is the session's own.
            unsafe { libc::kill(group, libc::SIGKILL) };
            Ok(ReclaimSignalsSent {
                sigterm: false,
                sigkill: true,
                exited_on_sigterm: false,
            })
        }
        ReclaimSignal::Graceful => {
            // SAFETY: as above.
            unsafe { libc::kill(group, libc::SIGTERM) };
            let deadline = std::time::Instant::now() + RECLAIM_GRACE;
            while std::time::Instant::now() < deadline {
                if !process_exists(pid as u32) {
                    return Ok(ReclaimSignalsSent {
                        sigterm: true,
                        sigkill: false,
                        exited_on_sigterm: true,
                    });
                }
                std::thread::sleep(RECLAIM_POLL);
            }
            // SAFETY: as above.
            unsafe { libc::kill(group, libc::SIGKILL) };
            Ok(ReclaimSignalsSent {
                sigterm: true,
                sigkill: true,
                exited_on_sigterm: false,
            })
        }
    }
}

/// Stores a PID together with its creation fingerprint.
pub(crate) fn update_session_process(
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
pub(crate) fn acquire_writer_lease(
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
pub(crate) fn heartbeat_writer(
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
pub(crate) fn validate_writer(
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
pub(crate) fn release_writer(
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
pub(crate) fn expire_writer_leases(conn: &Connection) -> Result<Vec<String>> {
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
         writer_lease_expires_at=NULL,writer_last_heartbeat_at=NULL,
         state=CASE WHEN state IN ('active','detached') THEN 'detached' ELSE state END,
         state_version=state_version+1,updated_at=?1
         WHERE writer_client_id IS NOT NULL AND writer_lease_expires_at<=?1",
        [&now],
    )?;
    Ok(ids)
}

/// A session reconciliation found running while its transport had disappeared.
///
/// Carrying the evidence rather than just the id keeps the decision auditable:
/// whoever acts on this can say which PID it signalled and on what grounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimCandidate {
    /// Session identifier.
    pub session_id: String,
    /// Owning task, so the reclamation can be recorded on the task's timeline.
    pub task_id: String,
    /// Recorded PID of the session's process-group leader.
    pub pid: i64,
    /// Fingerprint the reclamation must re-verify before signalling.
    pub process_fingerprint: Option<String>,
    /// This session's own directory, when it can be derived unambiguously.
    pub session_dir: Option<std::path::PathBuf>,
}

/// Outcome of one reconciliation pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileOutcome {
    /// `(session_id, new_state)` for every session whose state moved.
    pub changes: Vec<(String, String)>,
    /// Sessions that are running with no transport, i.e. orphans to reclaim.
    pub reclaim_candidates: Vec<ReclaimCandidate>,
}

/// Derives the directory that belongs to this session and nothing else.
///
/// Returns `None` unless all of the following hold, because the consequence of
/// getting it wrong is deleting somebody else's data:
///
/// * every recorded path (`input_fifo_path`, `transcript_path`, `stdout_path`,
///   `stderr_path`) sits directly in one and the same parent directory;
/// * that directory is named for this session id;
/// * its own parent is named `sessions`.
///
/// The layout this recognises is the one `phase_runner::spawn` creates:
/// `<logs>/sessions/<session_id>/{input.fifo,transcript.log,…}`. A row whose
/// paths have been rewritten, point outside the session tree, or disagree with
/// each other yields `None` and the directory is left alone. Refusing to guess
/// is the whole design: the FR's own instruction is that reclamation must never
/// walk up to `data/` or the temp root, because that would take sibling
/// sessions and the database with it.
fn session_owned_dir(row: &SessionRow) -> Option<std::path::PathBuf> {
    let parent = std::path::Path::new(&row.input_fifo_path).parent()?;
    for other in [&row.transcript_path, &row.stdout_path, &row.stderr_path] {
        if std::path::Path::new(other).parent() != Some(parent) {
            return None;
        }
    }
    if parent.file_name()?.to_str()? != row.id {
        return None;
    }
    if parent.parent()?.file_name()?.to_str()? != "sessions" {
        return None;
    }
    Some(parent.to_path_buf())
}

/// Reconciles non-terminal persisted sessions with OS process identity and transport state.
///
/// `owner_pid` is the PID of the daemon that owns live sessions — normally
/// `std::process::id()`. It is what distinguishes a session this daemon is
/// running from one a previous daemon left behind; see [`process_is_owned_by`].
///
/// Rows already in `failed` are re-examined for reclamation even though their
/// state cannot move. Without that, a single missed reclamation is permanent: the
/// first pass marks an orphan `failed`, and every later pass filters it out
/// before looking at it. Anything that interrupts the pass between the state
/// change and the signal — a daemon restart, the policy being off at the time —
/// would otherwise strand the process forever.
pub(crate) fn reconcile_sessions(conn: &Connection, owner_pid: u32) -> Result<ReconcileOutcome> {
    let rows = list_sessions(conn, None, None, None)?;
    let mut outcome = ReconcileOutcome::default();
    for row in rows.into_iter().filter(|row| {
        matches!(
            row.state.as_str(),
            "opening" | "active" | "detached" | "draining" | "failed"
        )
    }) {
        let identity = process_identity_status(row.pid, row.process_fingerprint.as_deref());
        let transport_exists = std::path::Path::new(&row.input_fifo_path).exists();
        let evidence_exists = std::path::Path::new(&row.transcript_path).exists()
            || std::path::Path::new(&row.stdout_path).exists();
        let lease_is_current = row
            .writer_lease_expires_at
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|expires| expires > chrono::Utc::now());

        // A live process this daemon did not spawn is unreachable: nothing can
        // send it input or read what it writes. Whether its FIFO happens to
        // still exist on disk says nothing about that.
        let orphaned = identity == ProcessIdentityStatus::VerifiedLive
            && !process_is_owned_by(row.pid, owner_pid);

        let target = match identity {
            ProcessIdentityStatus::VerifiedLive if transport_exists && !orphaned => {
                if row.state == "draining" {
                    "draining"
                } else if row.writer_client_id.is_some() && lease_is_current {
                    "active"
                } else {
                    "detached"
                }
            }
            ProcessIdentityStatus::Dead if evidence_exists => "closed",
            ProcessIdentityStatus::VerifiedLive
            | ProcessIdentityStatus::Dead
            | ProcessIdentityStatus::Mismatch
            | ProcessIdentityStatus::Unsupported => "failed",
        };

        if identity == ProcessIdentityStatus::VerifiedLive && (orphaned || !transport_exists) {
            outcome.reclaim_candidates.push(ReclaimCandidate {
                session_id: row.id.clone(),
                task_id: row.task_id.clone(),
                pid: row.pid,
                process_fingerprint: row.process_fingerprint.clone(),
                session_dir: session_owned_dir(&row),
            });
        }

        if target != row.state {
            update_session_state(
                conn,
                &row.id,
                target,
                row.exit_code,
                matches!(target, "closed" | "failed"),
            )?;
            outcome.changes.push((row.id, target.to_owned()));
        }
    }
    let expired = expire_writer_leases(conn)?;
    outcome.changes.extend(
        expired
            .into_iter()
            .map(|id| (id, "lease_expired".to_owned())),
    );
    Ok(outcome)
}

/// Loads a session row by session identifier.
pub(crate) fn load_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    conn.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE id = ?1"),
        params![session_id],
        row_to_session,
    )
    .optional()
    .context("load session")
}

/// Loads the latest active or detached session for a task step.
pub(crate) fn load_active_session_for_task_step(
    conn: &Connection,
    task_id: &str,
    step_id: &str,
) -> Result<Option<SessionRow>> {
    conn.query_row(
        &format!(
            "SELECT {SESSION_COLUMNS}
             FROM agent_sessions
             WHERE task_id = ?1 AND step_id = ?2 AND state IN ('active','detached')
             ORDER BY created_at DESC
             LIMIT 1"
        ),
        params![task_id, step_id],
        row_to_session,
    )
    .optional()
    .context("load active session for task step")
}

/// Lists all sessions for a task ordered from newest to oldest.
pub(crate) fn list_task_sessions(conn: &Connection, task_id: &str) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLUMNS}
             FROM agent_sessions
             WHERE task_id = ?1
             ORDER BY created_at DESC"
    ))?;
    let rows = stmt
        .query_map(params![task_id], row_to_session)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Attempts to acquire the writer lease for a session.
pub(crate) fn acquire_writer(conn: &Connection, session_id: &str, client_id: &str) -> Result<bool> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT writer_client_id FROM agent_sessions WHERE id = ?1",
            params![session_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    if let Some(owner) = existing
        && !owner.is_empty()
        && owner != client_id
    {
        return Ok(false);
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
pub(crate) fn attach_reader(conn: &Connection, session_id: &str, client_id: &str) -> Result<()> {
    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM session_attachments
         WHERE session_id=?1 AND client_id=?2 AND mode='reader' AND detached_at IS NULL",
        params![session_id, client_id],
        |row| row.get(0),
    )?;
    if existing > 0 {
        return Ok(());
    }
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

/// A stale-session candidate whose process is still running, so its record was
/// preserved rather than deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedLiveSession {
    /// Session identifier whose record was kept.
    pub session_id: String,
    /// Recorded PID that answered the liveness probe.
    pub pid: i64,
    /// Terminal state the record carried when the sweep found it.
    pub state: String,
    /// Whether the live process is the same incarnation the record describes.
    ///
    /// `Mismatch` means the PID was reused by something unrelated: the record
    /// is genuinely stale and only the conservative deletion rule keeps it.
    /// Carried for diagnostics; it does not affect whether the row survives.
    pub identity: ProcessIdentityStatus,
}

/// Outcome of a stale-session sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleSessionSweep {
    /// Session records removed.
    pub deleted: usize,
    /// Candidates preserved because their recorded process is still alive.
    pub live_retained: Vec<RetainedLiveSession>,
}

/// Deletes old terminal sessions, preserving any whose process is still alive.
///
/// The age and terminal-state filter alone is not a safe deletion predicate.
/// `reconcile_sessions` marks a session `failed` when its process is verified
/// live but its transport has gone, so a running orphan acquires exactly the
/// state this sweep deletes — and deleting it destroys the only record that the
/// process exists, after the system has already declined to reclaim it. Giving
/// up on reclamation is a bug; erasing the evidence afterwards is what makes it
/// unrecoverable (FR-159).
///
/// A candidate whose PID answers a liveness probe is therefore retained and
/// reported. The probe is deliberately `process_exists` rather than a
/// fingerprint match: a reused PID keeps a record that could have been dropped,
/// which costs one stale row, while trusting a fingerprint that is merely
/// `Unsupported` would delete the record of a live process, which is the defect
/// itself. This errs toward keeping evidence.
pub(crate) fn cleanup_stale_sessions(
    conn: &Connection,
    max_age_hours: u64,
) -> Result<StaleSessionSweep> {
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(max_age_hours as i64);
    let cutoff = cutoff.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT id, pid, state, process_fingerprint FROM agent_sessions
         WHERE state IN ('exited','closed','failed') AND updated_at < ?1",
    )?;
    let candidates = stmt
        .query_map([&cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut sweep = StaleSessionSweep::default();
    for (session_id, pid, state, fingerprint) in candidates {
        if pid > 0 && process_exists(pid as u32) {
            sweep.live_retained.push(RetainedLiveSession {
                session_id,
                pid,
                state,
                identity: process_identity_status(pid, fingerprint.as_deref()),
            });
            continue;
        }
        conn.execute(
            "DELETE FROM session_control_actions WHERE session_id = ?1",
            params![session_id],
        )?;
        conn.execute(
            "DELETE FROM session_attachments WHERE session_id = ?1",
            params![session_id],
        )?;
        sweep.deleted += conn.execute(
            "DELETE FROM agent_sessions WHERE id = ?1",
            params![session_id],
        )?;
    }
    Ok(sweep)
}

/// Releases a reader or writer attachment for a client.
pub(crate) fn release_attachment(
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
    pub async fn cleanup_stale_sessions(&self, max_age_hours: u64) -> Result<StaleSessionSweep> {
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

// ─── Async facades ───────────────────────────────────────────────
//
// The statements above take a connection because they are also called from
// inside larger transactions in this crate. The wrappers below are how callers
// *outside* it reach them: FR-141 removed `AsyncDatabase::writer()` from the
// public API, so a caller that used to open the closure itself now names the
// operation instead. Each one is the closure that stood at the call site,
// moved.

use crate::async_database::flatten_err;

fn other(error: anyhow::Error) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into())
}

/// Lists sessions matching the optional task, agent and state filters.
pub async fn list_sessions_async(
    db: &AsyncDatabase,
    task_id: Option<String>,
    agent_id: Option<String>,
    state: Option<String>,
) -> Result<Vec<SessionRow>> {
    db.reader()
        .call(move |conn| {
            list_sessions(
                conn,
                task_id.as_deref(),
                agent_id.as_deref(),
                state.as_deref(),
            )
            .map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Lists sessions whose recorded process id is `pid`.
pub async fn list_sessions_by_pid_async(db: &AsyncDatabase, pid: i64) -> Result<Vec<SessionRow>> {
    db.reader()
        .call(move |conn| list_sessions_by_pid(conn, pid).map_err(other))
        .await
        .map_err(flatten_err)
}

/// Acquires or renews the writer lease for a session.
pub async fn acquire_writer_lease_async(
    db: &AsyncDatabase,
    session_id: String,
    actor: String,
    client_id: String,
    ttl_secs: u64,
) -> Result<Option<WriterLease>> {
    db.writer()
        .call(move |conn| {
            acquire_writer_lease(conn, &session_id, &actor, &client_id, ttl_secs).map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Extends the writer lease, returning the new expiry when the token is live.
pub async fn heartbeat_writer_async(
    db: &AsyncDatabase,
    session_id: String,
    client_id: String,
    fencing_token: i64,
    ttl_secs: u64,
) -> Result<Option<String>> {
    db.writer()
        .call(move |conn| {
            heartbeat_writer(conn, &session_id, &client_id, fencing_token, ttl_secs).map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Reports whether a client still holds a live writer lease.
pub async fn validate_writer_async(
    db: &AsyncDatabase,
    session_id: String,
    client_id: String,
    fencing_token: i64,
) -> Result<bool> {
    db.reader()
        .call(move |conn| {
            validate_writer(conn, &session_id, &client_id, fencing_token).map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Releases the writer lease, returning whether it was held.
pub async fn release_writer_async(
    db: &AsyncDatabase,
    session_id: String,
    client_id: String,
    fencing_token: i64,
    reason: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            release_writer(conn, &session_id, &client_id, fencing_token, &reason).map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Detaches a reader attachment, recording when and why.
pub async fn detach_reader(
    db: &AsyncDatabase,
    session_id: String,
    client_id: String,
    detached_at: String,
    reason: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE session_attachments SET detached_at=?3,reason=?4 WHERE session_id=?1 AND client_id=?2 AND mode='reader' AND detached_at IS NULL",
                params![session_id, client_id, detached_at, reason],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Reconciles recorded session state against the processes still alive.
///
/// `owner_pid` identifies the daemon that owns live sessions; see
/// [`reconcile_sessions`].
pub async fn reconcile_sessions_async(
    db: &AsyncDatabase,
    owner_pid: u32,
) -> Result<ReconcileOutcome> {
    db.writer()
        .call(move |conn| reconcile_sessions(conn, owner_pid).map_err(other))
        .await
        .map_err(flatten_err)
}

/// Records the OS process backing a session.
pub async fn update_session_process_async(
    db: &AsyncDatabase,
    session_id: String,
    pid: i64,
    fingerprint: Option<String>,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            update_session_process(conn, &session_id, pid, fingerprint.as_deref()).map_err(other)
        })
        .await
        .map_err(flatten_err)
}

/// Reconciles session state through a connection this module opens.
///
/// Bootstrap runs this before any async runtime is guaranteed to exist —
/// `init_state` builds its managed state synchronously — so the path form is
/// what keeps the connection inside the layer without forcing that call site
/// to become async.
pub fn reconcile_sessions_by_path(
    db_path: &std::path::Path,
    owner_pid: u32,
) -> Result<ReconcileOutcome> {
    let conn = crate::sqlite::open_conn(db_path)?;
    reconcile_sessions(&conn, owner_pid)
}

/// Lists every non-terminal session whose process is still running.
///
/// Used by the shutdown drain, which wants all live sessions rather than only
/// the orphaned ones: on the way down, a session this daemon still owns is
/// exactly the session that is about to *become* an orphan.
///
/// Reported without regard to ownership or transport, because the reclamation
/// primitive re-verifies identity and group leadership before it signals
/// anything — the filtering that matters happens there, next to the kill.
pub fn live_sessions_by_path(db_path: &std::path::Path) -> Result<Vec<ReclaimCandidate>> {
    let conn = crate::sqlite::open_conn(db_path)?;
    let rows = list_sessions(&conn, None, None, None)?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            matches!(
                row.state.as_str(),
                "opening" | "active" | "detached" | "draining" | "failed"
            ) && row.pid > 0
                && process_exists(row.pid as u32)
        })
        .map(|row| ReclaimCandidate {
            session_id: row.id.clone(),
            task_id: row.task_id.clone(),
            pid: row.pid,
            process_fingerprint: row.process_fingerprint.clone(),
            session_dir: session_owned_dir(&row),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use crate::db::{init_schema, open_conn};
    use rusqlite::params;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db_path = dir.path().join("sessions.db");
        init_schema(&db_path).expect("init schema");
        (dir, db_path)
    }

    /// Spawns a process group leader with a child, mirroring a real session.
    ///
    /// The shell puts itself in its own process group and starts a background
    /// `sleep`, so the group has a leader and a descendant. That distinction is
    /// what separates a group signal from a leader-only one; a fixture with a
    /// single process cannot tell them apart.
    fn spawn_group_with_child() -> (std::process::Child, u32) {
        use std::io::Read;
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            // setsid may be unavailable; fall back to running in this group and
            // let the leadership assertion below decide what the test can claim.
            .arg("sleep 60 & echo $! ; wait")
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("spawn session-like group");
        let mut out = child.stdout.take().expect("capture stdout");
        let mut buf = String::new();
        // Read just the grandchild PID line.
        let mut byte = [0u8; 1];
        while out.read(&mut byte).unwrap_or(0) == 1 {
            if byte[0] == b'\n' {
                break;
            }
            buf.push(byte[0] as char);
        }
        let grandchild: u32 = buf.trim().parse().expect("grandchild pid");
        (child, grandchild)
    }

    /// A verified, group-leading PID is reclaimed together with its children.
    ///
    /// The grandchild assertion is the point. Signalling only the leader leaves
    /// the descendant alive and reparented, which looks identical in the leader's
    /// exit status — so a test that only checked the leader would pass on the
    /// defect this exists to prevent.
    #[test]
    fn reclaim_kills_the_whole_group_not_just_the_leader() {
        let (mut leader, grandchild) = spawn_group_with_child();
        let pid = leader.id() as i64;
        assert!(
            is_process_group_leader(pid),
            "fixture must lead its own group or it cannot exercise group reclamation"
        );
        let fingerprint = capture_process_fingerprint(pid as u32);
        assert!(process_exists(grandchild), "grandchild must start alive");

        let sent = reclaim_process_group(pid, fingerprint.as_deref(), ReclaimSignal::Immediate)
            .expect("a verified group leader must be reclaimable");
        assert!(sent.sigkill);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while process_exists(grandchild) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            !process_exists(grandchild),
            "the session's child must die with the group; if it survives, only the \
             leader was signalled and a new orphan has just been created"
        );
        let _ = leader.wait();
    }

    /// A fingerprint that does not match must produce no signal at all.
    ///
    /// The process must still be alive afterwards: asserting only that the call
    /// returned an error would pass just as well on an implementation that
    /// signalled first and reported second.
    #[test]
    fn reclaim_refuses_and_sends_nothing_when_the_fingerprint_mismatches() {
        let (mut leader, grandchild) = spawn_group_with_child();
        let pid = leader.id() as i64;

        let refusal = reclaim_process_group(
            pid,
            Some("not-the-real-fingerprint"),
            ReclaimSignal::Immediate,
        )
        .expect_err("a mismatched fingerprint must refuse");
        assert_eq!(refusal, ReclaimRefusal::IdentityMismatch);

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            process_exists(pid as u32),
            "a refused reclamation must leave the process running: this is the \
             PID-reuse guard, and killing first would defeat it"
        );
        assert!(
            process_exists(grandchild),
            "nor may its children be signalled"
        );

        let _ = leader.kill();
        let _ = leader.wait();
        // SAFETY: cleaning up the fixture's own descendant.
        unsafe { libc::kill(grandchild as i32, libc::SIGKILL) };
    }

    /// A PID that does not lead its own group must be refused.
    ///
    /// Without this, `kill(-pid, …)` would address whichever group happens to
    /// carry that number. The fixture is a process deliberately left in the test
    /// runner's group, so it is live and fingerprint-verifiable — every other
    /// precondition passes and only leadership fails.
    #[test]
    fn reclaim_refuses_a_pid_that_does_not_lead_its_group() {
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("spawn non-leader");
        let pid = child.id() as i64;
        assert!(
            !is_process_group_leader(pid),
            "fixture must NOT lead its group, otherwise this asserts nothing"
        );
        let fingerprint = capture_process_fingerprint(pid as u32);

        let refusal = reclaim_process_group(pid, fingerprint.as_deref(), ReclaimSignal::Immediate)
            .expect_err("a non-leader must be refused");
        assert_eq!(refusal, ReclaimRefusal::NotGroupLeader);
        assert!(
            process_exists(pid as u32),
            "the refused process must survive"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// Directory reclamation must refuse anything it cannot prove it owns.
    ///
    /// Each case relaxes exactly one of the three conditions and nothing else,
    /// so a rule that stopped checking that one condition is the only thing that
    /// can turn it green. The accepted case is included so the test cannot pass
    /// by refusing everything — a `session_owned_dir` that always returned
    /// `None` would be perfectly safe and perfectly useless, and the FR's
    /// requirement is that the session's own directory does get removed.
    #[test]
    fn session_owned_dir_refuses_every_path_it_cannot_prove_it_owns() {
        let row_with = |fifo: &str, transcript: &str, stdout: &str, stderr: &str, id: &str| {
            let base = make_session(id, "task-1", "qa", "active");
            let mut row = SessionRow {
                id: base.id.to_string(),
                task_id: base.task_id.to_string(),
                task_item_id: None,
                step_id: base.step_id.to_string(),
                phase: base.phase.to_string(),
                agent_id: base.agent_id.to_string(),
                state: base.state.to_string(),
                pid: 1,
                pty_backend: base.pty_backend.to_string(),
                cwd: base.cwd.to_string(),
                command: base.command.to_string(),
                input_fifo_path: fifo.to_string(),
                stdout_path: stdout.to_string(),
                stderr_path: stderr.to_string(),
                transcript_path: transcript.to_string(),
                output_json_path: None,
                writer_client_id: None,
                writer_actor: None,
                writer_lease_expires_at: None,
                writer_last_heartbeat_at: None,
                writer_fencing_token: 0,
                state_version: 1,
                process_fingerprint: None,
                created_at: String::new(),
                updated_at: String::new(),
                ended_at: None,
                exit_code: None,
            };
            row.id = id.to_string();
            row
        };

        // Accepted: the layout phase_runner::spawn actually creates.
        let good = row_with(
            "/data/logs/sessions/sess-1/input.fifo",
            "/data/logs/sessions/sess-1/transcript.log",
            "/data/logs/sessions/sess-1/stdout.log",
            "/data/logs/sessions/sess-1/stderr.log",
            "sess-1",
        );
        assert_eq!(
            session_owned_dir(&good),
            Some(std::path::PathBuf::from("/data/logs/sessions/sess-1")),
            "the session's own directory must be derivable, or nothing is ever cleaned"
        );

        // Refused: transcript lives somewhere else, so the parent is not agreed.
        let split = row_with(
            "/data/logs/sessions/sess-1/input.fifo",
            "/data/logs/transcript.log",
            "/data/logs/sessions/sess-1/stdout.log",
            "/data/logs/sessions/sess-1/stderr.log",
            "sess-1",
        );
        assert_eq!(
            session_owned_dir(&split),
            None,
            "paths that disagree on their parent must not authorise a deletion"
        );

        // Refused: directory is not named for this session.
        let misnamed = row_with(
            "/data/logs/sessions/other/input.fifo",
            "/data/logs/sessions/other/transcript.log",
            "/data/logs/sessions/other/stdout.log",
            "/data/logs/sessions/other/stderr.log",
            "sess-1",
        );
        assert_eq!(
            session_owned_dir(&misnamed),
            None,
            "a directory named for a different session is somebody else's data"
        );

        // Refused: not under a `sessions` root — this is the shape that would
        // walk up into the data directory.
        let outside = row_with(
            "/data/sess-1/input.fifo",
            "/data/sess-1/transcript.log",
            "/data/sess-1/stdout.log",
            "/data/sess-1/stderr.log",
            "sess-1",
        );
        assert_eq!(
            session_owned_dir(&outside),
            None,
            "a path outside the sessions tree must never be removed"
        );
    }

    /// A live process this daemon did not spawn is an orphan.
    #[test]
    fn process_ownership_distinguishes_our_children_from_strangers() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn child");
        let pid = child.id() as i64;

        assert!(
            process_is_owned_by(pid, std::process::id()),
            "a process we just spawned must read as ours"
        );
        assert!(
            !process_is_owned_by(pid, std::process::id().wrapping_add(1)),
            "the same process must read as an orphan against a different owner, \
             which is the daemon-restart case"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    /// A dead PID is reported as such rather than signalled blindly.
    #[test]
    fn reclaim_refuses_a_process_that_has_already_exited() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn short-lived");
        let pid = child.id() as i64;
        child.wait().expect("reap");

        let refusal = reclaim_process_group(pid, Some("whatever"), ReclaimSignal::Immediate)
            .expect_err("a dead PID must be refused");
        assert_eq!(refusal, ReclaimRefusal::ProcessGone);
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

        let sweep = cleanup_stale_sessions(&conn, 72).expect("cleanup");
        assert_eq!(sweep.deleted, 1);
        assert!(
            sweep.live_retained.is_empty(),
            "fixture PIDs are not running, so nothing should be retained for liveness"
        );

        // Verify correct session was deleted
        assert!(load_session(&conn, "old-exited").expect("load").is_none());
        assert!(load_session(&conn, "old-active").expect("load").is_some());
        assert!(load_session(&conn, "new-exited").expect("load").is_some());
    }

    /// A terminal record whose process is still running must survive the sweep.
    ///
    /// This is the amnesia path in FR-159: `reconcile_sessions` marks a live
    /// orphan `failed`, and a sweep keyed on state and age then erases the only
    /// record that the process exists. The two rows here are identical in state,
    /// age and shape, and differ only in whether their PID answers — so the
    /// assertion cannot be satisfied by a sweep that simply deleted less, or by
    /// one that deleted nothing at all.
    #[test]
    fn cleanup_stale_sessions_retains_records_of_live_processes() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");

        // A real process that outlives the sweep.
        let mut live = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn live process");
        let live_pid = live.id() as i64;

        // A PID that is definitely not running: spawn and reap it first.
        let mut dead = std::process::Command::new("true")
            .spawn()
            .expect("spawn short-lived process");
        let dead_pid = dead.id() as i64;
        dead.wait().expect("reap short-lived process");

        let old_ts = (chrono::Utc::now() - chrono::Duration::hours(100)).to_rfc3339();
        for (id, pid) in [("live-failed", live_pid), ("dead-failed", dead_pid)] {
            insert_session(&conn, &make_session(id, "task-1", "qa", "failed"))
                .expect("insert failed session");
            conn.execute(
                "UPDATE agent_sessions SET updated_at = ?2, pid = ?3 WHERE id = ?1",
                params![id, old_ts, pid],
            )
            .expect("backdate and set pid");
        }

        let sweep = cleanup_stale_sessions(&conn, 72).expect("cleanup");

        assert_eq!(
            sweep.deleted, 1,
            "only the record whose process is gone may be deleted"
        );
        assert!(
            load_session(&conn, "live-failed").expect("load").is_some(),
            "a live process must keep its record: erasing it is what makes an \
             unreclaimed orphan untrackable"
        );
        assert!(
            load_session(&conn, "dead-failed").expect("load").is_none(),
            "a genuinely dead session must still be swept, or the sweep does nothing"
        );

        let retained = sweep
            .live_retained
            .iter()
            .find(|entry| entry.session_id == "live-failed")
            .expect("the retained live session must be reported, not silently kept");
        assert_eq!(retained.pid, live_pid);
        assert_eq!(retained.state, "failed");

        let _ = live.kill();
        let _ = live.wait();
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
        let renewed = heartbeat_writer(&conn, "sess-fence", "client-a", first.fencing_token, 60)
            .expect("heartbeat")
            .expect("current writer renews");
        assert!(renewed > first.expires_at);
        assert!(
            heartbeat_writer(&conn, "sess-fence", "client-b", first.fencing_token, 60,)
                .unwrap()
                .is_none()
        );
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
    fn concurrent_writer_race_grants_exactly_one_client() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(&conn, &make_session("sess-race", "task-1", "qa", "active"))
            .expect("insert session");
        drop(conn);

        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for client in ["client-a", "client-b"] {
            let path = db_path.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let conn = open_conn(&path).expect("open racing connection");
                barrier.wait();
                acquire_writer_lease(&conn, "sess-race", client, client, 30)
                    .expect("race acquisition")
                    .is_some()
            }));
        }
        barrier.wait();
        let granted = handles
            .into_iter()
            .map(|handle| handle.join().expect("writer thread"))
            .filter(|granted| *granted)
            .count();
        assert_eq!(granted, 1);
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
        assert_eq!(
            process_identity_status(pid as i64, Some(&fingerprint)),
            ProcessIdentityStatus::VerifiedLive
        );
        assert_eq!(
            process_identity_status(pid as i64, Some("stale-fingerprint")),
            ProcessIdentityStatus::Mismatch
        );
        assert_eq!(
            process_identity_status(pid as i64, None),
            ProcessIdentityStatus::Unsupported
        );
        assert_eq!(
            process_identity_status(u32::MAX as i64, Some("missing")),
            ProcessIdentityStatus::Dead
        );
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
        attach_reader(&conn, "sess-bounds", "reader-0").expect("same reader is idempotent");
        assert!(attach_reader(&conn, "sess-bounds", "reader-9").is_err());
        let active_readers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM session_attachments
                 WHERE session_id='sess-bounds' AND mode='reader' AND detached_at IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("count active readers");
        assert_eq!(active_readers, 8);

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

    #[test]
    fn expired_writer_cleanup_never_resurrects_terminal_session() {
        let (_dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        insert_session(
            &conn,
            &make_session("sess-terminal", "task-1", "qa", "closed"),
        )
        .expect("insert terminal session");
        conn.execute(
            "UPDATE agent_sessions
             SET writer_client_id='old-writer', writer_actor='operator',
                 writer_lease_expires_at='1970-01-01T00:00:00Z'
             WHERE id='sess-terminal'",
            [],
        )
        .expect("seed expired terminal lease");

        assert_eq!(expire_writer_leases(&conn).unwrap(), vec!["sess-terminal"]);
        let row = load_session(&conn, "sess-terminal")
            .expect("load session")
            .expect("session exists");
        assert_eq!(row.state, "closed");
        assert!(row.writer_client_id.is_none());
    }

    #[test]
    fn reconciliation_distinguishes_dead_process_from_live_identity_mismatch() {
        let (dir, db_path) = make_db();
        let conn = open_conn(&db_path).expect("open conn");
        let transport = dir.path().join("input.fifo");
        let evidence = dir.path().join("transcript.log");
        std::fs::write(&transport, "transport").expect("create transport marker");
        std::fs::write(&evidence, "evidence").expect("create transcript evidence");

        insert_session(
            &conn,
            &make_session("sess-mismatch", "task-1", "qa", "active"),
        )
        .expect("insert mismatch session");
        conn.execute(
            "UPDATE agent_sessions
             SET pid=?2, process_fingerprint='stale-fingerprint',
                 input_fifo_path=?3, transcript_path=?4, stdout_path=?4
             WHERE id=?1",
            params![
                "sess-mismatch",
                std::process::id() as i64,
                transport.to_string_lossy(),
                evidence.to_string_lossy()
            ],
        )
        .expect("seed live mismatch");

        insert_session(&conn, &make_session("sess-dead", "task-1", "qa", "active"))
            .expect("insert dead session");
        conn.execute(
            "UPDATE agent_sessions
             SET pid=?2, process_fingerprint='missing', transcript_path=?3, stdout_path=?3
             WHERE id=?1",
            params!["sess-dead", u32::MAX as i64, evidence.to_string_lossy()],
        )
        .expect("seed dead session");

        let outcome = reconcile_sessions(&conn, std::process::id()).expect("reconcile sessions");
        assert!(
            outcome
                .changes
                .contains(&("sess-mismatch".into(), "failed".into()))
        );
        assert!(
            outcome
                .changes
                .contains(&("sess-dead".into(), "closed".into()))
        );
        assert!(
            outcome.reclaim_candidates.is_empty(),
            "neither a reused PID nor a dead one may be proposed for reclamation"
        );
        assert_eq!(
            load_session(&conn, "sess-mismatch").unwrap().unwrap().state,
            "failed"
        );
        assert_eq!(
            load_session(&conn, "sess-dead").unwrap().unwrap().state,
            "closed"
        );
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

        let sweep = store
            .cleanup_stale_sessions(72)
            .await
            .expect("cleanup stale sessions");
        assert_eq!(sweep.deleted, 1);
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
