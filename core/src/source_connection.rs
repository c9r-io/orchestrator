//! Durable provider connection lifecycle with project-scoped, secret-free projections.

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{future::Future, pin::Pin};
use uuid::Uuid;

/// Provider proxy port used by source automation without exposing managed credentials.
pub trait SourceConnectionProvider: Send + Sync {
    /// Resolves a reviewed provider permalink through a managed connection.
    fn permalink<'a>(
        &'a self,
        project_id: &'a str,
        connection_id: &'a str,
        channel_id: &'a str,
        message_ts: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

/// Disabled provider implementation used when managed integrations are not configured.
pub struct DisabledSourceConnectionProvider;

impl SourceConnectionProvider for DisabledSourceConnectionProvider {
    fn permalink<'a>(
        &'a self,
        _project_id: &'a str,
        _connection_id: &'a str,
        _channel_id: &'a str,
        _message_ts: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async { bail!("managed_source_connection_provider_unavailable") })
    }
}

/// Stable provisioning modes shared with integration gateways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnectionMode {
    /// One reviewed Orchestrator app installed into multiple provider tenants.
    ManagedShared,
    /// Reserved private-app mode; capability negotiation must fail closed until supported.
    ManagedDedicated,
    /// Existing user-managed app and credentials.
    Manual,
}

impl SourceConnectionMode {
    /// Stable persistence and API value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManagedShared => "managed_shared",
            Self::ManagedDedicated => "managed_dedicated",
            Self::Manual => "manual",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "managed_shared" => Ok(Self::ManagedShared),
            "managed_dedicated" => Ok(Self::ManagedDedicated),
            "manual" => Ok(Self::Manual),
            _ => bail!("invalid SourceConnection provisioning mode"),
        }
    }
}

/// Durable SourceConnection lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnectionState {
    /// OAuth or manual setup is not complete.
    Connecting,
    /// Delivery and reviewed provider operations are available.
    Active,
    /// Human intervention is required.
    Attention,
    /// Delivery is intentionally paused.
    Suspended,
    /// Provider credentials were revoked.
    Revoked,
    /// Credentials were destroyed and delivery permanently stopped.
    Disconnected,
}

impl SourceConnectionState {
    /// Stable persistence and API value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Active => "active",
            Self::Attention => "attention",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
            Self::Disconnected => "disconnected",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "connecting" => Ok(Self::Connecting),
            "active" => Ok(Self::Active),
            "attention" => Ok(Self::Attention),
            "suspended" => Ok(Self::Suspended),
            "revoked" => Ok(Self::Revoked),
            "disconnected" => Ok(Self::Disconnected),
            _ => bail!("invalid SourceConnection state"),
        }
    }
}

/// Role-safe SourceConnection projection. Credential and private endpoint fields are absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConnection {
    /// Stable local connection ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Provider identifier.
    pub provider: String,
    /// User-controlled bounded display label.
    pub display_label: String,
    /// Provisioning path.
    pub provisioning_mode: SourceConnectionMode,
    /// Slack App authority: Orchestrator shared app, workspace-owned app, or external app.
    pub app_ownership: String,
    /// Non-reversible Slack App identity digest.
    pub app_id_digest: Option<String>,
    /// Reviewed manifest profile version for managed dedicated apps.
    pub manifest_version: Option<String>,
    /// Safe dedicated provisioning checkpoint.
    pub provision_state: Option<String>,
    /// Stable dedicated provisioning failure code.
    pub provision_error_code: Option<String>,
    /// Opaque gateway installation identity, not a provider team ID.
    pub installation_id: String,
    /// Non-reversible provider tenant digest.
    pub installation_id_digest: String,
    /// Optional non-reversible enterprise digest.
    pub enterprise_id_digest: Option<String>,
    /// Exclusive owner daemon identity.
    pub owner_daemon_id: String,
    /// Credential generation fence.
    pub generation: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Lifecycle state.
    pub state: SourceConnectionState,
    /// Negotiated safe capabilities.
    pub capabilities: Vec<String>,
    /// Granted provider scope names.
    pub scopes: Vec<String>,
    /// Automatically created or associated Trigger.
    pub trigger_name: Option<String>,
    /// Last normalized delivery timestamp.
    pub last_delivery_at: Option<String>,
    /// Last acknowledged gateway cursor.
    pub last_acked_cursor: i64,
    /// Estimated unacknowledged delivery count.
    pub delivery_lag: i64,
    /// Stable privacy-safe failure code.
    pub last_error_code: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last mutation timestamp.
    pub updated_at: String,
    /// Most recent credential generation change.
    pub reauthorized_at: Option<String>,
    /// Disconnect timestamp.
    pub disconnected_at: Option<String>,
}

/// Internal input used after a verified Gateway OAuth completion.
#[derive(Debug, Clone)]
pub struct ActivateSourceConnection {
    /// Stable local ID, normally derived from gateway installation ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Provider, currently `slack` for managed mode.
    pub provider: String,
    /// Bounded display label.
    pub display_label: String,
    /// Provisioning mode.
    pub provisioning_mode: SourceConnectionMode,
    /// Slack App authority projection.
    pub app_ownership: String,
    /// Non-reversible Slack App identity digest.
    pub app_id_digest: Option<String>,
    /// Reviewed manifest profile version.
    pub manifest_version: Option<String>,
    /// Safe dedicated provisioning checkpoint.
    pub provision_state: Option<String>,
    /// Stable dedicated provisioning failure code.
    pub provision_error_code: Option<String>,
    /// Opaque installation ID.
    pub installation_id: String,
    /// Verified tenant digest.
    pub installation_id_digest: String,
    /// Optional verified enterprise digest.
    pub enterprise_id_digest: Option<String>,
    /// Exclusive daemon owner.
    pub owner_daemon_id: String,
    /// Gateway credential generation.
    pub generation: i64,
    /// Gateway lifecycle version.
    pub version: i64,
    /// Gateway cursor already acknowledged before this daemon adopted the installation.
    pub last_acked_cursor: i64,
    /// Negotiated capabilities.
    pub capabilities: Vec<String>,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// Associated Trigger.
    pub trigger_name: Option<String>,
    /// Validated public Gateway origin used internally by the adapter.
    pub gateway_origin: Option<String>,
    /// Encrypted installation-scoped pairing credential.
    pub pairing_secret_ciphertext: Option<String>,
    /// Canonical mutation request ID.
    pub request_id: String,
}

/// Internal input used after the Gateway atomically transfers installation ownership.
#[derive(Debug, Clone)]
pub struct TransferSourceConnectionOwner {
    /// Owning project.
    pub project_id: String,
    /// Local connection ID.
    pub id: String,
    /// Current local/Gateway lifecycle version.
    pub expected_version: i64,
    /// New exclusive daemon owner.
    pub target_daemon_id: String,
    /// Gateway credential generation.
    pub generation: i64,
    /// Canonical governed request ID.
    pub request_id: String,
}

/// Internal credential material returned only to the daemon adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceConnectionCredential {
    /// Opaque installation ID.
    pub installation_id: String,
    /// Owner daemon fence.
    pub owner_daemon_id: String,
    /// Credential generation fence.
    pub generation: i64,
    /// Validated Gateway origin.
    pub gateway_origin: String,
    /// Encrypted pairing credential envelope.
    pub pairing_secret_ciphertext: String,
}

/// Safe OAuth intent projection used by CLI and GUI status polling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConnectionIntent {
    /// Local intent ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Provider identifier.
    pub provider: String,
    /// Requested provisioning mode.
    pub provisioning_mode: SourceConnectionMode,
    /// Current status.
    pub status: String,
    /// Resulting connection ID after completion.
    pub connection_id: Option<String>,
    /// Stable failure code.
    pub error_code: Option<String>,
    /// Expiration timestamp.
    pub expires_at: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last status update.
    pub updated_at: String,
}

/// Internal encrypted OAuth intent record used by the daemon Gateway adapter.
#[derive(Debug, Clone)]
pub struct SourceConnectionIntentCredential {
    /// Safe intent fields.
    pub intent: SourceConnectionIntent,
    /// Gateway intent ID.
    pub gateway_intent_id: String,
    /// Encrypted authorize URL (contains OAuth state).
    pub authorize_url_ciphertext: String,
    /// Encrypted Gateway polling secret.
    pub poll_secret_ciphertext: String,
    /// Requested user-facing connection label.
    pub display_label: String,
    /// Owner daemon identity fence.
    pub owner_daemon_id: String,
}

/// Secret-free checkpoint for one managed dedicated Slack App provisioning flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedicatedProvisioning {
    /// Opaque local provisioning ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Bounded connection label.
    pub display_label: String,
    /// Exclusive daemon owner.
    pub owner_daemon_id: String,
    /// Existing logical connection selected for a reviewed mode migration.
    pub target_connection_id: Option<String>,
    /// Durable provisioning state.
    pub status: String,
    /// Reviewed manifest profile version.
    pub manifest_version: String,
    /// Manifest content digest.
    pub manifest_digest: String,
    /// Non-reversible App ID digest after creation.
    pub app_id_digest: Option<String>,
    /// Resulting OAuth intent after credential handoff.
    pub oauth_intent_id: Option<String>,
    /// Stable failure code.
    pub error_code: Option<String>,
    /// Ephemeral session expiry.
    pub expires_at: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Internal exact App identity envelope used only for governed lifecycle calls.
#[derive(Debug, Clone)]
pub struct DedicatedAppIdentityCredential {
    /// Provisioning/App connection identity used by the Gateway endpoints.
    pub provisioning_id: String,
    /// Encrypted exact Slack App ID; never exposed through a safe projection.
    pub app_id_ciphertext: String,
    /// Non-reversible App identity used for receipt and projection checks.
    pub app_id_digest: String,
}

/// Internal input for creating a dedicated provisioning checkpoint.
#[derive(Debug, Clone)]
pub struct StoreDedicatedProvisioning {
    /// Opaque local provisioning ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Bounded display label.
    pub display_label: String,
    /// Exclusive daemon owner.
    pub owner_daemon_id: String,
    /// Existing logical connection selected for a reviewed mode migration.
    pub target_connection_id: Option<String>,
    /// Reviewed manifest profile version.
    pub manifest_version: String,
    /// Manifest content digest.
    pub manifest_digest: String,
    /// Ephemeral session expiry.
    pub expires_at: String,
}

/// Internal CAS transition for a dedicated provisioning checkpoint.
#[derive(Debug, Clone)]
pub struct UpdateDedicatedProvisioning {
    /// Owning project.
    pub project_id: String,
    /// Opaque local provisioning ID.
    pub id: String,
    /// Exact prior status used as a CAS fence.
    pub expected_status: String,
    /// Next durable status.
    pub status: String,
    /// Optional encrypted exact Slack App ID for orphan recovery.
    pub app_id_ciphertext: Option<String>,
    /// Optional non-reversible App identity.
    pub app_id_digest: Option<String>,
    /// Optional resulting managed OAuth intent.
    pub oauth_intent_id: Option<String>,
    /// Optional privacy-safe failure code.
    pub error_code: Option<String>,
}

/// CAS update for safe dedicated App lifecycle metadata on a SourceConnection.
#[derive(Debug, Clone)]
pub struct UpdateDedicatedConnectionLifecycle {
    /// Owning project.
    pub project_id: String,
    /// Stable logical SourceConnection ID.
    pub id: String,
    /// Exact prior connection version.
    pub expected_version: i64,
    /// Resulting connection state.
    pub state: SourceConnectionState,
    /// Reviewed manifest profile version.
    pub manifest_version: String,
    /// Safe lifecycle projection (`completed`, `reauthorization_required`, `app_deleted`).
    pub provision_state: String,
    /// Optional stable privacy-safe failure code.
    pub error_code: Option<String>,
    /// Canonical governed request ID.
    pub request_id: String,
}

/// Input for persisting a newly authenticated Gateway OAuth intent.
#[derive(Debug, Clone)]
pub struct StoreSourceConnectionIntent {
    /// Local intent ID.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Provider identifier.
    pub provider: String,
    /// Requested user-facing connection label.
    pub display_label: String,
    /// Requested provisioning mode.
    pub provisioning_mode: SourceConnectionMode,
    /// Owner daemon identity.
    pub owner_daemon_id: String,
    /// Non-reversible initiating actor digest.
    pub actor_digest: String,
    /// Gateway intent ID.
    pub gateway_intent_id: String,
    /// Encrypted authorize URL.
    pub authorize_url_ciphertext: String,
    /// Encrypted polling secret.
    pub poll_secret_ciphertext: String,
    /// Expiration timestamp.
    pub expires_at: String,
}

/// Monotonic watch row for one connection transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceConnectionChange {
    /// Monotonic daemon-local cursor.
    pub cursor: i64,
    /// Connection ID.
    pub connection_id: String,
    /// Owning project.
    pub project_id: String,
    /// Connection version after the transition.
    pub connection_version: i64,
    /// State after the transition.
    pub state: SourceConnectionState,
    /// Optional stable error code.
    pub error_code: Option<String>,
    /// Canonical request ID for governed mutations.
    pub request_id: Option<String>,
    /// Change timestamp.
    pub created_at: String,
}

/// Async repository for SourceConnection lifecycle operations.
#[derive(Clone)]
pub struct AsyncSourceConnectionRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncSourceConnectionRepository {
    /// Creates a repository over the shared daemon database.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Returns or creates the stable daemon identity used for Gateway ownership.
    pub async fn daemon_id(&self) -> Result<String> {
        self.db
            .writer()
            .call(|conn| daemon_id(conn).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Activates or idempotently reauthorizes one verified installation.
    pub async fn activate(&self, input: ActivateSourceConnection) -> Result<SourceConnection> {
        self.db
            .writer()
            .call(move |conn| activate(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Gets one connection only when it belongs to the requested project.
    pub async fn get(&self, project_id: &str, id: &str) -> Result<Option<SourceConnection>> {
        let project_id = project_id.to_string();
        let id = id.to_string();
        self.db
            .reader()
            .call(move |conn| read_connection(conn, &project_id, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Lists connections inside one project boundary.
    pub async fn list(
        &self,
        project_id: &str,
        provider: Option<&str>,
        include_disconnected: bool,
        limit: usize,
    ) -> Result<Vec<SourceConnection>> {
        let project_id = project_id.to_string();
        let provider = provider.map(str::to_string);
        self.db
            .reader()
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
                            limit.clamp(1, 500)
                        ],
                        |row| row.get::<_, String>(0),
                    )?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ids.into_iter()
                    .map(|id| {
                        read_connection(conn, &project_id, &id)?
                            .context("SourceConnection disappeared during list")
                    })
                    .collect::<Result<Vec<_>>>()
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns encrypted adapter credentials after project and owner fences pass.
    pub async fn credential(
        &self,
        project_id: &str,
        id: &str,
        owner_daemon_id: &str,
    ) -> Result<Option<SourceConnectionCredential>> {
        let project_id = project_id.to_string();
        let id = id.to_string();
        let owner_daemon_id = owner_daemon_id.to_string();
        self.db
            .reader()
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT installation_id,owner_daemon_id,generation,gateway_origin,
                            pairing_secret_ciphertext FROM source_connections
                     WHERE id=?1 AND project_id=?2 AND owner_daemon_id=?3 AND state='active'",
                        params![id, project_id, owner_daemon_id],
                        |row| {
                            Ok(SourceConnectionCredential {
                                installation_id: row.get(0)?,
                                owner_daemon_id: row.get(1)?,
                                generation: row.get(2)?,
                                gateway_origin: row.get(3)?,
                                pairing_secret_ciphertext: row.get(4)?,
                            })
                        },
                    )
                    .optional()?)
            })
            .await
            .map_err(flatten_err)
    }

    /// Stores a resumable OAuth intent without exposing state or polling credentials.
    pub async fn store_intent(
        &self,
        input: StoreSourceConnectionIntent,
    ) -> Result<SourceConnectionIntent> {
        self.db
            .writer()
            .call(move |conn| store_intent(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Reads one encrypted intent only within its project and daemon owner boundary.
    pub async fn intent_credential(
        &self,
        project_id: &str,
        id: &str,
        owner_daemon_id: &str,
    ) -> Result<Option<SourceConnectionIntentCredential>> {
        let project_id = project_id.to_string();
        let id = id.to_string();
        let owner_daemon_id = owner_daemon_id.to_string();
        self.db
            .reader()
            .call(move |conn| {
                read_intent_credential(conn, &project_id, &id, &owner_daemon_id).map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Advances one OAuth intent to a terminal state exactly once.
    pub async fn complete_intent(
        &self,
        project_id: &str,
        id: &str,
        status: &str,
        connection_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<SourceConnectionIntent> {
        if !matches!(status, "completed" | "cancelled" | "expired" | "failed") {
            bail!("invalid terminal SourceConnection intent status");
        }
        let project_id = project_id.to_string();
        let id = id.to_string();
        let status = status.to_string();
        let connection_id = connection_id.map(str::to_string);
        let error_code = error_code.map(str::to_string);
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_connection_intents SET status=?3,connection_id=?4,error_code=?5,
                     updated_at=?6 WHERE id=?1 AND project_id=?2 AND status='pending'",
                    params![id, project_id, status, connection_id, error_code, now_ts()],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!(
                        "SourceConnection intent is not pending or project does not match"
                    )));
                }
                read_intent(conn, &project_id, &id)
                    .map_err(other)?
                    .context("completed SourceConnection intent missing")
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Applies a governed lifecycle transition using optimistic concurrency.
    pub async fn transition(
        &self,
        project_id: &str,
        id: &str,
        expected_version: i64,
        state: SourceConnectionState,
        error_code: Option<&str>,
        request_id: &str,
    ) -> Result<SourceConnection> {
        let project_id = project_id.to_string();
        let id = id.to_string();
        let error_code = error_code.map(str::to_string);
        let request_id = request_id.to_string();
        self.db
            .writer()
            .call(move |conn| {
                transition(
                    conn,
                    &project_id,
                    &id,
                    expected_version,
                    state,
                    error_code.as_deref(),
                    &request_id,
                )
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Records a successful Gateway ownership transfer and fences this daemon from delivery.
    pub async fn transfer_owner(
        &self,
        input: TransferSourceConnectionOwner,
    ) -> Result<SourceConnection> {
        for (label, value) in [
            ("target daemon", input.target_daemon_id.as_str()),
            ("request ID", input.request_id.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("SourceConnection {label} cannot be empty");
            }
        }
        if input.expected_version < 1 || input.generation < 1 {
            bail!("SourceConnection transfer fences must be positive");
        }
        self.db
            .writer()
            .call(move |conn| {
                let transaction = conn.transaction()?;
                let now = now_ts();
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
                    return Err(other(anyhow::anyhow!(
                        "SourceConnection transfer version conflict or connection inactive"
                    )));
                }
                append_change(
                    &transaction,
                    &input.id,
                    &input.project_id,
                    input.expected_version + 1,
                    SourceConnectionState::Suspended,
                    Some("owner_transfer_pending_acceptance"),
                    Some(&input.request_id),
                    &now,
                )
                .map_err(other)?;
                transaction.commit()?;
                read_connection(conn, &input.project_id, &input.id)
                    .map_err(other)?
                    .context("transferred SourceConnection missing")
                    .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Records durable delivery progress with a monotonic cursor fence.
    pub async fn record_delivery(
        &self,
        project_id: &str,
        id: &str,
        cursor: i64,
        lag: i64,
    ) -> Result<()> {
        if cursor < 0 || lag < 0 {
            bail!("SourceConnection cursor and lag must be non-negative");
        }
        let project_id = project_id.to_string();
        let id = id.to_string();
        self.db
            .writer()
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE source_connections SET last_acked_cursor=?3,delivery_lag=?4,
                     last_delivery_at=?5,updated_at=?5
                     WHERE id=?1 AND project_id=?2 AND state='active' AND last_acked_cursor<=?3",
                    params![id, project_id, cursor, lag, now_ts()],
                )?;
                if changed != 1 {
                    return Err(other(anyhow::anyhow!(
                        "SourceConnection delivery cursor is stale or connection inactive"
                    )));
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    /// Reads project-scoped changes after a monotonic cursor.
    pub async fn changes(
        &self,
        project_id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<SourceConnectionChange>> {
        let project_id = project_id.to_string();
        self.db
            .reader()
            .call(move |conn| read_changes(conn, &project_id, after, limit).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Creates one secret-free dedicated App provisioning checkpoint.
    pub async fn store_dedicated_provisioning(
        &self,
        input: StoreDedicatedProvisioning,
    ) -> Result<DedicatedProvisioning> {
        self.db
            .writer()
            .call(move |conn| store_dedicated_provisioning(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Reads one dedicated provisioning checkpoint inside its project boundary.
    pub async fn dedicated_provisioning(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<DedicatedProvisioning>> {
        let project_id = project_id.to_string();
        let id = id.to_string();
        self.db
            .reader()
            .call(move |conn| read_dedicated_provisioning(conn, &project_id, &id).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Resolves the encrypted exact App identity behind one active connection.
    pub async fn dedicated_app_identity_for_connection(
        &self,
        project_id: &str,
        connection_id: &str,
    ) -> Result<Option<DedicatedAppIdentityCredential>> {
        let project_id = project_id.to_string();
        let connection_id = connection_id.to_string();
        self.db
            .reader()
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
                        Ok(DedicatedAppIdentityCredential {
                            provisioning_id: row.get(0)?,
                            app_id_ciphertext: row.get(1)?,
                            app_id_digest: row.get(2)?,
                        })
                    },
                )
                .optional()
                .map_err(other)
            })
            .await
            .map_err(flatten_err)
    }

    /// Advances a dedicated checkpoint with an exact prior-state fence.
    pub async fn update_dedicated_provisioning(
        &self,
        input: UpdateDedicatedProvisioning,
    ) -> Result<DedicatedProvisioning> {
        self.db
            .writer()
            .call(move |conn| update_dedicated_provisioning(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }

    /// Updates dedicated App lifecycle metadata with the SourceConnection version fence.
    pub async fn update_dedicated_connection_lifecycle(
        &self,
        input: UpdateDedicatedConnectionLifecycle,
    ) -> Result<SourceConnection> {
        self.db
            .writer()
            .call(move |conn| update_dedicated_connection_lifecycle(conn, input).map_err(other))
            .await
            .map_err(flatten_err)
    }
}

fn valid_provision_status(value: &str) -> bool {
    matches!(
        value,
        "awaiting_approval"
            | "creating"
            | "handoff_pending"
            | "oauth_pending"
            | "attention"
            | "abandoned"
            | "completed"
    )
}

fn store_dedicated_provisioning(
    conn: &Connection,
    input: StoreDedicatedProvisioning,
) -> Result<DedicatedProvisioning> {
    for (label, value, max) in [
        ("provisioning ID", input.id.as_str(), 128),
        ("project", input.project_id.as_str(), 128),
        ("display label", input.display_label.as_str(), 128),
        ("owner daemon", input.owner_daemon_id.as_str(), 128),
        ("manifest version", input.manifest_version.as_str(), 64),
        ("manifest digest", input.manifest_digest.as_str(), 128),
        ("expiry", input.expires_at.as_str(), 64),
    ] {
        if value.trim().is_empty() || value.len() > max {
            bail!("dedicated Slack {label} must contain 1-{max} characters");
        }
    }
    let now = now_ts();
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
    read_dedicated_provisioning(conn, &input.project_id, &input.id)?
        .context("stored dedicated provisioning checkpoint missing")
}

fn update_dedicated_provisioning(
    conn: &Connection,
    input: UpdateDedicatedProvisioning,
) -> Result<DedicatedProvisioning> {
    if !valid_provision_status(&input.expected_status) || !valid_provision_status(&input.status) {
        bail!("invalid dedicated Slack provisioning state");
    }
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
            now_ts(),
        ],
    )?;
    if changed != 1 {
        bail!("dedicated Slack provisioning state conflict");
    }
    read_dedicated_provisioning(conn, &input.project_id, &input.id)?
        .context("updated dedicated provisioning checkpoint missing")
}

fn read_dedicated_provisioning(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<DedicatedProvisioning>> {
    conn.query_row(
        "SELECT id,project_id,display_label,owner_daemon_id,target_connection_id,status,
         manifest_version,manifest_digest,app_id_digest,oauth_intent_id,error_code,
         expires_at,created_at,updated_at
         FROM source_connection_provisioning WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            Ok(DedicatedProvisioning {
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

fn update_dedicated_connection_lifecycle(
    conn: &Connection,
    input: UpdateDedicatedConnectionLifecycle,
) -> Result<SourceConnection> {
    if input.expected_version < 1
        || input.manifest_version.trim().is_empty()
        || input.manifest_version.len() > 64
        || !matches!(
            input.provision_state.as_str(),
            "completed" | "reauthorization_required" | "app_deleted"
        )
        || input.request_id.trim().is_empty()
    {
        bail!("invalid dedicated Slack App lifecycle update");
    }
    let transaction = conn.unchecked_transaction()?;
    let now = now_ts();
    let changed = transaction.execute(
        "UPDATE source_connections SET state=?4,version=version+1,manifest_version=?5,
         provision_state=?6,provision_error_code=?7,last_error_code=?7,updated_at=?8
         WHERE id=?1 AND project_id=?2 AND version=?3
           AND provisioning_mode='managed_dedicated'",
        params![
            input.id,
            input.project_id,
            input.expected_version,
            input.state.as_str(),
            input.manifest_version,
            input.provision_state,
            input.error_code,
            now,
        ],
    )?;
    if changed != 1 {
        bail!("dedicated SourceConnection version or mode conflict");
    }
    append_change(
        &transaction,
        &input.id,
        &input.project_id,
        input.expected_version + 1,
        input.state,
        input.error_code.as_deref(),
        Some(&input.request_id),
        &now,
    )?;
    transaction.commit()?;
    read_connection(conn, &input.project_id, &input.id)?
        .context("updated dedicated SourceConnection missing")
}

fn store_intent(
    conn: &Connection,
    input: StoreSourceConnectionIntent,
) -> Result<SourceConnectionIntent> {
    for (label, value) in [
        ("intent ID", input.id.as_str()),
        ("project", input.project_id.as_str()),
        ("display label", input.display_label.as_str()),
        ("owner daemon", input.owner_daemon_id.as_str()),
        ("actor digest", input.actor_digest.as_str()),
        ("Gateway intent ID", input.gateway_intent_id.as_str()),
        (
            "authorize envelope",
            input.authorize_url_ciphertext.as_str(),
        ),
        ("poll envelope", input.poll_secret_ciphertext.as_str()),
        ("expiry", input.expires_at.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("SourceConnection {label} cannot be empty");
        }
    }
    if !matches!(
        input.provisioning_mode,
        SourceConnectionMode::ManagedShared | SourceConnectionMode::ManagedDedicated
    ) {
        bail!("manual SourceConnection does not use managed OAuth intents");
    }
    let now = now_ts();
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
            input.provisioning_mode.as_str(),
            input.owner_daemon_id,
            input.actor_digest,
            input.gateway_intent_id,
            input.authorize_url_ciphertext,
            input.poll_secret_ciphertext,
            input.expires_at,
            now,
        ],
    )?;
    read_intent(conn, &input.project_id, &input.id)?.context("stored intent missing")
}

fn read_intent(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<SourceConnectionIntent>> {
    conn.query_row(
        "SELECT id,project_id,provider,provisioning_mode,status,connection_id,error_code,
         expires_at,created_at,updated_at FROM source_connection_intents
         WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        },
    )
    .optional()?
    .map(
        |(
            id,
            project_id,
            provider,
            mode,
            status,
            connection_id,
            error_code,
            expires,
            created,
            updated,
        )| {
            Ok(SourceConnectionIntent {
                id,
                project_id,
                provider,
                provisioning_mode: SourceConnectionMode::parse(&mode)?,
                status,
                connection_id,
                error_code,
                expires_at: expires,
                created_at: created,
                updated_at: updated,
            })
        },
    )
    .transpose()
}

fn read_intent_credential(
    conn: &Connection,
    project_id: &str,
    id: &str,
    owner_daemon_id: &str,
) -> Result<Option<SourceConnectionIntentCredential>> {
    let Some(intent) = read_intent(conn, project_id, id)? else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT gateway_intent_id,authorize_url_ciphertext,poll_secret_ciphertext,owner_daemon_id,
            display_label
         FROM source_connection_intents WHERE id=?1 AND project_id=?2 AND owner_daemon_id=?3",
        params![id, project_id, owner_daemon_id],
        |row| {
            Ok(SourceConnectionIntentCredential {
                intent: intent.clone(),
                gateway_intent_id: row.get(0)?,
                authorize_url_ciphertext: row.get(1)?,
                poll_secret_ciphertext: row.get(2)?,
                owner_daemon_id: row.get(3)?,
                display_label: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

fn daemon_id(conn: &Connection) -> Result<String> {
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
    let value = format!("daemon-{}", Uuid::new_v4());
    conn.execute(
        "INSERT OR IGNORE INTO source_daemon_identity(singleton,daemon_id,created_at)
         VALUES(1,?1,?2)",
        params![value, now_ts()],
    )?;
    conn.query_row(
        "SELECT daemon_id FROM source_daemon_identity WHERE singleton=1",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn activate(conn: &Connection, input: ActivateSourceConnection) -> Result<SourceConnection> {
    validate_activation(&input)?;
    let transaction = conn.unchecked_transaction()?;
    let now = now_ts();
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
    let version = if let Some((id, project, owner, generation, version, state)) = existing {
        if project != input.project_id || owner != input.owner_daemon_id || id != input.id {
            bail!("SourceConnection installation already has another owner");
        }
        if input.generation < generation || input.version < version {
            bail!("SourceConnection credential generation or version is stale");
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
                input.provisioning_mode.as_str(),
                input.app_ownership,
                input.app_id_digest,
                input.manifest_version,
                input.provision_state,
                input.provision_error_code,
                input.generation,
                input.version,
                serde_json::to_string(&input.capabilities)?,
                serde_json::to_string(&input.scopes)?,
                input.trigger_name,
                input.gateway_origin,
                input.pairing_secret_ciphertext,
                input.last_acked_cursor,
                now,
            ],
        )?;
        if changed != 1 || state == "disconnected" {
            bail!("SourceConnection reauthorization conflict");
        }
        input.version
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
                input.provisioning_mode.as_str(),
                input.installation_id,
                input.installation_id_digest,
                input.enterprise_id_digest,
                input.owner_daemon_id,
                input.generation,
                input.version,
                serde_json::to_string(&input.capabilities)?,
                serde_json::to_string(&input.scopes)?,
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
        input.version
    };
    append_change(
        &transaction,
        &input.id,
        &input.project_id,
        version,
        SourceConnectionState::Active,
        None,
        Some(&input.request_id),
        &now,
    )?;
    transaction.commit()?;
    read_connection(conn, &input.project_id, &input.id)?
        .context("activated SourceConnection missing")
}

fn validate_activation(input: &ActivateSourceConnection) -> Result<()> {
    for (label, value, max) in [
        ("id", input.id.as_str(), 128),
        ("project", input.project_id.as_str(), 128),
        ("provider", input.provider.as_str(), 64),
        ("display label", input.display_label.as_str(), 128),
        ("installation ID", input.installation_id.as_str(), 128),
        (
            "installation digest",
            input.installation_id_digest.as_str(),
            128,
        ),
        ("owner daemon", input.owner_daemon_id.as_str(), 128),
        ("request ID", input.request_id.as_str(), 128),
    ] {
        if value.trim().is_empty() || value.len() > max {
            bail!("SourceConnection {label} must contain 1-{max} characters");
        }
    }
    if !matches!(
        input.app_ownership.as_str(),
        "orchestrator" | "workspace" | "external"
    ) {
        bail!("invalid SourceConnection App ownership");
    }
    if input.provisioning_mode == SourceConnectionMode::ManagedDedicated
        && (input.app_ownership != "workspace"
            || input.app_id_digest.is_none()
            || input.manifest_version.is_none())
    {
        bail!("managed_dedicated requires workspace App identity and manifest version");
    }
    if input.provisioning_mode != SourceConnectionMode::Manual
        && (input.gateway_origin.is_none() || input.pairing_secret_ciphertext.is_none())
    {
        bail!("managed SourceConnection requires encrypted Gateway pairing material");
    }
    if input.generation < 1 || input.version < 1 || input.last_acked_cursor < 0 {
        bail!("SourceConnection generation and version must be positive");
    }
    Ok(())
}

fn transition(
    conn: &Connection,
    project_id: &str,
    id: &str,
    expected_version: i64,
    state: SourceConnectionState,
    error_code: Option<&str>,
    request_id: &str,
) -> Result<SourceConnection> {
    if expected_version < 1 || request_id.trim().is_empty() {
        bail!("SourceConnection transition requires version and request ID");
    }
    let transaction = conn.unchecked_transaction()?;
    let now = now_ts();
    let clear_credentials = state == SourceConnectionState::Disconnected;
    let changed = transaction.execute(
        "UPDATE source_connections SET state=?4,version=version+1,last_error_code=?5,
         pairing_secret_ciphertext=CASE WHEN ?6 THEN NULL ELSE pairing_secret_ciphertext END,
         disconnected_at=CASE WHEN ?6 THEN ?7 ELSE disconnected_at END,updated_at=?7
         WHERE id=?1 AND project_id=?2 AND version=?3",
        params![
            id,
            project_id,
            expected_version,
            state.as_str(),
            error_code,
            clear_credentials,
            now,
        ],
    )?;
    if changed != 1 {
        bail!("SourceConnection version conflict or project boundary mismatch");
    }
    let version = expected_version + 1;
    append_change(
        &transaction,
        id,
        project_id,
        version,
        state,
        error_code,
        Some(request_id),
        &now,
    )?;
    transaction.commit()?;
    read_connection(conn, project_id, id)?.context("transitioned SourceConnection missing")
}

fn read_connection(
    conn: &Connection,
    project_id: &str,
    id: &str,
) -> Result<Option<SourceConnection>> {
    conn.query_row(
        "SELECT id,project_id,provider,display_label,provisioning_mode,installation_id,
         installation_id_digest,enterprise_id_digest,owner_daemon_id,generation,version,state,
         capabilities_json,scopes_json,trigger_name,last_delivery_at,last_acked_cursor,
         delivery_lag,last_error_code,created_at,updated_at,reauthorized_at,disconnected_at,
         app_ownership,app_id_digest,manifest_version,provision_state,provision_error_code
         FROM source_connections WHERE id=?1 AND project_id=?2",
        params![id, project_id],
        |row| {
            let mode = row.get::<_, String>(4)?;
            let state = row.get::<_, String>(11)?;
            let capabilities = row.get::<_, String>(12)?;
            let scopes = row.get::<_, String>(13)?;
            Ok((mode, state, capabilities, scopes, row_to_fields(row)?))
        },
    )
    .optional()?
    .map(|(mode, state, capabilities, scopes, fields)| {
        let mut connection = fields;
        connection.provisioning_mode = SourceConnectionMode::parse(&mode)?;
        connection.state = SourceConnectionState::parse(&state)?;
        connection.capabilities = serde_json::from_str(&capabilities)?;
        connection.scopes = serde_json::from_str(&scopes)?;
        Ok(connection)
    })
    .transpose()
}

fn row_to_fields(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceConnection> {
    Ok(SourceConnection {
        id: row.get(0)?,
        project_id: row.get(1)?,
        provider: row.get(2)?,
        display_label: row.get(3)?,
        provisioning_mode: SourceConnectionMode::Manual,
        app_ownership: row.get(23)?,
        app_id_digest: row.get(24)?,
        manifest_version: row.get(25)?,
        provision_state: row.get(26)?,
        provision_error_code: row.get(27)?,
        installation_id: row.get(5)?,
        installation_id_digest: row.get(6)?,
        enterprise_id_digest: row.get(7)?,
        owner_daemon_id: row.get(8)?,
        generation: row.get(9)?,
        version: row.get(10)?,
        state: SourceConnectionState::Connecting,
        capabilities: vec![],
        scopes: vec![],
        trigger_name: row.get(14)?,
        last_delivery_at: row.get(15)?,
        last_acked_cursor: row.get(16)?,
        delivery_lag: row.get(17)?,
        last_error_code: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        reauthorized_at: row.get(21)?,
        disconnected_at: row.get(22)?,
    })
}

#[allow(clippy::too_many_arguments)]
fn append_change(
    conn: &Connection,
    id: &str,
    project_id: &str,
    version: i64,
    state: SourceConnectionState,
    error_code: Option<&str>,
    request_id: Option<&str>,
    created_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO source_connection_changes
         (connection_id,project_id,connection_version,state,error_code,request_id,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            id,
            project_id,
            version,
            state.as_str(),
            error_code,
            request_id,
            created_at,
        ],
    )?;
    Ok(())
}

fn read_changes(
    conn: &Connection,
    project_id: &str,
    after: i64,
    limit: usize,
) -> Result<Vec<SourceConnectionChange>> {
    let mut statement = conn.prepare(
        "SELECT id,connection_id,project_id,connection_version,state,error_code,request_id,created_at
         FROM source_connection_changes WHERE project_id=?1 AND id>?2 ORDER BY id LIMIT ?3",
    )?;
    let rows = statement
        .query_map(
            params![project_id, after.max(0), limit.clamp(1, 500)],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(cursor, connection_id, project_id, version, state, error_code, request_id, at)| {
                Ok(SourceConnectionChange {
                    cursor,
                    connection_id,
                    project_id,
                    connection_version: version,
                    state: SourceConnectionState::parse(&state)?,
                    error_code,
                    request_id,
                    created_at: at,
                })
            },
        )
        .collect()
}

fn other(error: impl Into<anyhow::Error>) -> tokio_rusqlite::Error {
    tokio_rusqlite::Error::Other(error.into().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;

    async fn repository() -> (tempfile::TempDir, AsyncSourceConnectionRepository) {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("connections.db");
        init_schema(&path).expect("schema");
        let database = Arc::new(AsyncDatabase::open(&path).await.expect("database"));
        (temp, AsyncSourceConnectionRepository::new(database))
    }

    fn activation(project: &str) -> ActivateSourceConnection {
        ActivateSourceConnection {
            id: "conn-install-1".into(),
            project_id: project.into(),
            provider: "slack".into(),
            display_label: "Slack workspace".into(),
            provisioning_mode: SourceConnectionMode::ManagedShared,
            app_ownership: "orchestrator".into(),
            app_id_digest: None,
            manifest_version: None,
            provision_state: None,
            provision_error_code: None,
            installation_id: "install-1".into(),
            installation_id_digest: "digest-team".into(),
            enterprise_id_digest: None,
            owner_daemon_id: "daemon-1".into(),
            generation: 1,
            version: 1,
            last_acked_cursor: 0,
            capabilities: vec!["permalink_proxy".into()],
            scopes: vec!["reactions:read".into()],
            trigger_name: Some("slack-conn-install-1".into()),
            gateway_origin: Some("https://gateway.example".into()),
            pairing_secret_ciphertext: Some("encrypted-pairing-envelope".into()),
            request_id: "req-connect-1".into(),
        }
    }

    #[tokio::test]
    async fn stable_daemon_identity_survives_repository_calls() {
        let (_temp, repository) = repository().await;
        let first = repository.daemon_id().await.expect("first");
        let second = repository.daemon_id().await.expect("second");
        assert_eq!(first, second);
        assert!(first.starts_with("daemon-"));
    }

    #[tokio::test]
    async fn projections_are_project_scoped_and_exclude_credentials() {
        let (_temp, repository) = repository().await;
        let connection = repository
            .activate(activation("project-a"))
            .await
            .expect("activate");
        assert_eq!(connection.state, SourceConnectionState::Active);
        assert!(
            repository
                .get("project-b", &connection.id)
                .await
                .expect("cross project")
                .is_none()
        );
        let encoded = serde_json::to_string(&connection).expect("projection");
        assert!(!encoded.contains("encrypted-pairing-envelope"));
        assert!(!encoded.contains("gateway.example"));
    }

    #[tokio::test]
    async fn reauthorization_advances_generation_without_duplicate_connection() {
        let (_temp, repository) = repository().await;
        repository
            .activate(activation("project-a"))
            .await
            .expect("activate");
        let mut reauthorize = activation("project-a");
        reauthorize.generation = 2;
        reauthorize.version = 2;
        reauthorize.pairing_secret_ciphertext = Some("encrypted-generation-2".into());
        reauthorize.request_id = "req-reauthorize".into();
        let connection = repository.activate(reauthorize).await.expect("reauthorize");
        assert_eq!(connection.generation, 2);
        assert_eq!(connection.version, 2);
        assert_eq!(
            repository
                .list("project-a", None, false, 10)
                .await
                .expect("list")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn disconnect_is_cas_guarded_and_destroys_adapter_credential() {
        let (_temp, repository) = repository().await;
        let connection = repository
            .activate(activation("project-a"))
            .await
            .expect("activate");
        assert!(
            repository
                .transition(
                    "project-a",
                    &connection.id,
                    99,
                    SourceConnectionState::Disconnected,
                    None,
                    "req-stale",
                )
                .await
                .is_err()
        );
        let disconnected = repository
            .transition(
                "project-a",
                &connection.id,
                connection.version,
                SourceConnectionState::Disconnected,
                None,
                "req-disconnect",
            )
            .await
            .expect("disconnect");
        assert_eq!(disconnected.state, SourceConnectionState::Disconnected);
        assert!(
            repository
                .credential("project-a", &connection.id, "daemon-1")
                .await
                .expect("credential")
                .is_none()
        );
        let changes = repository
            .changes("project-a", 0, 10)
            .await
            .expect("changes");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[1].request_id.as_deref(), Some("req-disconnect"));
    }

    #[tokio::test]
    async fn owner_and_generation_conflicts_fail_closed() {
        let (_temp, repository) = repository().await;
        repository
            .activate(activation("project-a"))
            .await
            .expect("activate");
        let mut conflict = activation("project-b");
        conflict.owner_daemon_id = "daemon-2".into();
        assert!(repository.activate(conflict).await.is_err());
        let mut stale = activation("project-a");
        stale.generation = 0;
        assert!(repository.activate(stale).await.is_err());
    }

    #[tokio::test]
    async fn ownership_transfer_is_cas_guarded_and_fences_the_old_daemon() {
        let (_temp, repository) = repository().await;
        let active = repository
            .activate(activation("project-a"))
            .await
            .expect("activate");
        assert!(
            repository
                .transfer_owner(TransferSourceConnectionOwner {
                    project_id: "project-a".into(),
                    id: active.id.clone(),
                    expected_version: 99,
                    target_daemon_id: "daemon-2".into(),
                    generation: 1,
                    request_id: "req-stale-transfer".into(),
                })
                .await
                .is_err()
        );
        let transferred = repository
            .transfer_owner(TransferSourceConnectionOwner {
                project_id: "project-a".into(),
                id: active.id.clone(),
                expected_version: active.version,
                target_daemon_id: "daemon-2".into(),
                generation: 1,
                request_id: "req-transfer".into(),
            })
            .await
            .expect("transfer");
        assert_eq!(transferred.owner_daemon_id, "daemon-2");
        assert_eq!(transferred.state, SourceConnectionState::Suspended);
        assert_eq!(transferred.version, active.version + 1);
        assert!(
            repository
                .credential("project-a", &active.id, "daemon-1")
                .await
                .expect("old credential")
                .is_none()
        );
        let changes = repository
            .changes("project-a", 0, 10)
            .await
            .expect("changes");
        assert_eq!(
            changes
                .last()
                .and_then(|change| change.request_id.as_deref()),
            Some("req-transfer")
        );
    }

    #[tokio::test]
    async fn oauth_intent_preserves_label_only_in_the_internal_record() {
        let (_temp, repository) = repository().await;
        let stored = repository
            .store_intent(StoreSourceConnectionIntent {
                id: "intent-1".into(),
                project_id: "project-a".into(),
                provider: "slack".into(),
                display_label: "Engineering Slack".into(),
                provisioning_mode: SourceConnectionMode::ManagedShared,
                owner_daemon_id: "daemon-1".into(),
                actor_digest: "actor-digest".into(),
                gateway_intent_id: "gateway-intent-1".into(),
                authorize_url_ciphertext: "encrypted-authorize-url".into(),
                poll_secret_ciphertext: "encrypted-poll-secret".into(),
                expires_at: "2026-07-18T01:00:00Z".into(),
            })
            .await
            .expect("store intent");
        assert!(
            !serde_json::to_string(&stored)
                .expect("serialize")
                .contains("Engineering Slack")
        );
        let internal = repository
            .intent_credential("project-a", "intent-1", "daemon-1")
            .await
            .expect("read")
            .expect("intent");
        assert_eq!(internal.display_label, "Engineering Slack");
    }

    #[tokio::test]
    async fn dedicated_app_identity_is_resolved_only_from_its_completed_connection() {
        let (_temp, repository) = repository().await;
        repository
            .store_dedicated_provisioning(StoreDedicatedProvisioning {
                id: "dedicated-1".into(),
                project_id: "project-a".into(),
                display_label: "Private Slack".into(),
                owner_daemon_id: "daemon-1".into(),
                target_connection_id: None,
                manifest_version: "dedicated-v1".into(),
                manifest_digest: "manifest-digest".into(),
                expires_at: "2026-07-19T01:00:00Z".into(),
            })
            .await
            .expect("store provisioning");
        repository
            .store_intent(StoreSourceConnectionIntent {
                id: "intent-dedicated-1".into(),
                project_id: "project-a".into(),
                provider: "slack".into(),
                display_label: "Private Slack".into(),
                provisioning_mode: SourceConnectionMode::ManagedDedicated,
                owner_daemon_id: "daemon-1".into(),
                actor_digest: "actor-digest".into(),
                gateway_intent_id: "gateway-intent-dedicated-1".into(),
                authorize_url_ciphertext: "encrypted-authorize-url".into(),
                poll_secret_ciphertext: "encrypted-poll-secret".into(),
                expires_at: "2026-07-19T01:00:00Z".into(),
            })
            .await
            .expect("store intent");
        repository
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: "project-a".into(),
                id: "dedicated-1".into(),
                expected_status: "awaiting_approval".into(),
                status: "oauth_pending".into(),
                app_id_ciphertext: Some("encrypted-app-id".into()),
                app_id_digest: Some("app-id-digest".into()),
                oauth_intent_id: Some("intent-dedicated-1".into()),
                error_code: None,
            })
            .await
            .expect("link intent");
        let mut dedicated = activation("project-a");
        dedicated.provisioning_mode = SourceConnectionMode::ManagedDedicated;
        dedicated.app_ownership = "workspace".into();
        dedicated.app_id_digest = Some("app-id-digest".into());
        dedicated.manifest_version = Some("dedicated-v1".into());
        repository
            .activate(dedicated)
            .await
            .expect("activate dedicated connection");
        repository
            .complete_intent(
                "project-a",
                "intent-dedicated-1",
                "completed",
                Some("conn-install-1"),
                None,
            )
            .await
            .expect("complete intent");
        repository
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: "project-a".into(),
                id: "dedicated-1".into(),
                expected_status: "oauth_pending".into(),
                status: "completed".into(),
                app_id_ciphertext: None,
                app_id_digest: None,
                oauth_intent_id: None,
                error_code: None,
            })
            .await
            .expect("complete provisioning");

        let identity = repository
            .dedicated_app_identity_for_connection("project-a", "conn-install-1")
            .await
            .expect("read identity")
            .expect("identity");
        assert_eq!(identity.provisioning_id, "dedicated-1");
        assert_eq!(identity.app_id_ciphertext, "encrypted-app-id");
        assert_eq!(identity.app_id_digest, "app-id-digest");
        assert!(
            repository
                .dedicated_app_identity_for_connection("project-b", "conn-install-1")
                .await
                .expect("cross-project read")
                .is_none()
        );

        let suspended = repository
            .update_dedicated_connection_lifecycle(UpdateDedicatedConnectionLifecycle {
                project_id: "project-a".into(),
                id: "conn-install-1".into(),
                expected_version: 1,
                state: SourceConnectionState::Suspended,
                manifest_version: "dedicated-v2".into(),
                provision_state: "reauthorization_required".into(),
                error_code: Some("slack_manifest_reauthorization_required".into()),
                request_id: "req-upgrade".into(),
            })
            .await
            .expect("update lifecycle");
        assert_eq!(suspended.version, 2);
        assert_eq!(suspended.state, SourceConnectionState::Suspended);
        assert_eq!(suspended.manifest_version.as_deref(), Some("dedicated-v2"));
        assert_eq!(
            suspended.provision_state.as_deref(),
            Some("reauthorization_required")
        );
        assert!(
            repository
                .update_dedicated_connection_lifecycle(UpdateDedicatedConnectionLifecycle {
                    project_id: "project-a".into(),
                    id: "conn-install-1".into(),
                    expected_version: 1,
                    state: SourceConnectionState::Active,
                    manifest_version: "dedicated-v2".into(),
                    provision_state: "completed".into(),
                    error_code: None,
                    request_id: "req-stale-upgrade".into(),
                })
                .await
                .is_err()
        );
    }
}
