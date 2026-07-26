//! The SourceConnection tables: `source_connections`, `source_connection_intents`,
//! `source_connection_provisioning`, `source_connection_changes` and
//! `source_daemon_identity`.
//!
//! `core::source_connection` owns the contract: which fields are bounded and by
//! how much, which provisioning modes may hold managed OAuth intents, which
//! terminal statuses a caller may ask for, and how a stored mode/state string
//! and the capability and scope JSON become typed values. This module owns the
//! statements and the transactions.
//!
//! Rows cross the boundary with their enums and JSON **unparsed** — `state`,
//! `provisioning_mode`, `capabilities_json` and `scopes_json` are `String`.
//! That is deliberate: the parse belongs to whoever owns the types, and keeping
//! it above the row mapper means a malformed value is an `anyhow` error naming
//! the connection rather than a fabricated column-conversion failure (FR-130
//! B13).
//!
//! Every operation whose `UPDATE` carries a fence reports whether the fence
//! held, and says nothing about what that means. There are three separate
//! copies of the `version=?3` optimistic fence, four of `state='active'`, and
//! two of `owner_daemon_id=?3`; each is a distinct statement and each needs its
//! own assertion.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::async_database::{AsyncDatabase, flatten_err};

/// Longest list any read here will return, whatever the caller asks for.
pub const MAX_LIST_ROWS: usize = 500;

/// One `source_connections` row, with its enums and JSON still unparsed.
#[derive(Debug, Clone)]
pub struct SourceConnectionRow {
    /// Connection identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Operator-facing label.
    pub display_label: String,
    /// Provisioning mode, as stored.
    pub provisioning_mode: String,
    /// Provider installation identifier.
    pub installation_id: String,
    /// Digest of the installation identifier.
    pub installation_id_digest: String,
    /// Digest of the enterprise identifier, when there is one.
    pub enterprise_id_digest: Option<String>,
    /// Daemon that owns delivery for this connection.
    pub owner_daemon_id: String,
    /// Credential generation.
    pub generation: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Lifecycle state, as stored.
    pub state: String,
    /// Granted capabilities, as stored JSON.
    pub capabilities_json: String,
    /// Granted scopes, as stored JSON.
    pub scopes_json: String,
    /// Trigger name bound to the connection, when there is one.
    pub trigger_name: Option<String>,
    /// Timestamp of the last accepted delivery.
    pub last_delivery_at: Option<String>,
    /// Highest delivery cursor acknowledged so far.
    pub last_acked_cursor: i64,
    /// Deliveries outstanding behind the cursor.
    pub delivery_lag: i64,
    /// Last recorded error code.
    pub last_error_code: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Timestamp of the last reauthorization.
    pub reauthorized_at: Option<String>,
    /// Timestamp of disconnection.
    pub disconnected_at: Option<String>,
    /// Who owns the underlying provider App.
    pub app_ownership: String,
    /// Digest of the provider App identifier.
    pub app_id_digest: Option<String>,
    /// Reviewed manifest version in force.
    pub manifest_version: Option<String>,
    /// Dedicated-App provisioning state.
    pub provision_state: Option<String>,
    /// Dedicated-App provisioning error code.
    pub provision_error_code: Option<String>,
}

/// Everything an activation writes, already validated by the caller.
#[derive(Debug, Clone)]
pub struct NewActivation {
    /// Connection identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Operator-facing label.
    pub display_label: String,
    /// Provisioning mode, already rendered to its stored form.
    pub provisioning_mode: String,
    /// Provider installation identifier.
    pub installation_id: String,
    /// Digest of the installation identifier.
    pub installation_id_digest: String,
    /// Digest of the enterprise identifier, when there is one.
    pub enterprise_id_digest: Option<String>,
    /// Daemon claiming delivery ownership.
    pub owner_daemon_id: String,
    /// Credential generation.
    pub generation: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Granted capabilities, already serialized.
    pub capabilities_json: String,
    /// Granted scopes, already serialized.
    pub scopes_json: String,
    /// Trigger name to bind, when there is one.
    pub trigger_name: Option<String>,
    /// Gateway origin for managed connections.
    pub gateway_origin: Option<String>,
    /// Encrypted Gateway pairing material.
    pub pairing_secret_ciphertext: Option<String>,
    /// Delivery cursor to adopt, never lowered.
    pub last_acked_cursor: i64,
    /// Who owns the underlying provider App.
    pub app_ownership: String,
    /// Digest of the provider App identifier.
    pub app_id_digest: Option<String>,
    /// Reviewed manifest version.
    pub manifest_version: Option<String>,
    /// Dedicated-App provisioning state.
    pub provision_state: Option<String>,
    /// Dedicated-App provisioning error code.
    pub provision_error_code: Option<String>,
    /// Correlating request identifier, recorded in the change log.
    pub request_id: String,
}

/// What [`activate`] did, or why it declined.
///
/// The three refusals are separate because they are three different operator
/// problems; the caller names them.
#[derive(Debug, Clone)]
pub enum Activation {
    /// No live installation existed; a new connection was inserted.
    Created(SourceConnectionRow),
    /// An existing installation was reauthorized in place.
    Reauthorized(SourceConnectionRow),
    /// A live row exists for this installation under a different project,
    /// owner, or connection id.
    OwnerConflict,
    /// The offered generation or version is older than what is stored.
    StaleFence,
    /// The fenced `UPDATE` matched nothing, or the row was disconnected.
    ReauthorizationConflict,
}

/// Encrypted adapter credentials for one active, owned connection.
#[derive(Debug, Clone)]
pub struct ConnectionCredentialRow {
    /// Provider installation identifier.
    pub installation_id: String,
    /// Owning daemon.
    pub owner_daemon_id: String,
    /// Credential generation.
    pub generation: i64,
    /// Gateway origin. Read as non-null: the fence upstream releases credentials
    /// only for an active managed connection, which always has one.
    pub gateway_origin: String,
    /// Encrypted Gateway pairing material, non-null for the same reason.
    pub pairing_secret_ciphertext: String,
}

/// One `source_connection_intents` row, mode unparsed.
#[derive(Debug, Clone)]
pub struct IntentRow {
    /// Intent identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Provisioning mode, as stored.
    pub provisioning_mode: String,
    /// Lifecycle status.
    pub status: String,
    /// Connection the intent resolved to, once it did.
    pub connection_id: Option<String>,
    /// Terminal error code.
    pub error_code: Option<String>,
    /// Expiry timestamp.
    pub expires_at: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// The encrypted half of an intent, released only inside its owner fence.
#[derive(Debug, Clone)]
pub struct IntentCredentialRow {
    /// Gateway-side intent identifier.
    pub gateway_intent_id: String,
    /// Encrypted authorize URL.
    pub authorize_url_ciphertext: String,
    /// Encrypted polling secret.
    pub poll_secret_ciphertext: String,
    /// Owning daemon.
    pub owner_daemon_id: String,
    /// Operator-facing label.
    pub display_label: String,
}

/// A resumable OAuth intent to be recorded.
#[derive(Debug, Clone)]
pub struct NewIntent {
    /// Intent identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Provider label.
    pub provider: String,
    /// Operator-facing label.
    pub display_label: String,
    /// Provisioning mode, already rendered to its stored form.
    pub provisioning_mode: String,
    /// Daemon that will own the resulting connection.
    pub owner_daemon_id: String,
    /// Digest identifying the initiating operator.
    pub actor_digest: String,
    /// Gateway-side intent identifier.
    pub gateway_intent_id: String,
    /// Encrypted authorize URL.
    pub authorize_url_ciphertext: String,
    /// Encrypted polling secret.
    pub poll_secret_ciphertext: String,
    /// Expiry timestamp.
    pub expires_at: String,
}

/// One `source_connection_provisioning` row.
#[derive(Debug, Clone)]
pub struct ProvisioningRow {
    /// Provisioning identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Operator-facing label.
    pub display_label: String,
    /// Owning daemon.
    pub owner_daemon_id: String,
    /// Connection this provisioning targets, when it targets one.
    pub target_connection_id: Option<String>,
    /// Lifecycle status.
    pub status: String,
    /// Reviewed manifest version.
    pub manifest_version: String,
    /// Digest of the reviewed manifest.
    pub manifest_digest: String,
    /// Digest of the provider App identifier.
    pub app_id_digest: Option<String>,
    /// OAuth intent bound to this provisioning.
    pub oauth_intent_id: Option<String>,
    /// Terminal error code.
    pub error_code: Option<String>,
    /// Expiry timestamp.
    pub expires_at: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// A dedicated-App provisioning checkpoint to be recorded.
#[derive(Debug, Clone)]
pub struct NewProvisioning {
    /// Provisioning identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Operator-facing label.
    pub display_label: String,
    /// Owning daemon.
    pub owner_daemon_id: String,
    /// Connection this provisioning targets, when it targets one.
    pub target_connection_id: Option<String>,
    /// Reviewed manifest version.
    pub manifest_version: String,
    /// Digest of the reviewed manifest.
    pub manifest_digest: String,
    /// Expiry timestamp.
    pub expires_at: String,
}

/// A fenced update to a provisioning checkpoint.
#[derive(Debug, Clone)]
pub struct ProvisioningUpdate {
    /// Provisioning identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// The status the row must currently hold.
    pub expected_status: String,
    /// The status to move it to.
    pub status: String,
    /// Encrypted App identifier, when this step learns one.
    pub app_id_ciphertext: Option<String>,
    /// Digest of the App identifier, when this step learns one.
    pub app_id_digest: Option<String>,
    /// OAuth intent to bind, when this step creates one.
    pub oauth_intent_id: Option<String>,
    /// Error code to record.
    pub error_code: Option<String>,
}

/// The encrypted exact App identity behind one active dedicated connection.
#[derive(Debug, Clone)]
pub struct AppIdentityRow {
    /// Provisioning identifier that holds the identity.
    pub provisioning_id: String,
    /// Encrypted App identifier. Read as non-null: the join requires a
    /// `completed` provisioning, which is the step that records it.
    pub app_id_ciphertext: String,
    /// Digest of the App identifier, non-null for the same reason.
    pub app_id_digest: String,
}

/// A fenced dedicated-App lifecycle update against a connection.
#[derive(Debug, Clone)]
pub struct LifecycleUpdate {
    /// Connection identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// The version the row must currently hold.
    pub expected_version: i64,
    /// Lifecycle state to move to, already rendered.
    pub state: String,
    /// Reviewed manifest version now in force.
    pub manifest_version: String,
    /// Dedicated-App provisioning state.
    pub provision_state: String,
    /// Error code to record on both the provisioning and the connection.
    pub error_code: Option<String>,
    /// Correlating request identifier, recorded in the change log.
    pub request_id: String,
}

/// A completed ownership transfer, already fenced by the caller's validation.
#[derive(Debug, Clone)]
pub struct OwnerTransfer {
    /// Connection identifier.
    pub id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// The version the row must currently hold.
    pub expected_version: i64,
    /// Daemon taking ownership.
    pub target_daemon_id: String,
    /// Credential generation the new owner starts at.
    pub generation: i64,
    /// Correlating request identifier, recorded in the change log.
    pub request_id: String,
}

/// One `source_connection_changes` row, state unparsed.
#[derive(Debug, Clone)]
pub struct ChangeRow {
    /// Monotonic cursor.
    pub cursor: i64,
    /// Connection the change belongs to.
    pub connection_id: String,
    /// Project isolation scope.
    pub project_id: String,
    /// Connection version after the change.
    pub connection_version: i64,
    /// Lifecycle state, as stored.
    pub state: String,
    /// Error code recorded with the change.
    pub error_code: Option<String>,
    /// Correlating request identifier.
    pub request_id: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
}

/// Returns the stable daemon identity, creating it on first call.
pub async fn daemon_id(db: &AsyncDatabase, candidate: String, now: String) -> Result<String> {
    db.writer()
        .call(move |conn| {
            daemon_id_blocking(conn, &candidate, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Activates a new installation or reauthorizes an existing one, in one
/// transaction with its change-log entry.
pub async fn activate(db: &AsyncDatabase, input: NewActivation, now: String) -> Result<Activation> {
    db.writer()
        .call(move |conn| {
            activate_blocking(conn, &input, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one connection, only inside its project.
pub async fn read_connection(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
) -> Result<Option<SourceConnectionRow>> {
    db.reader()
        .call(move |conn| {
            read_connection_blocking(conn, &project_id, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Lists connections inside one project, most recently updated first.
pub async fn list_connections(
    db: &AsyncDatabase,
    project_id: String,
    provider: Option<String>,
    include_disconnected: bool,
    limit: usize,
) -> Result<Vec<SourceConnectionRow>> {
    db.reader()
        .call(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id FROM source_connections
                 WHERE project_id=?1 AND (?2 IS NULL OR provider=?2)
                   AND (?3 OR state!='disconnected')
                 ORDER BY updated_at DESC,id DESC LIMIT ?4",
            )?;
            let ids = statement
                .query_map(
                    params![
                        project_id,
                        provider,
                        include_disconnected,
                        limit.clamp(1, MAX_LIST_ROWS)
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| {
                    read_connection_blocking(conn, &project_id, &id)?
                        .context("SourceConnection disappeared during list")
                })
                .collect::<Result<Vec<_>>>()
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Releases encrypted adapter credentials, inside the project, owner and
/// active-state fences together.
pub async fn read_credential(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
    owner_daemon_id: String,
) -> Result<Option<ConnectionCredentialRow>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT installation_id,owner_daemon_id,generation,gateway_origin,
                        pairing_secret_ciphertext FROM source_connections
                 WHERE id=?1 AND project_id=?2 AND owner_daemon_id=?3 AND state='active'",
                params![id, project_id, owner_daemon_id],
                |row| {
                    Ok(ConnectionCredentialRow {
                        installation_id: row.get(0)?,
                        owner_daemon_id: row.get(1)?,
                        generation: row.get(2)?,
                        gateway_origin: row.get(3)?,
                        pairing_secret_ciphertext: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
        .await
        .map_err(flatten_err)
}

/// Records a resumable OAuth intent and reads it back.
pub async fn store_intent(db: &AsyncDatabase, input: NewIntent, now: String) -> Result<IntentRow> {
    db.writer()
        .call(move |conn| {
            store_intent_blocking(conn, &input, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one intent, only inside its project.
pub async fn read_intent(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
) -> Result<Option<IntentRow>> {
    db.reader()
        .call(move |conn| {
            read_intent_blocking(conn, &project_id, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads an intent's encrypted half, inside its project and owner fences.
///
/// Returns `None` when the intent does not exist in the project *or* when the
/// owner does not match, so a caller with the wrong daemon identity learns
/// nothing about whether the intent exists.
pub async fn read_intent_credential(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
    owner_daemon_id: String,
) -> Result<Option<(IntentRow, IntentCredentialRow)>> {
    db.reader()
        .call(move |conn| {
            let intent = read_intent_blocking(conn, &project_id, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))?;
            let Some(intent) = intent else {
                return Ok(None);
            };
            let credential = conn
                .query_row(
                    "SELECT gateway_intent_id,authorize_url_ciphertext,poll_secret_ciphertext,
                            owner_daemon_id,display_label
                     FROM source_connection_intents
                     WHERE id=?1 AND project_id=?2 AND owner_daemon_id=?3",
                    params![id, project_id, owner_daemon_id],
                    |row| {
                        Ok(IntentCredentialRow {
                            gateway_intent_id: row.get(0)?,
                            authorize_url_ciphertext: row.get(1)?,
                            poll_secret_ciphertext: row.get(2)?,
                            owner_daemon_id: row.get(3)?,
                            display_label: row.get(4)?,
                        })
                    },
                )
                .optional()?;
            Ok(credential.map(|credential| (intent, credential)))
        })
        .await
        .map_err(flatten_err)
}

/// Moves a pending intent to a terminal status, once.
///
/// Returns `None` when the intent was not pending or belongs to another
/// project — the fence is one condition and its two failure meanings are the
/// caller's to distinguish or not.
pub async fn complete_intent(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
    status: String,
    connection_id: Option<String>,
    error_code: Option<String>,
    now: String,
) -> Result<Option<IntentRow>> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE source_connection_intents SET status=?3,connection_id=?4,error_code=?5,
                 updated_at=?6 WHERE id=?1 AND project_id=?2 AND status='pending'",
                params![id, project_id, status, connection_id, error_code, now],
            )?;
            if changed != 1 {
                return Ok(None);
            }
            read_intent_blocking(conn, &project_id, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Applies a fenced lifecycle transition and logs it, in one transaction.
///
/// Returns `None` when the optimistic version fence or the project boundary
/// rejected it.
#[allow(clippy::too_many_arguments)]
pub async fn transition(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
    expected_version: i64,
    state: String,
    error_code: Option<String>,
    request_id: String,
    now: String,
) -> Result<Option<SourceConnectionRow>> {
    db.writer()
        .call(move |conn| {
            transition_blocking(
                conn,
                &project_id,
                &id,
                expected_version,
                &state,
                error_code.as_deref(),
                &request_id,
                &now,
            )
            .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Records a Gateway ownership transfer, fencing this daemon out of delivery.
///
/// Returns `None` when the version fence or the `state='active'` fence rejected
/// it. The pairing secret is dropped in the same statement that changes owner:
/// a transfer that moved ownership but left this daemon's credentials behind is
/// the state the fence exists to prevent.
pub async fn transfer_owner(
    db: &AsyncDatabase,
    input: OwnerTransfer,
    now: String,
) -> Result<Option<SourceConnectionRow>> {
    db.writer()
        .call(move |conn| {
            transfer_owner_blocking(conn, &input, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Records delivery progress behind a monotonic cursor fence.
///
/// Returns `false` when the cursor would go backwards or the connection is not
/// active. `last_acked_cursor<=?3` is what makes a duplicated or reordered
/// acknowledgement harmless.
pub async fn record_delivery(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
    cursor: i64,
    lag: i64,
    now: String,
) -> Result<bool> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE source_connections SET last_acked_cursor=?3,delivery_lag=?4,
                 last_delivery_at=?5,updated_at=?5
                 WHERE id=?1 AND project_id=?2 AND state='active' AND last_acked_cursor<=?3",
                params![id, project_id, cursor, lag, now],
            )?;
            Ok(changed == 1)
        })
        .await
        .map_err(flatten_err)
}

/// Reads project-scoped change-log rows after a cursor.
pub async fn read_changes(
    db: &AsyncDatabase,
    project_id: String,
    after: i64,
    limit: usize,
) -> Result<Vec<ChangeRow>> {
    db.reader()
        .call(move |conn| {
            let mut statement = conn.prepare(
                "SELECT id,connection_id,project_id,connection_version,state,error_code,request_id,
                        created_at
                 FROM source_connection_changes WHERE project_id=?1 AND id>?2 ORDER BY id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(
                    params![project_id, after.max(0), limit.clamp(1, MAX_LIST_ROWS)],
                    |row| {
                        Ok(ChangeRow {
                            cursor: row.get(0)?,
                            connection_id: row.get(1)?,
                            project_id: row.get(2)?,
                            connection_version: row.get(3)?,
                            state: row.get(4)?,
                            error_code: row.get(5)?,
                            request_id: row.get(6)?,
                            created_at: row.get(7)?,
                        })
                    },
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(flatten_err)
}

/// Records a dedicated-App provisioning checkpoint and reads it back.
pub async fn store_provisioning(
    db: &AsyncDatabase,
    input: NewProvisioning,
    now: String,
) -> Result<ProvisioningRow> {
    db.writer()
        .call(move |conn| {
            store_provisioning_blocking(conn, &input, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Reads one provisioning checkpoint, only inside its project.
pub async fn read_provisioning(
    db: &AsyncDatabase,
    project_id: String,
    id: String,
) -> Result<Option<ProvisioningRow>> {
    db.reader()
        .call(move |conn| {
            read_provisioning_blocking(conn, &project_id, &id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Advances a provisioning checkpoint behind an exact prior-status fence.
///
/// Returns `None` when the fence rejected it. `COALESCE` on the three learned
/// values means a later step never blanks what an earlier one recorded.
pub async fn update_provisioning(
    db: &AsyncDatabase,
    input: ProvisioningUpdate,
    now: String,
) -> Result<Option<ProvisioningRow>> {
    db.writer()
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE source_connection_provisioning SET status=?4,
                 app_id_ciphertext=COALESCE(?5,app_id_ciphertext),
                 app_id_digest=COALESCE(?6,app_id_digest),
                 oauth_intent_id=COALESCE(?7,oauth_intent_id),error_code=?8,updated_at=?9
                 WHERE id=?1 AND project_id=?2 AND status=?3",
                params![
                    input.id,
                    input.project_id,
                    input.expected_status,
                    input.status,
                    input.app_id_ciphertext,
                    input.app_id_digest,
                    input.oauth_intent_id,
                    input.error_code,
                    now,
                ],
            )?;
            if changed != 1 {
                return Ok(None);
            }
            read_provisioning_blocking(conn, &input.project_id, &input.id)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Resolves the encrypted App identity behind one completed dedicated
/// connection, joined through its OAuth intent.
pub async fn read_app_identity(
    db: &AsyncDatabase,
    project_id: String,
    connection_id: String,
) -> Result<Option<AppIdentityRow>> {
    db.reader()
        .call(move |conn| {
            conn.query_row(
                "SELECT p.id,p.app_id_ciphertext,p.app_id_digest
                 FROM source_connection_provisioning p
                 JOIN source_connection_intents i ON i.id=p.oauth_intent_id
                 JOIN source_connections c ON c.id=i.connection_id AND c.project_id=p.project_id
                 WHERE p.project_id=?1 AND c.id=?2 AND p.status='completed'
                   AND c.provisioning_mode='managed_dedicated'",
                params![project_id, connection_id],
                |row| {
                    Ok(AppIdentityRow {
                        provisioning_id: row.get(0)?,
                        app_id_ciphertext: row.get(1)?,
                        app_id_digest: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
        .await
        .map_err(flatten_err)
}

/// Applies a dedicated-App lifecycle update behind the version *and* mode
/// fences, logging the change in the same transaction.
///
/// Returns `None` when either fence rejected it.
pub async fn update_dedicated_lifecycle(
    db: &AsyncDatabase,
    input: LifecycleUpdate,
    now: String,
) -> Result<Option<SourceConnectionRow>> {
    db.writer()
        .call(move |conn| {
            update_dedicated_lifecycle_blocking(conn, &input, &now)
                .map_err(|error| tokio_rusqlite::Error::Other(error.into()))
        })
        .await
        .map_err(flatten_err)
}

fn daemon_id_blocking(conn: &Connection, candidate: &str, now: &str) -> Result<String> {
    if let Some(value) = conn
        .query_row(
            "SELECT daemon_id FROM source_daemon_identity WHERE singleton=1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(value);
    }
    conn.execute(
        "INSERT OR IGNORE INTO source_daemon_identity(singleton,daemon_id,created_at)
         VALUES(1,?1,?2)",
        params![candidate, now],
    )?;
    // Read back rather than returning `candidate`: `OR IGNORE` means a
    // concurrent caller may have won, and the identity that counts is the one
    // in the table.
    conn.query_row(
        "SELECT daemon_id FROM source_daemon_identity WHERE singleton=1",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn activate_blocking(conn: &Connection, input: &NewActivation, now: &str) -> Result<Activation> {
    let transaction = conn.unchecked_transaction()?;
    let existing = transaction
        .query_row(
            "SELECT id,project_id,owner_daemon_id,generation,version,state FROM source_connections
             WHERE provider=?1 AND installation_id=?2 AND state!='disconnected'",
            params![input.provider, input.installation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let reauthorized = if let Some((id, project, owner, generation, version, state)) = existing {
        if project != input.project_id || owner != input.owner_daemon_id || id != input.id {
            return Ok(Activation::OwnerConflict);
        }
        if input.generation < generation || input.version < version {
            return Ok(Activation::StaleFence);
        }
        let changed = transaction.execute(
            "UPDATE source_connections SET display_label=?2,provisioning_mode=?3,
             app_ownership=?4,app_id_digest=?5,manifest_version=?6,provision_state=?7,
             provision_error_code=?8,generation=?9,version=?10,state='active',
             capabilities_json=?11,scopes_json=?12,trigger_name=?13,gateway_origin=?14,
             pairing_secret_ciphertext=?15,last_error_code=NULL,
             last_acked_cursor=MAX(last_acked_cursor,?16),updated_at=?17,
             reauthorized_at=CASE WHEN ?9>generation THEN ?17 ELSE reauthorized_at END,
             disconnected_at=NULL WHERE id=?1 AND generation<=?9 AND version<=?10",
            params![
                input.id,
                input.display_label,
                input.provisioning_mode,
                input.app_ownership,
                input.app_id_digest,
                input.manifest_version,
                input.provision_state,
                input.provision_error_code,
                input.generation,
                input.version,
                input.capabilities_json,
                input.scopes_json,
                input.trigger_name,
                input.gateway_origin,
                input.pairing_secret_ciphertext,
                input.last_acked_cursor,
                now,
            ],
        )?;
        if changed != 1 || state == "disconnected" {
            return Ok(Activation::ReauthorizationConflict);
        }
        true
    } else {
        transaction.execute(
            "INSERT INTO source_connections
             (id,project_id,provider,display_label,provisioning_mode,installation_id,
              installation_id_digest,enterprise_id_digest,owner_daemon_id,generation,version,
              state,capabilities_json,scopes_json,trigger_name,gateway_origin,
              pairing_secret_ciphertext,last_acked_cursor,created_at,updated_at,app_ownership,
              app_id_digest,manifest_version,provision_state,provision_error_code)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'active',?12,?13,?14,?15,
                    ?16,?17,?18,?18,?19,?20,?21,?22,?23)",
            params![
                input.id,
                input.project_id,
                input.provider,
                input.display_label,
                input.provisioning_mode,
                input.installation_id,
                input.installation_id_digest,
                input.enterprise_id_digest,
                input.owner_daemon_id,
                input.generation,
                input.version,
                input.capabilities_json,
                input.scopes_json,
                input.trigger_name,
                input.gateway_origin,
                input.pairing_secret_ciphertext,
                input.last_acked_cursor,
                now,
                input.app_ownership,
                input.app_id_digest,
                input.manifest_version,
                input.provision_state,
                input.provision_error_code,
            ],
        )?;
        false
    };
    append_change(
        &transaction,
        &input.id,
        &input.project_id,
        input.version,
        "active",
        None,
        Some(&input.request_id),
        now,
    )?;
    transaction.commit()?;
    let row = read_connection_blocking(conn, &input.project_id, &input.id)?
        .context("activated SourceConnection missing")?;
    Ok(if reauthorized {
        Activation::Reauthorized(row)
    } else {
        Activation::Created(row)
    })
}

fn store_intent_blocking(conn: &Connection, input: &NewIntent, now: &str) -> Result<IntentRow> {
    conn.execute(
        "INSERT INTO source_connection_intents
         (id,project_id,provider,display_label,provisioning_mode,owner_daemon_id,actor_digest,
          gateway_intent_id,authorize_url_ciphertext,poll_secret_ciphertext,status,
          expires_at,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending',?11,?12,?12)",
        params![
            input.id,
            input.project_id,
            input.provider,
            input.display_label,
            input.provisioning_mode,
            input.owner_daemon_id,
            input.actor_digest,
            input.gateway_intent_id,
            input.authorize_url_ciphertext,
            input.poll_secret_ciphertext,
            input.expires_at,
            now,
        ],
    )?;
    read_intent_blocking(conn, &input.project_id, &input.id)?.context("stored intent missing")
}

fn read_intent_blocking(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<IntentRow>> {
    conn.query_row(
        "SELECT id,project_id,provider,provisioning_mode,status,connection_id,error_code,
         expires_at,created_at,updated_at FROM source_connection_intents
         WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            Ok(IntentRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                provider: row.get(2)?,
                provisioning_mode: row.get(3)?,
                status: row.get(4)?,
                connection_id: row.get(5)?,
                error_code: row.get(6)?,
                expires_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn transition_blocking(
    conn: &Connection,
    project_id: &str,
    id: &str,
    expected_version: i64,
    state: &str,
    error_code: Option<&str>,
    request_id: &str,
    now: &str,
) -> Result<Option<SourceConnectionRow>> {
    let transaction = conn.unchecked_transaction()?;
    let clear_credentials = state == "disconnected";
    let changed = transaction.execute(
        "UPDATE source_connections SET state=?4,version=version+1,last_error_code=?5,
         pairing_secret_ciphertext=CASE WHEN ?6 THEN NULL ELSE pairing_secret_ciphertext END,
         disconnected_at=CASE WHEN ?6 THEN ?7 ELSE disconnected_at END,updated_at=?7
         WHERE id=?1 AND project_id=?2 AND version=?3",
        params![
            id,
            project_id,
            expected_version,
            state,
            error_code,
            clear_credentials,
            now,
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    append_change(
        &transaction,
        id,
        project_id,
        expected_version + 1,
        state,
        error_code,
        Some(request_id),
        now,
    )?;
    transaction.commit()?;
    read_connection_blocking(conn, project_id, id)
}

fn transfer_owner_blocking(
    conn: &Connection,
    input: &OwnerTransfer,
    now: &str,
) -> Result<Option<SourceConnectionRow>> {
    let transaction = conn.unchecked_transaction()?;
    let changed = transaction.execute(
        "UPDATE source_connections SET owner_daemon_id=?4,generation=?5,
         pairing_secret_ciphertext=NULL,state='suspended',version=version+1,
         last_error_code='owner_transfer_pending_acceptance',updated_at=?6
         WHERE id=?1 AND project_id=?2 AND version=?3 AND state='active'",
        params![
            input.id,
            input.project_id,
            input.expected_version,
            input.target_daemon_id,
            input.generation,
            now,
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    append_change(
        &transaction,
        &input.id,
        &input.project_id,
        input.expected_version + 1,
        "suspended",
        Some("owner_transfer_pending_acceptance"),
        Some(&input.request_id),
        now,
    )?;
    transaction.commit()?;
    read_connection_blocking(conn, &input.project_id, &input.id)
}

fn store_provisioning_blocking(
    conn: &Connection,
    input: &NewProvisioning,
    now: &str,
) -> Result<ProvisioningRow> {
    conn.execute(
        "INSERT INTO source_connection_provisioning
         (id,project_id,display_label,owner_daemon_id,target_connection_id,status,
          manifest_version,manifest_digest,expires_at,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,'awaiting_approval',?6,?7,?8,?9,?9)",
        params![
            input.id,
            input.project_id,
            input.display_label,
            input.owner_daemon_id,
            input.target_connection_id,
            input.manifest_version,
            input.manifest_digest,
            input.expires_at,
            now,
        ],
    )?;
    read_provisioning_blocking(conn, &input.project_id, &input.id)?
        .context("stored dedicated provisioning checkpoint missing")
}

fn read_provisioning_blocking(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<ProvisioningRow>> {
    conn.query_row(
        "SELECT id,project_id,display_label,owner_daemon_id,target_connection_id,status,
         manifest_version,manifest_digest,app_id_digest,oauth_intent_id,error_code,
         expires_at,created_at,updated_at
         FROM source_connection_provisioning WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            Ok(ProvisioningRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                display_label: row.get(2)?,
                owner_daemon_id: row.get(3)?,
                target_connection_id: row.get(4)?,
                status: row.get(5)?,
                manifest_version: row.get(6)?,
                manifest_digest: row.get(7)?,
                app_id_digest: row.get(8)?,
                oauth_intent_id: row.get(9)?,
                error_code: row.get(10)?,
                expires_at: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn update_dedicated_lifecycle_blocking(
    conn: &Connection,
    input: &LifecycleUpdate,
    now: &str,
) -> Result<Option<SourceConnectionRow>> {
    let transaction = conn.unchecked_transaction()?;
    let changed = transaction.execute(
        "UPDATE source_connections SET state=?4,version=version+1,manifest_version=?5,
         provision_state=?6,provision_error_code=?7,last_error_code=?7,updated_at=?8
         WHERE id=?1 AND project_id=?2 AND version=?3
           AND provisioning_mode='managed_dedicated'",
        params![
            input.id,
            input.project_id,
            input.expected_version,
            input.state,
            input.manifest_version,
            input.provision_state,
            input.error_code,
            now,
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    append_change(
        &transaction,
        &input.id,
        &input.project_id,
        input.expected_version + 1,
        &input.state,
        input.error_code.as_deref(),
        Some(&input.request_id),
        now,
    )?;
    transaction.commit()?;
    read_connection_blocking(conn, &input.project_id, &input.id)
}

fn read_connection_blocking(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<SourceConnectionRow>> {
    conn.query_row(
        "SELECT id,project_id,provider,display_label,provisioning_mode,installation_id,
         installation_id_digest,enterprise_id_digest,owner_daemon_id,generation,version,state,
         capabilities_json,scopes_json,trigger_name,last_delivery_at,last_acked_cursor,
         delivery_lag,last_error_code,created_at,updated_at,reauthorized_at,disconnected_at,
         app_ownership,app_id_digest,manifest_version,provision_state,provision_error_code
         FROM source_connections WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            Ok(SourceConnectionRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                provider: row.get(2)?,
                display_label: row.get(3)?,
                provisioning_mode: row.get(4)?,
                installation_id: row.get(5)?,
                installation_id_digest: row.get(6)?,
                enterprise_id_digest: row.get(7)?,
                owner_daemon_id: row.get(8)?,
                generation: row.get(9)?,
                version: row.get(10)?,
                state: row.get(11)?,
                capabilities_json: row.get(12)?,
                scopes_json: row.get(13)?,
                trigger_name: row.get(14)?,
                last_delivery_at: row.get(15)?,
                last_acked_cursor: row.get(16)?,
                delivery_lag: row.get(17)?,
                last_error_code: row.get(18)?,
                created_at: row.get(19)?,
                updated_at: row.get(20)?,
                reauthorized_at: row.get(21)?,
                disconnected_at: row.get(22)?,
                app_ownership: row.get(23)?,
                app_id_digest: row.get(24)?,
                manifest_version: row.get(25)?,
                provision_state: row.get(26)?,
                provision_error_code: row.get(27)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn append_change(
    conn: &Connection,
    id: &str,
    project_id: &str,
    version: i64,
    state: &str,
    error_code: Option<&str>,
    request_id: Option<&str>,
    created_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO source_connection_changes
         (connection_id,project_id,connection_version,state,error_code,request_id,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            id, project_id, version, state, error_code, request_id, created_at,
        ],
    )?;
    Ok(())
}
