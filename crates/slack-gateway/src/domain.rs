//! Secret-free gateway domain types shared by API and persistence layers.

use serde::{Deserialize, Serialize};

/// Stable provisioning modes understood by daemon and gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningMode {
    /// One official Orchestrator app installed into many workspaces.
    ManagedShared,
    /// Reserved for FR-115; never silently downgraded.
    ManagedDedicated,
    /// User-owned credentials managed outside the gateway.
    Manual,
}

impl ProvisioningMode {
    /// Stable protocol value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManagedShared => "managed_shared",
            Self::ManagedDedicated => "managed_dedicated",
            Self::Manual => "manual",
        }
    }
}

/// Managed connection lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// OAuth has not completed.
    Connecting,
    /// Delivery and provider proxy are available.
    Active,
    /// Human review is required.
    Attention,
    /// Operator-paused without credential destruction.
    Suspended,
    /// Provider revoked the installation.
    Revoked,
    /// Credentials were destroyed intentionally.
    Disconnected,
}

impl ConnectionState {
    /// Stable protocol value.
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
}

/// Public capability catalog used for fail-closed feature negotiation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayCapabilities {
    /// Protocol version implemented by this gateway.
    pub protocol_version: u32,
    /// Provisioning modes accepted for new connections.
    pub supported_modes: Vec<String>,
    /// Maximum claim batch size.
    pub max_delivery_batch: u32,
    /// Whether permalink proxy is enabled.
    pub permalink_proxy: bool,
}

impl Default for GatewayCapabilities {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            supported_modes: vec![
                ProvisioningMode::ManagedShared.as_str().into(),
                ProvisioningMode::ManagedDedicated.as_str().into(),
            ],
            max_delivery_batch: 100,
            permalink_proxy: true,
        }
    }
}

/// Safe connection projection returned to a paired daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallationProjection {
    /// Stable gateway installation ID.
    pub id: String,
    /// Non-reversible team identity.
    pub team_digest: String,
    /// Optional non-reversible Enterprise identity.
    pub enterprise_digest: Option<String>,
    /// Owning daemon identity.
    pub owner_daemon_id: String,
    /// Owning project identity.
    pub owner_project_id: String,
    /// App provisioning mode currently authoritative for this workspace.
    pub provisioning_mode: String,
    /// Dedicated App connection identity, absent for the official shared App.
    pub app_connection_id: Option<String>,
    /// Non-reversible Slack App identity digest.
    pub app_id_digest: Option<String>,
    /// Reviewed App Manifest profile version.
    pub manifest_version: Option<String>,
    /// Current credential generation.
    pub generation: i64,
    /// Optimistic concurrency version.
    pub version: i64,
    /// Lifecycle state.
    pub state: String,
    /// Granted OAuth scope names.
    pub scopes: Vec<String>,
    /// Last acknowledged delivery cursor.
    pub last_acked_cursor: i64,
    /// Last safe provider or delivery error code.
    pub last_error_code: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Allowlisted Slack event envelope persisted by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedSlackEvent {
    /// Slack event ID used for deduplication.
    pub external_event_id: String,
    /// Event kind accepted by the managed Slack adapter.
    pub event_type: String,
    /// Non-secret installation identity.
    pub installation_id: String,
    /// External actor ID for daemon-side role resolution.
    pub external_actor_id: Option<String>,
    /// Reaction name, when applicable.
    pub reaction: Option<String>,
    /// Slack channel ID, when applicable.
    pub channel_id: Option<String>,
    /// Slack message timestamp, when applicable.
    pub message_ts: Option<String>,
    /// Provider event timestamp.
    pub event_ts: String,
    /// Digest of the verified provider team identity.
    pub team_digest: String,
    /// Optional digest of the verified Enterprise identity.
    pub enterprise_digest: Option<String>,
}

/// Claimed delivery returned to one installation owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryProjection {
    /// Monotonic gateway cursor.
    pub cursor: i64,
    /// Opaque delivery ID.
    pub delivery_id: String,
    /// Normalized event.
    pub event: NormalizedSlackEvent,
    /// Lease expiration timestamp.
    pub lease_expires_at: String,
}

/// Durable ownership handoff revealed only to its enrolled target daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnershipTransferClaim {
    /// Safe installation projection after the owner CAS.
    pub installation: InstallationProjection,
    /// Replacement installation-scoped credential for the target daemon.
    pub pairing_secret: String,
}
