//! Managed SourceConnection gRPC surface and OAuth intent reconciliation.

use agent_orchestrator::attention::{AttentionCandidate, AttentionSeverity};
use agent_orchestrator::source_connection::{
    ActivateSourceConnection, AsyncSourceConnectionRepository,
    DedicatedProvisioning as CoreDedicatedProvisioning, SourceConnection as CoreConnection,
    SourceConnectionIntent as CoreIntent, SourceConnectionMode, SourceConnectionState,
    StoreDedicatedProvisioning, StoreSourceConnectionIntent, TransferSourceConnectionOwner,
    UpdateDedicatedConnectionLifecycle, UpdateDedicatedProvisioning,
};
use futures::Stream;
use orchestrator_proto::*;
use orchestrator_slack_gateway::slack::{render_manifest_endpoints, reviewed_manifest_contract};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

#[cfg(test)]
mod tests;

pub(crate) type SourceConnectionWatchStream =
    Pin<Box<dyn Stream<Item = Result<SourceConnectionDelta, Status>> + Send>>;

const DEDICATED_MANIFEST_VERSION: &str = "orchestrator-slack-dedicated-v1";

pub(crate) type DedicatedSessionStore = Arc<Mutex<HashMap<String, DedicatedSession>>>;
pub(crate) type DedicatedLifecycleSessionStore =
    Arc<Mutex<HashMap<String, DedicatedLifecycleSession>>>;

pub(crate) struct DedicatedSession {
    project_id: String,
    display_label: String,
    owner_daemon_id: String,
    manifest: serde_json::Value,
    manifest_digest: String,
    config_token: Zeroizing<String>,
    import_secret: Option<Zeroizing<String>>,
    created_credentials: Option<DedicatedCreatedCredentials>,
}

struct DedicatedCreatedCredentials {
    app_id: Zeroizing<String>,
    client_id: Zeroizing<String>,
    client_secret: Zeroizing<String>,
    signing_secret: Zeroizing<String>,
}

pub(crate) struct DedicatedLifecycleSession {
    project_id: String,
    connection_id: String,
    expected_version: i64,
    provisioning_id: String,
    app_id: Zeroizing<String>,
    app_id_digest: String,
    manifest: serde_json::Value,
    manifest_digest: String,
    diff: Vec<SourceConnectionManifestDiffEntry>,
    permission_expansion: bool,
    config_token: Zeroizing<String>,
    expires_at: String,
}

mod dedicated;
mod lifecycle;
mod oauth;
mod projection;
mod query;
mod transfer;

pub(crate) use dedicated::{
    dedicated_abandon, dedicated_approve, dedicated_get, dedicated_preview,
};
pub(crate) use lifecycle::{dedicated_delete, dedicated_upgrade_apply, dedicated_upgrade_preview};
pub(crate) use oauth::{cancel, connect, intent_get, migrate_to_shared, reauthorize};
pub(crate) use query::{catalog, get, list, watch};
pub(crate) use transfer::{disconnect, ensure_default_trigger, transfer};
