use orchestrator_proto::OrchestratorServiceClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

use crate::client::{self, TransportKind};

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Cached gRPC channel (lazy init, supports reconnect).
    channel: Arc<RwLock<Option<Channel>>>,
    /// Transport kind of the current connection.
    transport: Arc<RwLock<Option<TransportKind>>>,
    /// Active streaming subscriptions keyed by stream ID.
    active_streams: Arc<RwLock<HashMap<String, CancellationToken>>>,
    /// Cached RBAC role for the current connection.
    role: Arc<RwLock<Option<String>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            channel: Arc::new(RwLock::new(None)),
            transport: Arc::new(RwLock::new(None)),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            role: Arc::new(RwLock::new(None)),
        }
    }

    /// Connect to the daemon, replacing any existing connection.
    pub async fn connect(&self, config: Option<&str>) -> Result<(), String> {
        let (channel, transport) = client::connect(config)
            .await
            .map_err(|e| format!("{e:#}"))?;
        *self.channel.write().await = Some(channel);
        *self.transport.write().await = Some(transport);
        // Clear cached role on reconnect.
        *self.role.write().await = None;
        Ok(())
    }

    /// Get a gRPC client from the cached channel.
    pub async fn client(&self) -> Result<OrchestratorServiceClient<Channel>, String> {
        let guard = self.channel.read().await;
        let channel = guard
            .as_ref()
            .ok_or_else(|| "not connected to daemon".to_string())?
            .clone();
        Ok(
            OrchestratorServiceClient::new(channel)
                .max_decoding_message_size(client::max_decode_size()),
        )
    }

    /// Get the transport kind of the current connection.
    pub async fn transport_kind(&self) -> Option<TransportKind> {
        *self.transport.read().await
    }

    /// Register a streaming subscription and return its cancellation token.
    pub async fn register_stream(&self, key: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.active_streams
            .write()
            .await
            .insert(key.to_string(), token.clone());
        token
    }

    /// Cancel and remove a streaming subscription.
    pub async fn cancel_stream(&self, key: &str) {
        if let Some(token) = self.active_streams.write().await.remove(key) {
            token.cancel();
        }
    }

    /// Cache the probed RBAC role.
    pub async fn set_role(&self, role: String) {
        *self.role.write().await = Some(role);
    }

    /// Get the cached RBAC role.
    pub async fn get_role(&self) -> Option<String> {
        self.role.read().await.clone()
    }
}
