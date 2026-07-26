//! Durable provider connection lifecycle with project-scoped, secret-free projections.
//!
//! The five tables and their statements live in
//! `orchestrator_persistence::source_connections` (FR-130 B13). What stays here
//! is the contract: field bounds, which provisioning modes may hold a managed
//! OAuth intent, which terminal statuses a caller may ask for, what each refused
//! fence means to an operator, and how a stored mode/state string and the
//! capability and scope JSON become typed values.

use crate::async_database::AsyncDatabase;
use crate::config_load::now_ts;
use anyhow::{Context, Result, bail};
use orchestrator_persistence::source_connections as store;
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
        store::daemon_id(&self.db, format!("daemon-{}", Uuid::new_v4()), now_ts()).await
    }

    /// Activates or idempotently reauthorizes one verified installation.
    pub async fn activate(&self, input: ActivateSourceConnection) -> Result<SourceConnection> {
        validate_activation(&input)?;
        let outcome = store::activate(
            &self.db,
            store::NewActivation {
                id: input.id,
                project_id: input.project_id,
                provider: input.provider,
                display_label: input.display_label,
                provisioning_mode: input.provisioning_mode.as_str().to_string(),
                installation_id: input.installation_id,
                installation_id_digest: input.installation_id_digest,
                enterprise_id_digest: input.enterprise_id_digest,
                owner_daemon_id: input.owner_daemon_id,
                generation: input.generation,
                version: input.version,
                capabilities_json: serde_json::to_string(&input.capabilities)?,
                scopes_json: serde_json::to_string(&input.scopes)?,
                trigger_name: input.trigger_name,
                gateway_origin: input.gateway_origin,
                pairing_secret_ciphertext: input.pairing_secret_ciphertext,
                last_acked_cursor: input.last_acked_cursor,
                app_ownership: input.app_ownership,
                app_id_digest: input.app_id_digest,
                manifest_version: input.manifest_version,
                provision_state: input.provision_state,
                provision_error_code: input.provision_error_code,
                request_id: input.request_id,
            },
            now_ts(),
        )
        .await?;
        match outcome {
            store::Activation::Created(row) | store::Activation::Reauthorized(row) => {
                connection_from_row(row)
            }
            store::Activation::OwnerConflict => {
                bail!("SourceConnection installation already has another owner")
            }
            store::Activation::StaleFence => {
                bail!("SourceConnection credential generation or version is stale")
            }
            store::Activation::ReauthorizationConflict => {
                bail!("SourceConnection reauthorization conflict")
            }
        }
    }

    /// Gets one connection only when it belongs to the requested project.
    pub async fn get(&self, project_id: &str, id: &str) -> Result<Option<SourceConnection>> {
        store::read_connection(&self.db, project_id.to_string(), id.to_string())
            .await?
            .map(connection_from_row)
            .transpose()
    }

    /// Lists connections inside one project boundary.
    pub async fn list(
        &self,
        project_id: &str,
        provider: Option<&str>,
        include_disconnected: bool,
        limit: usize,
    ) -> Result<Vec<SourceConnection>> {
        store::list_connections(
            &self.db,
            project_id.to_string(),
            provider.map(str::to_string),
            include_disconnected,
            limit,
        )
        .await?
        .into_iter()
        .map(connection_from_row)
        .collect()
    }

    /// Returns encrypted adapter credentials after project and owner fences pass.
    pub async fn credential(
        &self,
        project_id: &str,
        id: &str,
        owner_daemon_id: &str,
    ) -> Result<Option<SourceConnectionCredential>> {
        Ok(store::read_credential(
            &self.db,
            project_id.to_string(),
            id.to_string(),
            owner_daemon_id.to_string(),
        )
        .await?
        .map(|row| SourceConnectionCredential {
            installation_id: row.installation_id,
            owner_daemon_id: row.owner_daemon_id,
            generation: row.generation,
            gateway_origin: row.gateway_origin,
            pairing_secret_ciphertext: row.pairing_secret_ciphertext,
        }))
    }

    /// Stores a resumable OAuth intent without exposing state or polling credentials.
    pub async fn store_intent(
        &self,
        input: StoreSourceConnectionIntent,
    ) -> Result<SourceConnectionIntent> {
        validate_intent(&input)?;
        let row = store::store_intent(
            &self.db,
            store::NewIntent {
                id: input.id,
                project_id: input.project_id,
                provider: input.provider,
                display_label: input.display_label,
                provisioning_mode: input.provisioning_mode.as_str().to_string(),
                owner_daemon_id: input.owner_daemon_id,
                actor_digest: input.actor_digest,
                gateway_intent_id: input.gateway_intent_id,
                authorize_url_ciphertext: input.authorize_url_ciphertext,
                poll_secret_ciphertext: input.poll_secret_ciphertext,
                expires_at: input.expires_at,
            },
            now_ts(),
        )
        .await?;
        intent_from_row(row)
    }

    /// Reads one encrypted intent only within its project and daemon owner boundary.
    pub async fn intent_credential(
        &self,
        project_id: &str,
        id: &str,
        owner_daemon_id: &str,
    ) -> Result<Option<SourceConnectionIntentCredential>> {
        let Some((intent, credential)) = store::read_intent_credential(
            &self.db,
            project_id.to_string(),
            id.to_string(),
            owner_daemon_id.to_string(),
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(Some(SourceConnectionIntentCredential {
            intent: intent_from_row(intent)?,
            gateway_intent_id: credential.gateway_intent_id,
            authorize_url_ciphertext: credential.authorize_url_ciphertext,
            poll_secret_ciphertext: credential.poll_secret_ciphertext,
            owner_daemon_id: credential.owner_daemon_id,
            display_label: credential.display_label,
        }))
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
        let row = store::complete_intent(
            &self.db,
            project_id.to_string(),
            id.to_string(),
            status.to_string(),
            connection_id.map(str::to_string),
            error_code.map(str::to_string),
            now_ts(),
        )
        .await?
        .context("SourceConnection intent is not pending or project does not match")?;
        intent_from_row(row)
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
        if expected_version < 1 || request_id.trim().is_empty() {
            bail!("SourceConnection transition requires version and request ID");
        }
        let row = store::transition(
            &self.db,
            project_id.to_string(),
            id.to_string(),
            expected_version,
            state.as_str().to_string(),
            error_code.map(str::to_string),
            request_id.to_string(),
            now_ts(),
        )
        .await?
        .context("SourceConnection version conflict or project boundary mismatch")?;
        connection_from_row(row)
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
        let row = store::transfer_owner(
            &self.db,
            store::OwnerTransfer {
                id: input.id,
                project_id: input.project_id,
                expected_version: input.expected_version,
                target_daemon_id: input.target_daemon_id,
                generation: input.generation,
                request_id: input.request_id,
            },
            now_ts(),
        )
        .await?
        .context("SourceConnection transfer version conflict or connection inactive")?;
        connection_from_row(row)
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
        if !store::record_delivery(
            &self.db,
            project_id.to_string(),
            id.to_string(),
            cursor,
            lag,
            now_ts(),
        )
        .await?
        {
            bail!("SourceConnection delivery cursor is stale or connection inactive");
        }
        Ok(())
    }

    /// Reads project-scoped changes after a monotonic cursor.
    pub async fn changes(
        &self,
        project_id: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<SourceConnectionChange>> {
        store::read_changes(&self.db, project_id.to_string(), after, limit)
            .await?
            .into_iter()
            .map(|row| {
                Ok(SourceConnectionChange {
                    cursor: row.cursor,
                    connection_id: row.connection_id,
                    project_id: row.project_id,
                    connection_version: row.connection_version,
                    state: SourceConnectionState::parse(&row.state)?,
                    error_code: row.error_code,
                    request_id: row.request_id,
                    created_at: row.created_at,
                })
            })
            .collect()
    }

    /// Creates one secret-free dedicated App provisioning checkpoint.
    pub async fn store_dedicated_provisioning(
        &self,
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
        Ok(provisioning_from_row(
            store::store_provisioning(
                &self.db,
                store::NewProvisioning {
                    id: input.id,
                    project_id: input.project_id,
                    display_label: input.display_label,
                    owner_daemon_id: input.owner_daemon_id,
                    target_connection_id: input.target_connection_id,
                    manifest_version: input.manifest_version,
                    manifest_digest: input.manifest_digest,
                    expires_at: input.expires_at,
                },
                now_ts(),
            )
            .await?,
        ))
    }

    /// Reads one dedicated provisioning checkpoint inside its project boundary.
    pub async fn dedicated_provisioning(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<Option<DedicatedProvisioning>> {
        Ok(
            store::read_provisioning(&self.db, project_id.to_string(), id.to_string())
                .await?
                .map(provisioning_from_row),
        )
    }

    /// Resolves the encrypted exact App identity behind one active connection.
    pub async fn dedicated_app_identity_for_connection(
        &self,
        project_id: &str,
        connection_id: &str,
    ) -> Result<Option<DedicatedAppIdentityCredential>> {
        Ok(
            store::read_app_identity(&self.db, project_id.to_string(), connection_id.to_string())
                .await?
                .map(|row| DedicatedAppIdentityCredential {
                    provisioning_id: row.provisioning_id,
                    app_id_ciphertext: row.app_id_ciphertext,
                    app_id_digest: row.app_id_digest,
                }),
        )
    }

    /// Advances a dedicated checkpoint with an exact prior-state fence.
    pub async fn update_dedicated_provisioning(
        &self,
        input: UpdateDedicatedProvisioning,
    ) -> Result<DedicatedProvisioning> {
        if !valid_provision_status(&input.expected_status) || !valid_provision_status(&input.status)
        {
            bail!("invalid dedicated Slack provisioning state");
        }
        let row = store::update_provisioning(
            &self.db,
            store::ProvisioningUpdate {
                id: input.id,
                project_id: input.project_id,
                expected_status: input.expected_status,
                status: input.status,
                app_id_ciphertext: input.app_id_ciphertext,
                app_id_digest: input.app_id_digest,
                oauth_intent_id: input.oauth_intent_id,
                error_code: input.error_code,
            },
            now_ts(),
        )
        .await?
        .context("dedicated Slack provisioning state conflict")?;
        Ok(provisioning_from_row(row))
    }

    /// Updates dedicated App lifecycle metadata with the SourceConnection version fence.
    pub async fn update_dedicated_connection_lifecycle(
        &self,
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
        let row = store::update_dedicated_lifecycle(
            &self.db,
            store::LifecycleUpdate {
                id: input.id,
                project_id: input.project_id,
                expected_version: input.expected_version,
                state: input.state.as_str().to_string(),
                manifest_version: input.manifest_version,
                provision_state: input.provision_state,
                error_code: input.error_code,
                request_id: input.request_id,
            },
            now_ts(),
        )
        .await?
        .context("dedicated SourceConnection version or mode conflict")?;
        connection_from_row(row)
    }
}

/// Turns a stored row into the typed connection.
///
/// The mode, the state and the two JSON columns are parsed here rather than in
/// the row mapper below the boundary, where a `serde_json` failure would have to
/// be reported as a column-conversion failure against an invented column index.
fn connection_from_row(row: store::SourceConnectionRow) -> Result<SourceConnection> {
    Ok(SourceConnection {
        provisioning_mode: SourceConnectionMode::parse(&row.provisioning_mode)?,
        state: SourceConnectionState::parse(&row.state)?,
        capabilities: serde_json::from_str(&row.capabilities_json)
            .with_context(|| format!("parse stored capabilities of SourceConnection {}", row.id))?,
        scopes: serde_json::from_str(&row.scopes_json)
            .with_context(|| format!("parse stored scopes of SourceConnection {}", row.id))?,
        id: row.id,
        project_id: row.project_id,
        provider: row.provider,
        display_label: row.display_label,
        app_ownership: row.app_ownership,
        app_id_digest: row.app_id_digest,
        manifest_version: row.manifest_version,
        provision_state: row.provision_state,
        provision_error_code: row.provision_error_code,
        installation_id: row.installation_id,
        installation_id_digest: row.installation_id_digest,
        enterprise_id_digest: row.enterprise_id_digest,
        owner_daemon_id: row.owner_daemon_id,
        generation: row.generation,
        version: row.version,
        trigger_name: row.trigger_name,
        last_delivery_at: row.last_delivery_at,
        last_acked_cursor: row.last_acked_cursor,
        delivery_lag: row.delivery_lag,
        last_error_code: row.last_error_code,
        created_at: row.created_at,
        updated_at: row.updated_at,
        reauthorized_at: row.reauthorized_at,
        disconnected_at: row.disconnected_at,
    })
}

fn intent_from_row(row: store::IntentRow) -> Result<SourceConnectionIntent> {
    Ok(SourceConnectionIntent {
        provisioning_mode: SourceConnectionMode::parse(&row.provisioning_mode)?,
        id: row.id,
        project_id: row.project_id,
        provider: row.provider,
        status: row.status,
        connection_id: row.connection_id,
        error_code: row.error_code,
        expires_at: row.expires_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn provisioning_from_row(row: store::ProvisioningRow) -> DedicatedProvisioning {
    DedicatedProvisioning {
        id: row.id,
        project_id: row.project_id,
        display_label: row.display_label,
        owner_daemon_id: row.owner_daemon_id,
        target_connection_id: row.target_connection_id,
        status: row.status,
        manifest_version: row.manifest_version,
        manifest_digest: row.manifest_digest,
        app_id_digest: row.app_id_digest,
        oauth_intent_id: row.oauth_intent_id,
        error_code: row.error_code,
        expires_at: row.expires_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// The contract an OAuth intent must satisfy before it is worth storing.
fn validate_intent(input: &StoreSourceConnectionIntent) -> Result<()> {
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
    Ok(())
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
