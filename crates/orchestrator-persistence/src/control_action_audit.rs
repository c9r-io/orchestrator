//! The `control_action_audit` table: durable envelopes for state-changing
//! control-plane actions.
//!
//! What lives here is the table and the statements over it. What deliberately
//! does not is the control-plane contract above them — which fields are bounded
//! and how, how a canonical request is reduced to a hash, which lifecycle
//! statuses a caller may ask for, and what a reused idempotency key means. Those
//! stayed in `core::action_audit`, which settles all of them before it builds a
//! [`NewActionAudit`] and reads back the outcome this module reports.
//!
//! The split shows up in [`Reservation`]. `INSERT OR IGNORE` followed by a read
//! is one storage operation, so it is one function here; but *which* prior row
//! came back is the fact the conflict rule turns on, and the two prior-row cases
//! carry different diagnostics upstairs. So this module names the case it took
//! rather than deciding what it means.
//!
//! The statements are the ones that ran in `core::action_audit` before FR-130
//! B8, transcribed rather than rewritten — including the repeated column lists,
//! so the move can be read as a move.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::async_database::{AsyncDatabase, flatten_err};

/// A row to be written into `control_action_audit`.
///
/// Every field is a column. `request_hash` arrives already computed: how a
/// canonical request is reduced to a hash is the caller's contract, not this
/// table's.
#[derive(Debug, Clone)]
pub struct NewActionAudit {
    /// Request identifier, the table's primary key.
    pub request_id: String,
    /// Envelope schema version.
    pub schema_version: i64,
    /// Project isolation scope.
    pub project_id: String,
    /// Trusted actor identity when authentication succeeded.
    pub actor: Option<String>,
    /// Locally resolved role.
    pub resolved_role: Option<String>,
    /// Request transport.
    pub transport: String,
    /// Target kind.
    pub target_type: String,
    /// Target identifier.
    pub target_id: String,
    /// Closed action identifier.
    pub action: String,
    /// Machine-readable reason code.
    pub reason_code: String,
    /// Optional bounded operator explanation.
    pub operator_reason: Option<String>,
    /// Retry identity when the action has one.
    pub idempotency_key: Option<String>,
    /// Expected optimistic state version.
    pub expected_version: Option<String>,
    /// Lease fencing token when applicable.
    pub fencing_token: Option<i64>,
    /// Hash over the allowlisted canonical inputs, computed by the caller.
    pub request_hash: String,
}

/// Durable canonical action audit envelope, one row of `control_action_audit`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionAuditRecord {
    /// Request identifier joining transport, domain and event evidence.
    pub request_id: String,
    /// Envelope schema version.
    pub schema_version: i64,
    /// Project isolation scope.
    pub project_id: String,
    /// Trusted actor identity when authentication succeeded.
    pub actor: Option<String>,
    /// Locally resolved role.
    pub resolved_role: Option<String>,
    /// Request transport.
    pub transport: String,
    /// Target kind.
    pub target_type: String,
    /// Target identifier.
    pub target_id: String,
    /// Closed action identifier.
    pub action: String,
    /// Machine-readable reason code.
    pub reason_code: String,
    /// Optional bounded operator explanation.
    pub operator_reason: Option<String>,
    /// Retry identity when applicable.
    pub idempotency_key: Option<String>,
    /// Expected optimistic state version.
    pub expected_version: Option<String>,
    /// Lease fencing token when applicable.
    pub fencing_token: Option<i64>,
    /// SHA-256 over allowlisted canonical inputs.
    pub request_hash: String,
    /// Lifecycle status (`reserved`, `succeeded`, `failed`, or `denied`).
    pub status: String,
    /// Stable terminal error code.
    pub error_code: Option<String>,
    /// Result reference kind.
    pub result_type: Option<String>,
    /// Result reference identifier.
    pub result_id: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Terminal timestamp.
    pub completed_at: Option<String>,
}

/// Project-scoped list filters for canonical action audit records.
#[derive(Debug, Clone, Default)]
pub struct ActionAuditFilter {
    /// Required project isolation scope.
    pub project_id: String,
    /// Optional actor filter.
    pub actor: Option<String>,
    /// Optional target kind filter.
    pub target_type: Option<String>,
    /// Optional target identifier filter.
    pub target_id: Option<String>,
    /// Optional action filter.
    pub action: Option<String>,
    /// Optional status filter.
    pub status: Option<String>,
    /// Inclusive lower timestamp bound.
    pub from_time: Option<String>,
    /// Exclusive upper timestamp bound.
    pub to_time: Option<String>,
    /// Maximum row count.
    pub limit: usize,
}

/// Which row [`reserve`] ended up holding, and how it found it.
///
/// The caller compares hashes and decides whether a prior row is a legitimate
/// retry or a conflict; the two prior-row cases stay distinct because they mean
/// different things and produce different diagnostics.
#[derive(Debug, Clone)]
pub enum Reservation {
    /// The insert claimed the row — this caller owns the side effect.
    Claimed(ActionAuditRecord),
    /// A prior row was already registered under the same retry identity.
    PriorByRetryIdentity(ActionAuditRecord),
    /// The request id was already used, and no retry identity was supplied.
    PriorByRequestId(ActionAuditRecord),
}

impl Reservation {
    /// Consumes the outcome and yields the row, whichever case produced it.
    pub fn into_record(self) -> ActionAuditRecord {
        match self {
            Self::Claimed(record)
            | Self::PriorByRetryIdentity(record)
            | Self::PriorByRequestId(record) => record,
        }
    }
}

/// The largest number of rows [`list`] will return, whatever the filter asks for.
pub const MAX_LIST_ROWS: usize = 500;

/// Reserves an envelope, or reports the prior row that already holds its slot.
pub async fn reserve(db: &AsyncDatabase, new: NewActionAudit) -> Result<Reservation> {
    db.writer()
        .call(move |conn| {
            reserve_blocking(conn, &new).map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Inserts a row that is terminal on arrival, under its own request identifier.
pub async fn insert_terminal(
    db: &AsyncDatabase,
    new: NewActionAudit,
    status: String,
    error_code: String,
) -> Result<ActionAuditRecord> {
    db.writer()
        .call(move |conn| {
            insert_terminal_blocking(conn, &new, &status, &error_code)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Marks a reserved envelope terminal with an allowlisted result reference.
///
/// Reads the row back unchanged when nothing matched, so a caller completing an
/// already-terminal envelope sees the terminal row rather than an error.
pub async fn complete(
    db: &AsyncDatabase,
    request_id: String,
    status: String,
    error_code: Option<String>,
    result_type: Option<String>,
    result_id: Option<String>,
) -> Result<ActionAuditRecord> {
    db.writer()
        .call(move |conn| {
            complete_blocking(
                conn,
                &request_id,
                &status,
                error_code.as_deref(),
                result_type.as_deref(),
                result_id.as_deref(),
            )
            .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one envelope within its project scope.
pub async fn get(
    db: &AsyncDatabase,
    project_id: String,
    request_id: String,
) -> Result<Option<ActionAuditRecord>> {
    db.reader()
        .call(move |conn| {
            read_scoped(conn, &project_id, &request_id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Lists envelopes using bounded, project-scoped filters.
pub async fn list(db: &AsyncDatabase, filter: ActionAuditFilter) -> Result<Vec<ActionAuditRecord>> {
    db.reader()
        .call(move |conn| {
            list_blocking(conn, &filter).map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

fn reserve_blocking(conn: &Connection, new: &NewActionAudit) -> Result<Reservation> {
    let now = crate::now_ts();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO control_action_audit
         (request_id,schema_version,project_id,actor,resolved_role,transport,target_type,target_id,
          action,reason_code,operator_reason,idempotency_key,expected_version,fencing_token,
          request_hash,status,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'reserved',?16,?16)",
        params![
            new.request_id,
            new.schema_version,
            new.project_id,
            new.actor,
            new.resolved_role,
            new.transport,
            new.target_type,
            new.target_id,
            new.action,
            new.reason_code,
            new.operator_reason,
            new.idempotency_key,
            new.expected_version,
            new.fencing_token,
            new.request_hash,
            now,
        ],
    )?;
    if inserted == 1 {
        return Ok(Reservation::Claimed(
            read_by_request_id(conn, &new.request_id)?.context("reserved action audit missing")?,
        ));
    }
    // The row was already there. A retry identity is the stronger key: it can
    // match a row written under a *different* request id, which is exactly what
    // a client retry looks like.
    if let Some(key) = new.idempotency_key.as_deref() {
        let existing = read_by_retry_identity(
            conn,
            &new.project_id,
            &new.target_type,
            &new.target_id,
            &new.action,
            key,
        )?
        .context("action audit reservation conflict without existing row")?;
        return Ok(Reservation::PriorByRetryIdentity(existing));
    }
    let existing = read_by_request_id(conn, &new.request_id)?
        .context("request_id was reused without a matching action audit")?;
    Ok(Reservation::PriorByRequestId(existing))
}

fn insert_terminal_blocking(
    conn: &Connection,
    new: &NewActionAudit,
    status: &str,
    error_code: &str,
) -> Result<ActionAuditRecord> {
    let now = crate::now_ts();
    conn.execute(
        "INSERT INTO control_action_audit
         (request_id,schema_version,project_id,actor,resolved_role,transport,target_type,target_id,
          action,reason_code,operator_reason,idempotency_key,expected_version,fencing_token,
          request_hash,status,error_code,created_at,updated_at,completed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?18)",
        params![
            new.request_id,
            new.schema_version,
            new.project_id,
            new.actor,
            new.resolved_role,
            new.transport,
            new.target_type,
            new.target_id,
            new.action,
            new.reason_code,
            new.operator_reason,
            new.idempotency_key,
            new.expected_version,
            new.fencing_token,
            new.request_hash,
            status,
            error_code,
            now,
        ],
    )?;
    read_by_request_id(conn, &new.request_id)?.context("terminal action audit missing")
}

fn complete_blocking(
    conn: &Connection,
    request_id: &str,
    status: &str,
    error_code: Option<&str>,
    result_type: Option<&str>,
    result_id: Option<&str>,
) -> Result<ActionAuditRecord> {
    let now = crate::now_ts();
    let changed = conn.execute(
        "UPDATE control_action_audit SET status=?2,error_code=?3,result_type=?4,result_id=?5,
         updated_at=?6,completed_at=?6 WHERE request_id=?1 AND status='reserved'",
        params![request_id, status, error_code, result_type, result_id, now],
    )?;
    if changed == 0 {
        return read_by_request_id(conn, request_id)?.context("action audit record not found");
    }
    read_by_request_id(conn, request_id)?.context("completed action audit missing")
}

fn read_scoped(
    conn: &Connection,
    project_id: &str,
    request_id: &str,
) -> Result<Option<ActionAuditRecord>> {
    conn.query_row(
        "SELECT request_id,schema_version,project_id,actor,resolved_role,transport,target_type,
                target_id,action,reason_code,operator_reason,idempotency_key,expected_version,
                fencing_token,request_hash,status,error_code,result_type,result_id,created_at,
                updated_at,completed_at
         FROM control_action_audit WHERE project_id=?1 AND request_id=?2",
        params![project_id, request_id],
        map_record,
    )
    .optional()
    .map_err(Into::into)
}

fn read_by_request_id(conn: &Connection, request_id: &str) -> Result<Option<ActionAuditRecord>> {
    conn.query_row(
        "SELECT request_id,schema_version,project_id,actor,resolved_role,transport,target_type,
                target_id,action,reason_code,operator_reason,idempotency_key,expected_version,
                fencing_token,request_hash,status,error_code,result_type,result_id,created_at,
                updated_at,completed_at
         FROM control_action_audit WHERE request_id=?1",
        [request_id],
        map_record,
    )
    .optional()
    .map_err(Into::into)
}

fn read_by_retry_identity(
    conn: &Connection,
    project_id: &str,
    target_type: &str,
    target_id: &str,
    action: &str,
    key: &str,
) -> Result<Option<ActionAuditRecord>> {
    conn.query_row(
        "SELECT request_id,schema_version,project_id,actor,resolved_role,transport,target_type,
                target_id,action,reason_code,operator_reason,idempotency_key,expected_version,
                fencing_token,request_hash,status,error_code,result_type,result_id,created_at,
                updated_at,completed_at
         FROM control_action_audit
         WHERE project_id=?1 AND target_type=?2 AND target_id=?3 AND action=?4
           AND idempotency_key=?5 AND status IN ('reserved','succeeded')",
        params![project_id, target_type, target_id, action, key],
        map_record,
    )
    .optional()
    .map_err(Into::into)
}

fn list_blocking(conn: &Connection, filter: &ActionAuditFilter) -> Result<Vec<ActionAuditRecord>> {
    let limit = filter.limit.clamp(1, MAX_LIST_ROWS) as i64;
    let mut statement = conn.prepare(
        "SELECT request_id,schema_version,project_id,actor,resolved_role,transport,target_type,
                target_id,action,reason_code,operator_reason,idempotency_key,expected_version,
                fencing_token,request_hash,status,error_code,result_type,result_id,created_at,
                updated_at,completed_at
         FROM control_action_audit
         WHERE project_id=?1
           AND (?2 IS NULL OR actor=?2)
           AND (?3 IS NULL OR target_type=?3)
           AND (?4 IS NULL OR target_id=?4)
           AND (?5 IS NULL OR action=?5)
           AND (?6 IS NULL OR status=?6)
           AND (?7 IS NULL OR created_at>=?7)
           AND (?8 IS NULL OR created_at<?8)
         ORDER BY created_at DESC, request_id DESC LIMIT ?9",
    )?;
    let rows = statement.query_map(
        params![
            filter.project_id,
            filter.actor,
            filter.target_type,
            filter.target_id,
            filter.action,
            filter.status,
            filter.from_time,
            filter.to_time,
            limit,
        ],
        map_record,
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionAuditRecord> {
    Ok(ActionAuditRecord {
        request_id: row.get(0)?,
        schema_version: row.get(1)?,
        project_id: row.get(2)?,
        actor: row.get(3)?,
        resolved_role: row.get(4)?,
        transport: row.get(5)?,
        target_type: row.get(6)?,
        target_id: row.get(7)?,
        action: row.get(8)?,
        reason_code: row.get(9)?,
        operator_reason: row.get(10)?,
        idempotency_key: row.get(11)?,
        expected_version: row.get(12)?,
        fencing_token: row.get(13)?,
        request_hash: row.get(14)?,
        status: row.get(15)?,
        error_code: row.get(16)?,
        result_type: row.get(17)?,
        result_id: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        completed_at: row.get(21)?,
    })
}
