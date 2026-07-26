//! Canonical, bounded audit records for state-changing control-plane actions.
//!
//! The contract lives here: which fields are bounded and how, how a canonical
//! request is reduced to a hash, which lifecycle statuses a caller may ask for,
//! and what it means when a retry identity comes back attached to a different
//! request. The table those decisions are written to lives in
//! `orchestrator_persistence::control_action_audit` (FR-130 B8), which reports
//! *which* row it ended up holding and leaves the meaning to this module.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::async_database::AsyncDatabase;
use orchestrator_persistence::control_action_audit::{self as store, NewActionAudit, Reservation};

pub use orchestrator_persistence::control_action_audit::{ActionAuditFilter, ActionAuditRecord};

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
    ///
    /// The store reports which row it ended up holding; the rule that a prior
    /// row with a different `request_hash` is a conflict rather than a retry is
    /// applied here, because it is a control-plane contract and not a property
    /// of the table.
    pub async fn reserve(
        &self,
        input: ActionAuditReservation,
    ) -> Result<ActionAuditReservationResult> {
        validate(&input)?;
        let request_hash = canonical_request_hash(&input.canonical_request)?;
        let outcome = store::reserve(&self.db, new_audit(input, request_hash.clone())).await?;
        let (should_execute, record) = match outcome {
            Reservation::Claimed(record) => (true, record),
            Reservation::PriorByRetryIdentity(record) => {
                if record.request_hash != request_hash {
                    bail!("idempotency key was reused with a different canonical request");
                }
                (false, record)
            }
            Reservation::PriorByRequestId(record) => {
                if record.request_hash != request_hash {
                    bail!("request_id was reused with a different canonical request");
                }
                (false, record)
            }
        };
        Ok(ActionAuditReservationResult {
            record,
            should_execute,
        })
    }

    /// Persists an authorization denial without allowing its retry identity to block a later
    /// authorized attempt.
    pub async fn deny(
        &self,
        input: ActionAuditReservation,
        error_code: &str,
    ) -> Result<ActionAuditRecord> {
        self.insert_terminal(input, "denied", error_code).await
    }

    /// Persists a failed pre-mutation attempt, including idempotency conflicts, under its own
    /// request identifier.
    pub async fn fail_attempt(
        &self,
        input: ActionAuditReservation,
        error_code: &str,
    ) -> Result<ActionAuditRecord> {
        self.insert_terminal(input, "failed", error_code).await
    }

    async fn insert_terminal(
        &self,
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
        store::insert_terminal(
            &self.db,
            new_audit(input, request_hash),
            status.to_owned(),
            error_code.to_owned(),
        )
        .await
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
        if !matches!(status, "succeeded" | "failed" | "denied") {
            bail!("invalid terminal action audit status");
        }
        store::complete(
            &self.db,
            request_id.to_owned(),
            status.to_owned(),
            error_code.map(str::to_owned),
            result_type.map(str::to_owned),
            result_id.map(str::to_owned),
        )
        .await
    }

    /// Gets one envelope within its project scope.
    pub async fn get(
        &self,
        project_id: &str,
        request_id: &str,
    ) -> Result<Option<ActionAuditRecord>> {
        store::get(&self.db, project_id.to_owned(), request_id.to_owned()).await
    }

    /// Lists envelopes using bounded, project-scoped filters.
    pub async fn list(&self, filter: ActionAuditFilter) -> Result<Vec<ActionAuditRecord>> {
        if filter.project_id.trim().is_empty() {
            bail!("project_id is required");
        }
        store::list(&self.db, filter).await
    }
}

/// Projects a validated reservation and its already-computed hash onto the row
/// the store writes. `canonical_request` is deliberately not carried across:
/// only its hash is durable.
fn new_audit(input: ActionAuditReservation, request_hash: String) -> NewActionAudit {
    NewActionAudit {
        request_id: input.request_id,
        schema_version: SCHEMA_VERSION,
        project_id: input.project_id,
        actor: input.actor,
        resolved_role: input.resolved_role,
        transport: input.transport,
        target_type: input.target_type,
        target_id: input.target_id,
        action: input.action,
        reason_code: input.reason_code,
        operator_reason: input.operator_reason,
        idempotency_key: input.idempotency_key,
        expected_version: input.expected_version,
        fencing_token: input.fencing_token,
        request_hash,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_database::AsyncDatabase;
    use crate::db::configure_conn;
    // Inside the test module on purpose: the boundary scanner strips `cfg(test)`
    // blocks, and a file-scope import would count this fixture as production use.
    use crate::persistence::migration::{registered_migrations as all_migrations, run_pending};
    use rusqlite::Connection;
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
