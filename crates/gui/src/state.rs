use orchestrator_proto::OrchestratorServiceClient;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

use crate::client::{self, TransportKind};

const MAX_ATTENTION_NOTIFICATION_KEYS: usize = 512;

#[derive(Default)]
struct AttentionNotificationLedger {
    keys: HashSet<String>,
    order: VecDeque<String>,
}

impl AttentionNotificationLedger {
    fn record(&mut self, key: String) -> bool {
        if self.keys.contains(&key) {
            return false;
        }
        self.keys.insert(key.clone());
        self.order.push_back(key);
        while self.order.len() > MAX_ATTENTION_NOTIFICATION_KEYS {
            if let Some(expired) = self.order.pop_front() {
                self.keys.remove(&expired);
            }
        }
        true
    }
}

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
    /// Bounded per-app ledger preventing notification replay after reconnect.
    attention_notification_ledger: Arc<Mutex<AttentionNotificationLedger>>,
    /// Current connection state.
    connection_state: Arc<RwLock<ConnectionState>>,
    /// Tauri AppHandle for emitting events.
    app_handle: Arc<RwLock<Option<AppHandle>>>,
    /// Cancellation token for the heartbeat task.
    heartbeat_cancel: CancellationToken,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            channel: Arc::new(RwLock::new(None)),
            transport: Arc::new(RwLock::new(None)),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            role: Arc::new(RwLock::new(None)),
            attention_notification_ledger: Arc::new(Mutex::new(
                AttentionNotificationLedger::default(),
            )),
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
        Ok(OrchestratorServiceClient::new(channel)
            .max_decoding_message_size(client::max_decode_size()))
    }

    /// Install an already-connected channel for deterministic in-process adapter tests.
    #[cfg(test)]
    pub(crate) async fn install_test_channel(&self, channel: Channel) {
        *self.channel.write().await = Some(channel);
        *self.transport.write().await = Some(TransportKind::Tls);
        self.set_connection_state(ConnectionState::Connected).await;
    }

    /// Get the transport kind of the current connection.
    pub async fn transport_kind(&self) -> Option<TransportKind> {
        *self.transport.read().await
    }

    /// Register a streaming subscription and return its cancellation token.
    pub async fn register_stream(&self, key: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Some(previous) = self
            .active_streams
            .write()
            .await
            .insert(key.to_string(), token.clone())
        {
            previous.cancel();
        }
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

    /// Returns true exactly once for each daemon-authored item/version key.
    pub async fn record_attention_notification(&self, key: String) -> bool {
        self.attention_notification_ledger.lock().await.record(key)
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
                    Ok(mut c) => c.ping(orchestrator_proto::PingRequest {}).await.is_ok(),
                    Err(_) => false,
                };

                if ping_ok {
                    // If we were in a non-connected state, transition back.
                    if !matches!(
                        *state.connection_state.read().await,
                        ConnectionState::Connected
                    ) {
                        state.set_connection_state(ConnectionState::Connected).await;
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
                        if let Ok(mut c) = state.client().await
                            && c.ping(orchestrator_proto::PingRequest {}).await.is_ok()
                        {
                            reconnected = true;
                            // connect() already set Connected state.
                            break;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_ledger_deduplicates_and_stays_bounded() {
        let mut ledger = AttentionNotificationLedger::default();
        assert!(ledger.record("attention-1:1".into()));
        assert!(!ledger.record("attention-1:1".into()));
        assert!(ledger.record("attention-1:2".into()));
        for version in 3..=(MAX_ATTENTION_NOTIFICATION_KEYS + 3) {
            assert!(ledger.record(format!("attention-1:{version}")));
        }
        assert_eq!(ledger.keys.len(), MAX_ATTENTION_NOTIFICATION_KEYS);
        assert_eq!(ledger.order.len(), MAX_ATTENTION_NOTIFICATION_KEYS);
        assert!(ledger.record("attention-1:1".into()));
    }
}
