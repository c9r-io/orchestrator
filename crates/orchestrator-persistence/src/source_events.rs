//! The source-ingestion tables: `source_events`, `source_routing_attempts`,
//! `source_bindings` and `source_command_actions`.
//!
//! `core::source` decides what a well-formed external event is, derives the
//! deterministic identifiers, chooses the retry backoff, and says what a reused
//! idempotency key means. This module holds the statements those decisions are
//! written with, and the two multi-statement operations that have to stay
//! atomic — claiming a batch for routing, and closing one out.
//!
//! Payloads cross the boundary as JSON text. `normalized_payload_json` is a
//! `core` type serialized; parsing it back is core's job, which is also why the
//! row reader here no longer has to dress a `serde_json` failure up as a
//! column-conversion failure (FR-130 B11).

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::async_database::{AsyncDatabase, flatten_err};

/// Longest a claim batch may be, whatever the caller asks for.
pub const MAX_CLAIM_BATCH: usize = 100;

/// How many routing attempts an event gets before it stops being claimed.
pub const MAX_ROUTING_ATTEMPTS: i64 = 5;

/// Longest list any read here will return, whatever the caller asks for.
pub const MAX_LIST_ROWS: usize = 500;

/// A normalized external event to be recorded.
#[derive(Debug, Clone)]
pub struct NewSourceEvent {
    /// Deterministic event identifier, derived by the caller.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Provider installation.
    pub installation_id: String,
    /// Provider-side event identifier.
    pub external_event_id: String,
    /// Event kind label.
    pub event_type: String,
    /// Provider-side actor identifier.
    pub external_actor_id: String,
    /// Conversation the event belongs to, when it has one.
    pub conversation_id: Option<String>,
    /// Thread within the conversation, when it has one.
    pub thread_id: Option<String>,
    /// Provider-reported occurrence time.
    pub occurred_at: String,
    /// Local receipt time.
    pub received_at: String,
    /// The normalized event, already serialized.
    pub normalized_payload_json: String,
    /// Reference to the raw payload when one was retained.
    pub raw_payload_ref: Option<String>,
    /// Hash the caller computed over the payload.
    pub payload_hash: String,
}

/// One `source_events` row, with its payload still unparsed.
#[derive(Debug, Clone)]
pub struct SourceEventRow {
    /// Event identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Provider installation.
    pub installation_id: String,
    /// Provider-side event identifier.
    pub external_event_id: String,
    /// Event kind label.
    pub event_type: String,
    /// Provider-side actor identifier; the column is nullable for rows written
    /// before the field existed.
    pub external_actor_id: Option<String>,
    /// Conversation the event belongs to.
    pub conversation_id: Option<String>,
    /// Thread within the conversation.
    pub thread_id: Option<String>,
    /// Provider-reported occurrence time.
    pub occurred_at: String,
    /// Local receipt time.
    pub received_at: String,
    /// The normalized event as stored, for the caller to parse.
    pub normalized_payload_json: String,
    /// Hash over the payload.
    pub payload_hash: String,
    /// Current routing state.
    pub routing_state: String,
    /// How many routing attempts have been made.
    pub routing_attempts: i64,
    /// Task the event routed to, once it did.
    pub routed_task_id: Option<String>,
    /// Last recorded routing error code.
    pub last_error_code: Option<String>,
}

/// A correlation binding between an external conversation and a task.
#[derive(Debug, Clone)]
pub struct NewSourceBinding {
    /// Deterministic binding identifier, derived by the caller.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Task the correlation binds to.
    pub task_id: String,
    /// Provider label.
    pub provider: String,
    /// Provider installation.
    pub installation_id: String,
    /// Conversation, when the binding has one.
    pub conversation_id: Option<String>,
    /// Thread, when the binding has one.
    pub thread_id: Option<String>,
    /// Correlation key the caller derived.
    pub correlation_key: String,
    /// Binding kind label.
    pub binding_type: String,
    /// Event that created the binding.
    pub created_by_event_id: String,
}

/// One `source_bindings` row.
#[derive(Debug, Clone)]
pub struct SourceBindingRow {
    /// Binding identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Task the correlation binds to.
    pub task_id: String,
    /// Provider label.
    pub provider: String,
    /// Provider installation.
    pub installation_id: String,
    /// Conversation, when the binding has one.
    pub conversation_id: Option<String>,
    /// Thread, when the binding has one.
    pub thread_id: Option<String>,
    /// Binding kind label.
    pub binding_type: String,
    /// Event that created the binding.
    pub created_by_event_id: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// A command action to be started under a retry identity.
#[derive(Debug, Clone)]
pub struct NewCommandAction {
    /// Deterministic action identifier, derived by the caller.
    pub id: String,
    /// Event the command arrived on.
    pub source_event_id: String,
    /// Provider-side actor.
    pub actor: String,
    /// Locally resolved role.
    pub resolved_role: String,
    /// Target kind.
    pub target_type: String,
    /// Target identifier.
    pub target_id: String,
    /// Action label.
    pub action: String,
    /// Retry identity.
    pub idempotency_key: String,
    /// Hash over the canonical request.
    pub request_hash: String,
    /// Correlating request identifier.
    pub request_id: String,
}

/// What [`begin_command_action`] found under the retry identity.
///
/// The caller turns these into diagnostics; the discrimination stays here
/// because the read and the write that follows it have to be one operation on
/// the writer connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandActionStart {
    /// No prior attempt: the row was inserted and the caller should execute.
    Started,
    /// A prior attempt is not terminal and has been reset for another run.
    Restarted,
    /// A prior attempt already succeeded; the caller must not execute again.
    AlreadySucceeded,
    /// The retry identity exists under a different canonical request.
    RequestMismatch,
}

/// Records an event, or reports that this identifier was already recorded.
///
/// Returns the stored row and whether this call inserted it. The caller
/// compares payload hashes: an identifier that comes back attached to a
/// different payload is a provider reusing an id, which is a contract question
/// rather than a storage one.
pub async fn ingest_event(
    db: &AsyncDatabase,
    event: NewSourceEvent,
) -> Result<(SourceEventRow, bool)> {
    db.writer()
        .call(move |conn| {
            ingest_event_blocking(conn, &event)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one event by identifier.
pub async fn read_event(db: &AsyncDatabase, id: String) -> Result<Option<SourceEventRow>> {
    db.reader()
        .call(move |conn| {
            read_event_blocking(conn, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Claims up to `limit` events for routing and returns them.
///
/// One transaction, because the select, the state flip and the attempt row have
/// to be indivisible: two daemons reading the same pending event and both
/// claiming it is exactly what this prevents. `stale_before` is the cutoff past
/// which a claim by someone else is treated as abandoned; the caller picks it.
pub async fn claim_pending_events(
    db: &AsyncDatabase,
    limit: usize,
    stale_before: String,
    now: String,
) -> Result<Vec<SourceEventRow>> {
    db.writer()
        .call(move |conn| {
            claim_pending_blocking(conn, limit, &stale_before, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Closes out a routing attempt, and reports whether the event was still in the
/// `routing` state to close.
///
/// `false` means someone else already moved it; the caller decides what that
/// means. The event update and the attempt update are one transaction so an
/// attempt row cannot describe a state the event never reached.
pub async fn complete_routing(
    db: &AsyncDatabase,
    id: String,
    state: String,
    task_id: Option<String>,
    error_code: Option<String>,
    next_attempt_at: Option<String>,
    now: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            complete_routing_blocking(
                conn,
                &id,
                &state,
                task_id.as_deref(),
                error_code.as_deref(),
                next_attempt_at.as_deref(),
                &now,
            )
            .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Records a binding if its identifier is free, then returns the stored row.
///
/// The row returned may be an older binding under the same identifier. The
/// caller compares its `task_id`: a correlation already bound elsewhere is a
/// contract violation, not a storage error.
pub async fn create_binding(
    db: &AsyncDatabase,
    binding: NewSourceBinding,
    now: String,
) -> Result<SourceBindingRow> {
    db.writer()
        .call(move |conn| {
            create_binding_blocking(conn, &binding, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one binding by identifier.
pub async fn read_binding(db: &AsyncDatabase, id: String) -> Result<Option<SourceBindingRow>> {
    db.reader()
        .call(move |conn| {
            read_binding_blocking(conn, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Starts, restarts or refuses a command action under its retry identity.
pub async fn begin_command_action(
    db: &AsyncDatabase,
    action: NewCommandAction,
    now: String,
) -> Result<CommandActionStart> {
    db.writer()
        .call(move |conn| {
            begin_command_action_blocking(conn, &action, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Lists event identifiers matching optional project, task and state filters,
/// newest first, then reads each row.
pub async fn list_events(
    db: &AsyncDatabase,
    project_id: Option<String>,
    task_id: Option<String>,
    routing_state: Option<String>,
    limit: usize,
) -> Result<Vec<SourceEventRow>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM source_events
                 WHERE (?1 IS NULL OR project_id=?1)
                   AND (?2 IS NULL OR routed_task_id=?2)
                   AND (?3 IS NULL OR routing_state=?3)
                 ORDER BY received_at DESC, id DESC LIMIT ?4",
            )?;
            let ids = stmt
                .query_map(
                    params![
                        project_id,
                        task_id,
                        routing_state,
                        limit.clamp(1, MAX_LIST_ROWS) as i64
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| read_event_blocking(conn, &id)?.context("source event missing"))
                .collect::<Result<Vec<_>>>()
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Counts deliveries and automation routes that still owe work.
///
/// One statement rather than two so the two halves are counted against the same
/// snapshot; a caller adding them up from separate reads could report a lag that
/// never existed at any instant.
pub async fn routing_lag(db: &AsyncDatabase) -> Result<u64> {
    db.reader()
        .call(|conn| {
            let lag: i64 = conn.query_row(
                "SELECT
                   (SELECT COUNT(*) FROM source_events
                    WHERE routing_state IN ('received','routing','failed') AND routing_attempts < 5)
                   +
                   (SELECT COUNT(*) FROM source_automation_routes
                    WHERE status IN ('matched','resolving','rendered','creating','retrying','suspended'))",
                [],
                |row| row.get(0),
            )?;
            Ok(lag as u64)
        })
        .await
        .map_err(flatten_err)
}

/// Hands a claimed delivery over to the durable automation-route worker.
///
/// Reports `false` when the event was not in `routing` — the same guard
/// [`complete_routing`] uses, for the same reason.
pub async fn defer_to_automation(
    db: &AsyncDatabase,
    id: String,
    route_id: String,
    now: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let changed = tx.execute(
                "UPDATE source_events SET routing_state='automation_pending',
                 automation_route_id=?2,last_error_code=NULL,next_attempt_at=NULL,
                 routing_claimed_at=NULL WHERE id=?1 AND routing_state='routing'",
                params![id, route_id],
            )?;
            if changed != 1 {
                return Ok(false);
            }
            tx.execute(
                "UPDATE source_routing_attempts SET result='automation_pending',
                 automation_route_id=?2,completed_at=?3
                 WHERE source_event_id=?1
                   AND attempt_no=(SELECT routing_attempts FROM source_events WHERE id=?1)",
                params![id, route_id, now],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await
        .map_err(flatten_err)
}

/// Projects one automation route's terminal outcome onto every delivery
/// attached to it, in one transaction.
pub async fn complete_automation_route(
    db: &AsyncDatabase,
    route_id: String,
    state: String,
    task_id: Option<String>,
    error_code: Option<String>,
    now: String,
) -> Result<()> {
    db.writer()
        .call(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let ids = {
                let mut stmt =
                    tx.prepare("SELECT id FROM source_events WHERE automation_route_id=?1")?;
                stmt.query_map([&route_id], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            for id in ids {
                tx.execute(
                    "UPDATE source_events SET routing_state=?2,
                     routed_task_id=COALESCE(?3,routed_task_id),last_error_code=?4,
                     next_attempt_at=NULL,routing_claimed_at=NULL,routed_at=?5
                     WHERE id=?1",
                    params![id, state, task_id, error_code, now],
                )?;
                tx.execute(
                    "UPDATE source_routing_attempts SET result=?2,task_id=?3,error_code=?4,
                     automation_route_id=?5,completed_at=COALESCE(completed_at,?6)
                     WHERE source_event_id=?1 AND attempt_no=(
                       SELECT MAX(attempt_no) FROM source_routing_attempts WHERE source_event_id=?1)",
                    params![id, state, task_id, error_code, route_id, now],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Returns every delivery attached to a replayed route to the
/// automation-pending projection without consuming a delivery retry.
pub async fn requeue_automation_route(db: &AsyncDatabase, route_id: String) -> Result<()> {
    db.writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE source_events SET routing_state='automation_pending',
                 routed_task_id=NULL,last_error_code=NULL,next_attempt_at=NULL,
                 routing_claimed_at=NULL,routed_at=NULL
                 WHERE automation_route_id=?1",
                [&route_id],
            )?;
            Ok(())
        })
        .await
        .map_err(flatten_err)
}

/// Requeues a failed or attention-blocked event, resetting its attempt count.
///
/// Reports `false` when the event was in neither state — replaying a routed
/// event would duplicate its task.
pub async fn replay_event(db: &AsyncDatabase, id: String) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE source_events SET routing_state='received', last_error_code=NULL,
                 next_attempt_at=NULL, routing_attempts=0, routing_claimed_at=NULL
                 WHERE id=?1 AND routing_state IN ('failed','needs_attention')",
                [&id],
            )?;
            Ok(changed == 1)
        })
        .await
        .map_err(flatten_err)
}

/// Finds bindings at exact provider conversation coordinates, oldest first.
///
/// `IS` rather than `=` on the nullable columns: a binding with no thread must
/// match a lookup with no thread, and SQL equality never matches NULL.
pub async fn find_bindings(
    db: &AsyncDatabase,
    provider: String,
    installation_id: String,
    conversation_id: Option<String>,
    thread_id: Option<String>,
) -> Result<Vec<SourceBindingRow>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM source_bindings WHERE provider=?1 AND installation_id=?2
                 AND conversation_id IS ?3 AND thread_id IS ?4
                 ORDER BY created_at ASC, id ASC",
            )?;
            let ids = stmt
                .query_map(
                    params![provider, installation_id, conversation_id, thread_id],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| read_binding_blocking(conn, &id)?.context("binding missing"))
                .collect::<Result<Vec<_>>>()
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Lists one task's bindings, newest first.
pub async fn list_bindings(db: &AsyncDatabase, task_id: String) -> Result<Vec<SourceBindingRow>> {
    db.reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM source_bindings WHERE task_id=?1 ORDER BY created_at DESC",
            )?;
            let ids = stmt
                .query_map([task_id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| read_binding_blocking(conn, &id)?.context("binding missing"))
                .collect::<Result<Vec<_>>>()
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Closes out a reserved command action.
///
/// Reports `false` when no reservation matched, which the caller reads as a
/// completion for something it never reserved.
pub async fn complete_command_action(
    db: &AsyncDatabase,
    source_event_id: String,
    idempotency_key: String,
    status: String,
    result_json: Option<String>,
    error_code: Option<String>,
    now: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE source_command_actions SET status=?3,result_json=?4,error_code=?5,
                 completed_at=?6 WHERE source_event_id=?1 AND idempotency_key=?2",
                params![
                    source_event_id,
                    idempotency_key,
                    status,
                    result_json,
                    error_code,
                    now
                ],
            )?;
            Ok(changed == 1)
        })
        .await
        .map_err(flatten_err)
}

fn ingest_event_blocking(
    conn: &Connection,
    event: &NewSourceEvent,
) -> Result<(SourceEventRow, bool)> {
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO source_events
         (id,project_id,provider,installation_id,external_event_id,event_type,
          external_actor_id,conversation_id,thread_id,occurred_at,received_at,
          normalized_payload_json,raw_payload_ref,payload_hash,routing_state)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'received')",
        params![
            event.id,
            event.project_id,
            event.provider,
            event.installation_id,
            event.external_event_id,
            event.event_type,
            event.external_actor_id,
            event.conversation_id,
            event.thread_id,
            event.occurred_at,
            event.received_at,
            event.normalized_payload_json,
            event.raw_payload_ref,
            event.payload_hash,
        ],
    )? == 1;
    let row = read_event_blocking(conn, &event.id)?.context("inserted source event missing")?;
    Ok((row, inserted))
}

fn claim_pending_blocking(
    conn: &Connection,
    limit: usize,
    stale_before: &str,
    now: &str,
) -> Result<Vec<SourceEventRow>> {
    let tx = conn.unchecked_transaction()?;
    let ids = {
        let mut stmt = tx.prepare(
            "SELECT id FROM source_events
             WHERE (routing_state IN ('received','failed')
                    OR (routing_state='routing' AND routing_claimed_at <= ?2))
               AND routing_attempts < ?3
               AND (next_attempt_at IS NULL OR next_attempt_at <= datetime('now'))
             ORDER BY received_at ASC, id ASC LIMIT ?1",
        )?;
        stmt.query_map(
            params![
                limit.clamp(1, MAX_CLAIM_BATCH) as i64,
                stale_before,
                MAX_ROUTING_ATTEMPTS
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for id in &ids {
        tx.execute(
            "UPDATE source_events SET routing_state='routing', routing_attempts=routing_attempts+1,
             last_error_code=NULL,routing_claimed_at=?2 WHERE id=?1",
            params![id, now],
        )?;
        tx.execute(
            "INSERT INTO source_routing_attempts(source_event_id,attempt_no,result,created_at)
             SELECT id,routing_attempts,'routing',?2 FROM source_events WHERE id=?1",
            params![id, now],
        )?;
    }
    tx.commit()?;
    ids.into_iter()
        .map(|id| read_event_blocking(conn, &id)?.context("claimed source event missing"))
        .collect()
}

fn complete_routing_blocking(
    conn: &Connection,
    id: &str,
    state: &str,
    task_id: Option<&str>,
    error_code: Option<&str>,
    next_attempt_at: Option<&str>,
    now: &str,
) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "UPDATE source_events SET routing_state=?2, routed_task_id=COALESCE(?3,routed_task_id),
         last_error_code=?4, next_attempt_at=?5, routing_claimed_at=NULL,
         routed_at=CASE WHEN ?2 IN ('routed','ignored','needs_attention') THEN ?6 ELSE routed_at END
         WHERE id=?1 AND routing_state='routing'",
        params![id, state, task_id, error_code, next_attempt_at, now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    tx.execute(
        "UPDATE source_routing_attempts SET result=?2,task_id=?3,error_code=?4,completed_at=?5
         WHERE source_event_id=?1 AND attempt_no=(SELECT routing_attempts FROM source_events WHERE id=?1)",
        params![id, state, task_id, error_code, now],
    )?;
    tx.commit()?;
    Ok(true)
}

fn create_binding_blocking(
    conn: &Connection,
    binding: &NewSourceBinding,
    now: &str,
) -> Result<SourceBindingRow> {
    conn.execute(
        "INSERT OR IGNORE INTO source_bindings
         (id,project_id,task_id,provider,installation_id,conversation_id,thread_id,
          correlation_key,binding_type,created_by_event_id,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            binding.id,
            binding.project_id,
            binding.task_id,
            binding.provider,
            binding.installation_id,
            binding.conversation_id,
            binding.thread_id,
            binding.correlation_key,
            binding.binding_type,
            binding.created_by_event_id,
            now,
        ],
    )?;
    read_binding_blocking(conn, &binding.id)?.context("source binding missing")
}

fn begin_command_action_blocking(
    conn: &Connection,
    action: &NewCommandAction,
    now: &str,
) -> Result<CommandActionStart> {
    let existing = conn
        .query_row(
            "SELECT request_hash,status FROM source_command_actions
             WHERE source_event_id=?1 AND idempotency_key=?2",
            params![action.source_event_id, action.idempotency_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    if let Some((request_hash, status)) = existing {
        if request_hash != action.request_hash {
            return Ok(CommandActionStart::RequestMismatch);
        }
        if status == "succeeded" {
            return Ok(CommandActionStart::AlreadySucceeded);
        }
        conn.execute(
            "UPDATE source_command_actions SET status='running',result_json=NULL,error_code=NULL,
             completed_at=NULL WHERE source_event_id=?1 AND idempotency_key=?2",
            params![action.source_event_id, action.idempotency_key],
        )?;
        return Ok(CommandActionStart::Restarted);
    }
    conn.execute(
        "INSERT INTO source_command_actions
         (id,source_event_id,actor,resolved_role,target_type,target_id,action,idempotency_key,
          request_hash,status,created_at,request_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'running',?10,?11)",
        params![
            action.id,
            action.source_event_id,
            action.actor,
            action.resolved_role,
            action.target_type,
            action.target_id,
            action.action,
            action.idempotency_key,
            action.request_hash,
            now,
            action.request_id,
        ],
    )?;
    Ok(CommandActionStart::Started)
}

fn read_event_blocking(conn: &Connection, id: &str) -> Result<Option<SourceEventRow>> {
    conn.query_row(
        "SELECT id,project_id,provider,installation_id,external_event_id,event_type,
         external_actor_id,conversation_id,thread_id,occurred_at,received_at,
         normalized_payload_json,payload_hash,routing_state,routing_attempts,
         routed_task_id,last_error_code FROM source_events WHERE id=?1",
        [id],
        |row| {
            Ok(SourceEventRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                provider: row.get(2)?,
                installation_id: row.get(3)?,
                external_event_id: row.get(4)?,
                event_type: row.get(5)?,
                external_actor_id: row.get(6)?,
                conversation_id: row.get(7)?,
                thread_id: row.get(8)?,
                occurred_at: row.get(9)?,
                received_at: row.get(10)?,
                normalized_payload_json: row.get(11)?,
                payload_hash: row.get(12)?,
                routing_state: row.get(13)?,
                routing_attempts: row.get(14)?,
                routed_task_id: row.get(15)?,
                last_error_code: row.get(16)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn read_binding_blocking(conn: &Connection, id: &str) -> Result<Option<SourceBindingRow>> {
    conn.query_row(
        "SELECT id,project_id,task_id,provider,installation_id,conversation_id,thread_id,
         binding_type,created_by_event_id,created_at FROM source_bindings WHERE id=?1",
        [id],
        |row| {
            Ok(SourceBindingRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                provider: row.get(3)?,
                installation_id: row.get(4)?,
                conversation_id: row.get(5)?,
                thread_id: row.get(6)?,
                binding_type: row.get(7)?,
                created_by_event_id: row.get(8)?,
                created_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}
