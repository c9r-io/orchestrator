use orchestrator_proto::OrchestratorServiceClient;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

use crate::client::{self, TransportKind};

/// Connection lifecycle states emitted to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32, max_attempts: u32 },
    Failed { message: String },
}

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
    /// Current connection state.
    connection_state: Arc<RwLock<ConnectionState>>,
    /// Tauri AppHandle for emitting events.
    app_handle: Arc<RwLock<Option<AppHandle>>>,
    /// Cancellation token for the heartbeat task.
    heartbeat_cancel: CancellationToken,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            channel: Arc::new(RwLock::new(None)),
            transport: Arc::new(RwLock::new(None)),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            role: Arc::new(RwLock::new(None)),
            connection_state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            app_handle: Arc::new(RwLock::new(None)),
            heartbeat_cancel: CancellationToken::new(),
        }
    }

    /// Store the AppHandle for event emission (called during Tauri setup).
    pub async fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.write().await = Some(handle);
    }

    /// Emit the current connection state to the frontend.
    async fn emit_connection_state(&self) {
        let state = self.connection_state.read().await.clone();
        if let Some(handle) = self.app_handle.read().await.as_ref() {
            let _ = handle.emit("connection-state-changed", &state);
        }
    }

    /// Update connection state and emit to frontend.
    pub async fn set_connection_state(&self, state: ConnectionState) {
        *self.connection_state.write().await = state;
        self.emit_connection_state().await;
    }

    /// Get the current connection state.
    pub async fn get_connection_state(&self) -> ConnectionState {
        self.connection_state.read().await.clone()
    }

    /// Connect to the daemon, replacing any existing connection.
    pub async fn connect(&self, config: Option<&str>) -> Result<(), String> {
        self.set_connection_state(ConnectionState::Connecting).await;

        match client::connect(config).await {
            Ok((channel, transport)) => {
                *self.channel.write().await = Some(channel);
                *self.transport.write().await = Some(transport);
                // Clear cached role on reconnect.
                *self.role.write().await = None;
                self.set_connection_state(ConnectionState::Connected).await;
                Ok(())
            }
            Err(e) => {
                let msg = format!("{e:#}");
                self.set_connection_state(ConnectionState::Failed {
                    message: msg.clone(),
                })
                .await;
                Err(msg)
            }
        }
    }

    /// Get a gRPC client from the cached channel.
    pub async fn client(&self) -> Result<OrchestratorServiceClient<Channel>, String> {
        let guard = self.channel.read().await;
        let channel = guard
            .as_ref()
            .ok_or_else(|| "未连接到 daemon".to_string())?
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

    /// Start a background heartbeat task that pings every 5 seconds.
    ///
    /// On connection loss: attempts 3 reconnects at 1s intervals, emitting
    /// state transitions via the `connection-state-changed` Tauri event.
    pub fn start_heartbeat(self: &Arc<Self>) {
        let state = Arc::clone(self);
        let cancel = self.heartbeat_cancel.clone();

        tauri::async_runtime::spawn(async move {
            // Wait a brief moment for the initial connect to finish.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                }

                // Only run heartbeat if we have a channel.
                if state.channel.read().await.is_none() {
                    continue;
                }

                // Try a ping to check liveness.
                let ping_ok = match state.client().await {
                    Ok(mut c) => c
                        .ping(orchestrator_proto::PingRequest {})
                        .await
                        .is_ok(),
                    Err(_) => false,
                };

                if ping_ok {
                    // If we were in a non-connected state, transition back.
                    if !matches!(
                        *state.connection_state.read().await,
                        ConnectionState::Connected
                    ) {
                        state
                            .set_connection_state(ConnectionState::Connected)
                            .await;
                    }
                    continue;
                }

                // Connection lost — attempt reconnect.
                const MAX_ATTEMPTS: u32 = 3;
                let mut reconnected = false;

                for attempt in 1..=MAX_ATTEMPTS {
                    state
                        .set_connection_state(ConnectionState::Reconnecting {
                            attempt,
                            max_attempts: MAX_ATTEMPTS,
                        })
                        .await;

                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                    if state.connect(None).await.is_ok() {
                        // Verify with a ping.
                        if let Ok(mut c) = state.client().await {
                            if c.ping(orchestrator_proto::PingRequest {}).await.is_ok() {
                                reconnected = true;
                                // connect() already set Connected state.
                                break;
                            }
                        }
                    }
                }

                if !reconnected {
                    state
                        .set_connection_state(ConnectionState::Failed {
                            message: "重连失败，请检查 daemon 状态".into(),
                        })
                        .await;
                }
            }
        });
    }
}
