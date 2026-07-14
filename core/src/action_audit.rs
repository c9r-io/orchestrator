//! Canonical, bounded audit records for state-changing control-plane actions.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;

const SCHEMA_VERSION: i64 = 1;
const MAX_REASON_BYTES: usize = 500;

/// Input used to reserve one canonical action audit record.
#[derive(Debug, Clone)]
pub struct ActionAuditReservation {
    /// Daemon-issued or validated propagated request identifier.
    pub request_id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Trusted actor identity, absent only when authentication failed.
    pub actor: Option<String>,
    /// Locally resolved role, absent only when authorization did not resolve one.
    pub resolved_role: Option<String>,
    /// Transport label (`uds`, `tcp`, or provider-specific adapter label).
    pub transport: String,
    /// Closed target kind.
    pub target_type: String,
    /// Stable target identifier.
    pub target_id: String,
    /// Closed action identifier.
    pub action: String,
    /// Machine-readable reason code.
    pub reason_code: String,
    /// Optional bounded operator explanation.
    pub operator_reason: Option<String>,
    /// Optional retry identity. Business mutations supply one; renewable leases may omit it.
    pub idempotency_key: Option<String>,
    /// Optional optimistic state version.
    pub expected_version: Option<String>,
    /// Optional lease fencing token.
    pub fencing_token: Option<i64>,
    /// Canonical, non-secret request shape used for conflict detection.
    pub canonical_request: Value,
}

/// Durable canonical action audit envelope returned by read APIs.
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

/// Result of reserving an action audit envelope.
#[derive(Debug, Clone)]
pub struct ActionAuditReservationResult {
    /// Durable envelope.
    pub record: ActionAuditRecord,
    /// Whether the caller owns the first reservation and may perform side effects.
    pub should_execute: bool,
}

/// Async repository backed by the daemon's shared SQLite writer.
#[derive(Clone)]
pub struct AsyncActionAuditRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncActionAuditRepository {
    /// Creates an action-audit repository.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Reserves an envelope or returns a matching prior reservation.
    pub async fn reserve(
        &self,
        input: ActionAuditReservation,
    ) -> Result<ActionAuditReservationResult> {
        self.db
            .writer()
            .call(move |conn| {
                reserve(conn, input).map_err(|error| tokio_rusqlite::Error::Other(error.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists an authorization denial without allowing its retry identity to block a later
    /// authorized attempt.
    pub async fn deny(
        &self,
        input: ActionAuditReservation,
        error_code: &str,
    ) -> Result<ActionAuditRecord> {
        let error_code = error_code.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                insert_terminal(conn, input, "denied", &error_code)
                    .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists a failed pre-mutation attempt, including idempotency conflicts, under its own
    /// request identifier.
    pub async fn fail_attempt(
        &self,
        input: ActionAuditReservation,
        error_code: &str,
    ) -> Result<ActionAuditRecord> {
        let error_code = error_code.to_owned();
        self.db
            .writer()
            .call(move |conn| {
                insert_terminal(conn, input, "failed", &error_code)
                    .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Marks an envelope terminal with an allowlisted result reference.
    pub async fn complete(
        &self,
        request_id: &str,
        status: &str,
        error_code: Option<&str>,
        result_type: Option<&str>,
        result_id: Option<&str>,
    ) -> Result<ActionAuditRecord> {
        let request_id = request_id.to_owned();
        let status = status.to_owned();
        let error_code = error_code.map(str::to_owned);
        let result_type = result_type.map(str::to_owned);
        let result_id = result_id.map(str::to_owned);
        self.db
            .writer()
            .call(move |conn| {
                complete(
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

    /// Gets one envelope within its project scope.
    pub async fn get(
        &self,
        project_id: &str,
        request_id: &str,
    ) -> Result<Option<ActionAuditRecord>> {
        let project_id = project_id.to_owned();
        let request_id = request_id.to_owned();
        self.db
            .reader()
            .call(move |conn| {
                read(conn, &project_id, &request_id)
                    .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists envelopes using bounded, project-scoped filters.
    pub async fn list(&self, filter: ActionAuditFilter) -> Result<Vec<ActionAuditRecord>> {
        self.db
            .reader()
            .call(move |conn| {
                list(conn, &filter).map_err(|error| tokio_rusqlite::Error::Other(error.into()))
            })
            .await
            .map_err(flatten_err)
    }
}

/// Computes a deterministic hash after recursively sorting object keys.
pub fn canonical_request_hash(value: &Value) -> Result<String> {
    let normalized = canonicalize(value);
    let encoded = serde_json::to_vec(&normalized).context("serialize canonical action request")?;
    Ok(Sha256::digest(encoded)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        _ => value.clone(),
    }
}

fn validate(input: &ActionAuditReservation) -> Result<()> {
    for (name, value, max) in [
        ("request_id", input.request_id.as_str(), 128),
        ("project_id", input.project_id.as_str(), 128),
        ("transport", input.transport.as_str(), 32),
        ("target_type", input.target_type.as_str(), 64),
        ("target_id", input.target_id.as_str(), 256),
        ("action", input.action.as_str(), 128),
        ("reason_code", input.reason_code.as_str(), 64),
    ] {
        if value.trim().is_empty() || value.len() > max {
            bail!("{name} must contain 1-{max} characters");
        }
    }
    if input
        .operator_reason
        .as_ref()
        .is_some_and(|reason| reason.len() > MAX_REASON_BYTES)
    {
        bail!("operator_reason exceeds {MAX_REASON_BYTES} bytes");
    }
    if input
        .idempotency_key
        .as_ref()
        .is_some_and(|key| key.trim().is_empty() || key.len() > 128)
    {
        bail!("idempotency_key must contain 1-128 characters");
    }
    Ok(())
}

fn reserve(
    conn: &Connection,
    input: ActionAuditReservation,
) -> Result<ActionAuditReservationResult> {
    validate(&input)?;
    let request_hash = canonical_request_hash(&input.canonical_request)?;
    let now = now_ts();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO control_action_audit
         (request_id,schema_version,project_id,actor,resolved_role,transport,target_type,target_id,
          action,reason_code,operator_reason,idempotency_key,expected_version,fencing_token,
          request_hash,status,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'reserved',?16,?16)",
        params![
            input.request_id,
            SCHEMA_VERSION,
            input.project_id,
            input.actor,
            input.resolved_role,
            input.transport,
            input.target_type,
            input.target_id,
            input.action,
            input.reason_code,
            input.operator_reason,
            input.idempotency_key,
            input.expected_version,
            input.fencing_token,
            request_hash,
            now,
        ],
    )?;
    let record = if inserted == 1 {
        read_by_request_id(conn, &input.request_id)?.context("reserved action audit missing")?
    } else if let Some(key) = input.idempotency_key.as_deref() {
        let existing = read_by_retry_identity(
            conn,
            &input.project_id,
            &input.target_type,
            &input.target_id,
            &input.action,
            key,
        )?
        .context("action audit reservation conflict without existing row")?;
        if existing.request_hash != request_hash {
            bail!("idempotency key was reused with a different canonical request");
        }
        existing
    } else {
        let existing = read_by_request_id(conn, &input.request_id)?
            .context("request_id was reused without a matching action audit")?;
        if existing.request_hash != request_hash {
            bail!("request_id was reused with a different canonical request");
        }
        existing
    };
    Ok(ActionAuditReservationResult {
        should_execute: inserted == 1,
        record,
    })
}

fn insert_terminal(
    conn: &Connection,
    input: ActionAuditReservation,
    status: &str,
    error_code: &str,
) -> Result<ActionAuditRecord> {
    validate(&input)?;
    if !matches!(status, "failed" | "denied") {
        bail!("invalid direct terminal action audit status");
    }
    if error_code.trim().is_empty() || error_code.len() > 64 {
        bail!("error_code must contain 1-64 characters");
    }
    let request_hash = canonical_request_hash(&input.canonical_request)?;
    let now = now_ts();
    conn.execute(
        "INSERT INTO control_action_audit
         (request_id,schema_version,project_id,actor,resolved_role,transport,target_type,target_id,
          action,reason_code,operator_reason,idempotency_key,expected_version,fencing_token,
          request_hash,status,error_code,created_at,updated_at,completed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?18)",
        params![
            input.request_id,
            SCHEMA_VERSION,
            input.project_id,
            input.actor,
            input.resolved_role,
            input.transport,
            input.target_type,
            input.target_id,
            input.action,
            input.reason_code,
            input.operator_reason,
            input.idempotency_key,
            input.expected_version,
            input.fencing_token,
            request_hash,
            status,
            error_code,
            now,
        ],
    )?;
    read_by_request_id(conn, &input.request_id)?.context("terminal action audit missing")
}

fn complete(
    conn: &Connection,
    request_id: &str,
    status: &str,
    error_code: Option<&str>,
    result_type: Option<&str>,
    result_id: Option<&str>,
) -> Result<ActionAuditRecord> {
    if !matches!(status, "succeeded" | "failed" | "denied") {
        bail!("invalid terminal action audit status");
    }
    let now = now_ts();
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

fn read(
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

fn list(conn: &Connection, filter: &ActionAuditFilter) -> Result<Vec<ActionAuditRecord>> {
    if filter.project_id.trim().is_empty() {
        bail!("project_id is required");
    }
    let limit = filter.limit.clamp(1, 500) as i64;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use crate::db::configure_conn;
    use crate::migration::{all_migrations, run_pending};
    use tempfile::tempdir;

    async fn repository() -> (tempfile::TempDir, AsyncActionAuditRepository) {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("audit.db");
        let conn = Connection::open(&path).expect("open");
        configure_conn(&conn).expect("configure");
        run_pending(&conn, &all_migrations()).expect("migrate");
        drop(conn);
        let db = AsyncDatabase::open(&path).await.expect("async db");
        (directory, AsyncActionAuditRepository::new(Arc::new(db)))
    }

    fn input(request_id: &str, key: &str, value: Value) -> ActionAuditReservation {
        ActionAuditReservation {
            request_id: request_id.into(),
            project_id: "default".into(),
            actor: Some("uid:501".into()),
            resolved_role: Some("operator".into()),
            transport: "uds".into(),
            target_type: "attention_item".into(),
            target_id: "attn-1".into(),
            action: "attention.claim".into(),
            reason_code: "operator_triage".into(),
            operator_reason: None,
            idempotency_key: Some(key.into()),
            expected_version: Some("1".into()),
            fencing_token: None,
            canonical_request: value,
        }
    }

    #[test]
    fn canonical_hash_ignores_object_key_order() {
        let left = serde_json::json!({"b":2,"a":{"d":4,"c":3}});
        let right = serde_json::json!({"a":{"c":3,"d":4},"b":2});
        assert_eq!(
            canonical_request_hash(&left).expect("left"),
            canonical_request_hash(&right).expect("right")
        );
    }

    #[tokio::test]
    async fn matching_retry_returns_original_without_second_execution() {
        let (_directory, repository) = repository().await;
        let first = repository
            .reserve(input("req-1", "retry-1", serde_json::json!({"version":1})))
            .await
            .expect("reserve first");
        let duplicate = repository
            .reserve(input("req-2", "retry-1", serde_json::json!({"version":1})))
            .await
            .expect("reserve duplicate");
        assert!(first.should_execute);
        assert!(!duplicate.should_execute);
        assert_eq!(duplicate.record.request_id, "req-1");
    }

    #[tokio::test]
    async fn changed_retry_fails_closed() {
        let (_directory, repository) = repository().await;
        repository
            .reserve(input("req-1", "retry-1", serde_json::json!({"version":1})))
            .await
            .expect("reserve first");
        let error = repository
            .reserve(input("req-2", "retry-1", serde_json::json!({"version":2})))
            .await
            .expect_err("conflict");
        assert!(error.to_string().contains("different canonical request"));
        repository
            .fail_attempt(
                input("req-2", "retry-1", serde_json::json!({"version":2})),
                "idempotency_conflict",
            )
            .await
            .expect("record conflict");
        let conflict = repository
            .get("default", "req-2")
            .await
            .expect("get conflict")
            .expect("conflict row");
        assert_eq!(conflict.status, "failed");
    }

    #[tokio::test]
    async fn project_scoped_query_returns_bounded_envelope() {
        let (_directory, repository) = repository().await;
        repository
            .reserve(input("req-1", "retry-1", serde_json::json!({"version":1})))
            .await
            .expect("reserve");
        repository
            .complete(
                "req-1",
                "succeeded",
                None,
                Some("attention_action"),
                Some("attn-1"),
            )
            .await
            .expect("complete");
        let rows = repository
            .list(ActionAuditFilter {
                project_id: "default".into(),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "succeeded");
        assert_eq!(rows[0].result_id.as_deref(), Some("attn-1"));
    }

    #[tokio::test]
    async fn concurrent_matching_retry_has_one_execution_owner() {
        let (_directory, repository) = repository().await;
        let left = repository.reserve(input(
            "req-concurrent-1",
            "retry-concurrent",
            serde_json::json!({"version": 1}),
        ));
        let right = repository.reserve(input(
            "req-concurrent-2",
            "retry-concurrent",
            serde_json::json!({"version": 1}),
        ));
        let (left, right) = tokio::join!(left, right);
        let owners = [left.expect("left"), right.expect("right")]
            .into_iter()
            .filter(|result| result.should_execute)
            .count();
        assert_eq!(owners, 1);
    }

    #[tokio::test]
    async fn stored_envelope_contains_hash_not_request_body() {
        let (_directory, repository) = repository().await;
        let secret = "raw-terminal-input-must-not-survive";
        let result = repository
            .reserve(input(
                "req-redacted",
                "retry-redacted",
                serde_json::json!({"input_sha256": canonical_request_hash(&serde_json::json!(secret)).expect("hash")}),
            ))
            .await
            .expect("reserve");
        let encoded = serde_json::to_string(&result.record).expect("serialize record");
        assert!(!encoded.contains(secret));
        assert!(!encoded.contains("canonical_request"));
    }

    #[tokio::test]
    async fn denied_retry_identity_does_not_block_later_authorized_attempt() {
        let (_directory, repository) = repository().await;
        repository
            .deny(
                input(
                    "req-denied",
                    "retry-shared",
                    serde_json::json!({"version": 1}),
                ),
                "authorization_denied",
            )
            .await
            .expect("record denial");
        let authorized = repository
            .reserve(input(
                "req-authorized",
                "retry-shared",
                serde_json::json!({"version": 1}),
            ))
            .await
            .expect("authorized reservation");
        assert!(authorized.should_execute);
    }
}
