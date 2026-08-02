//! Behavioural tests for the managed SourceConnection gRPC surface.
//!
//! These live under a `tests/` directory on purpose. `scripts/coverage/coverage-governance.mjs`
//! excludes any path containing `/tests/`, so the `daemon/source_connection` key-module
//! percentage measures production lines only — test bodies inflate neither the numerator
//! nor the denominator.
//!
//! The fixture drives the real handlers against an in-process axum stub that speaks both
//! the Slack Gateway protocol (`/v1/**`) and the Slack manifest API (`/api/apps.manifest.*`).
//! `SlackGatewayClient::new` permits `http` only on loopback, which is what makes this
//! possible; see `docs/design_doc/orchestrator/170-source-domain-decomposition.md` for the
//! one path this cannot reach (a reviewed dedicated manifest must carry public HTTPS
//! endpoints, so `dedicated_preview` cannot complete against a loopback origin).

mod dedicated;
mod intent;
mod lifecycle;
mod projection;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use agent_orchestrator::source_connection::{
    ActivateSourceConnection, AsyncSourceConnectionRepository, SourceConnection as CoreConnection,
    SourceConnectionMode, StoreDedicatedProvisioning, StoreSourceConnectionIntent,
    UpdateDedicatedProvisioning,
};
use agent_orchestrator::test_utils::TestState;
use axum::Router;
use axum::extract::State;
use axum::routing::any;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Notify};

use crate::server::OrchestratorServer;
use crate::slack_gateway::SlackGatewayClient;

/// 32 bytes exactly — `SlackGatewayClient::new` rejects anything shorter.
pub(super) const ENROLLMENT_KEY: &str = "0123456789abcdef0123456789abcdef";

/// One recorded inbound request. Tests assert against these rather than against
/// "the handler returned Ok", so a handler that silently skips a Gateway call fails.
#[derive(Debug, Clone)]
pub(super) struct StubCall {
    pub(super) path: String,
    pub(super) body: Value,
    pub(super) authorization: Option<String>,
}

#[derive(Default)]
struct StubInner {
    responses: HashMap<String, (u16, Value)>,
    calls: Vec<StubCall>,
}

/// Scriptable stand-in for the Slack Gateway and the Slack manifest API.
///
/// Unconfigured paths answer `503 stub_unconfigured` rather than 404, so a handler that
/// reaches an endpoint the test did not anticipate fails loudly instead of taking a
/// not-found branch that happens to look like a legitimate error.
#[derive(Clone, Default)]
pub(super) struct GatewayStub {
    inner: Arc<StdMutex<StubInner>>,
}

impl GatewayStub {
    fn lock(&self) -> std::sync::MutexGuard<'_, StubInner> {
        self.inner.lock().expect("gateway stub lock")
    }

    /// Scripts a JSON body for one path.
    pub(super) fn reply(&self, path: &str, body: Value) -> &Self {
        self.lock().responses.insert(path.to_string(), (200, body));
        self
    }

    /// Scripts a bodiless success (the Gateway's `204` contract).
    pub(super) fn reply_no_content(&self, path: &str) -> &Self {
        self.lock()
            .responses
            .insert(path.to_string(), (204, Value::Null));
        self
    }

    /// Scripts a Gateway-shaped failure with a stable error code.
    pub(super) fn reply_error(&self, path: &str, status: u16, code: &str) -> &Self {
        self.lock()
            .responses
            .insert(path.to_string(), (status, json!({"error": code})));
        self
    }

    /// Scripts a Slack manifest API failure (`ok:false` under HTTP 200).
    pub(super) fn reply_manifest_error(&self, path: &str, code: &str) -> &Self {
        self.lock()
            .responses
            .insert(path.to_string(), (200, json!({"ok": false, "error": code})));
        self
    }

    pub(super) fn calls(&self) -> Vec<StubCall> {
        self.lock().calls.clone()
    }

    /// Every path the handler actually reached, in order.
    pub(super) fn paths(&self) -> Vec<String> {
        self.calls().into_iter().map(|call| call.path).collect()
    }

    pub(super) fn call(&self, path: &str) -> StubCall {
        self.calls()
            .into_iter()
            .find(|call| call.path == path)
            .unwrap_or_else(|| panic!("gateway stub never received {path}; saw {:?}", self.paths()))
    }

    pub(super) fn was_called(&self, path: &str) -> bool {
        self.calls().iter().any(|call| call.path == path)
    }
}

async fn handle(
    State(stub): State<GatewayStub>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let path = request.uri().path().to_string();
    let authorization = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_start_matches("Bearer ").to_string());
    let bytes = axum::body::to_bytes(request.into_body(), 256 * 1024)
        .await
        .unwrap_or_default();
    let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    let (status, payload) = {
        let mut inner = stub.inner.lock().expect("gateway stub lock");
        inner.calls.push(StubCall {
            path: path.clone(),
            body,
            authorization,
        });
        inner
            .responses
            .get(&path)
            .cloned()
            .unwrap_or_else(|| (503, json!({"error": "stub_unconfigured"})))
    };
    let code = axum::http::StatusCode::from_u16(status).expect("stub status");
    if payload.is_null() {
        return axum::response::Response::builder()
            .status(code)
            .body(axum::body::Body::empty())
            .expect("stub response");
    }
    (code, axum::Json(payload)).into_response()
}

use axum::response::IntoResponse;

/// A live `OrchestratorServer` wired to the stub, plus the seeding helpers the
/// dedicated flows need.
pub(super) struct Fixture {
    pub(super) server: OrchestratorServer,
    pub(super) stub: GatewayStub,
    _state: TestState,
}

impl Fixture {
    /// Builds a server whose Gateway *is* configured and points at the stub.
    pub(super) async fn with_gateway() -> Self {
        Self::build(true).await
    }

    /// Builds a server with no Gateway, for the fail-closed branches.
    pub(super) async fn without_gateway() -> Self {
        Self::build(false).await
    }

    async fn build(with_gateway: bool) -> Self {
        let mut harness = TestState::new();
        let state = harness.build();
        let stub = GatewayStub::default();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let address = listener.local_addr().expect("stub address");
        let app = Router::new().fallback(any(handle)).with_state(stub.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let origin = format!("http://{address}");

        let manifest_client =
            orchestrator_slack_gateway::slack::SlackClient::new(&origin, Duration::from_secs(5))
                .expect("stub manifest client");
        let gateway = with_gateway.then(|| {
            Arc::new(
                SlackGatewayClient::new(&origin, ENROLLMENT_KEY.to_string())
                    .expect("stub gateway client"),
            )
        });

        let server = OrchestratorServer::new(
            state,
            Arc::new(Notify::new()),
            None,
            None,
            gateway,
            Arc::new(manifest_client),
            Arc::new(Mutex::new(())),
        );
        Self {
            server,
            stub,
            _state: harness,
        }
    }

    pub(super) fn repository(&self) -> AsyncSourceConnectionRepository {
        AsyncSourceConnectionRepository::new(self.server.state.async_database.clone())
    }

    pub(super) async fn daemon_id(&self) -> String {
        self.repository().daemon_id().await.expect("daemon id")
    }

    fn encryption(&self) -> agent_orchestrator::secret_store_crypto::SecretEncryption {
        let keyring = agent_orchestrator::secret_key_lifecycle::load_keyring(
            &self.server.state.data_dir,
            &self.server.state.db_path,
        )
        .expect("test keyring");
        agent_orchestrator::secret_store_crypto::SecretEncryption::from_keyring(&keyring)
            .expect("test encryption")
    }

    pub(super) fn encrypt(&self, scope: &str, plaintext: &str) -> String {
        self.encryption()
            .encrypt_source_connection_credential("default", scope, plaintext)
            .expect("encrypt credential")
    }

    /// Activates a connection directly, bypassing OAuth, for the handlers whose
    /// subject is an already-connected installation.
    pub(super) async fn seed_connection(
        &self,
        id: &str,
        installation_id: &str,
        mode: SourceConnectionMode,
    ) -> CoreConnection {
        let pairing = self.encrypt(id, "pairing-secret-value");
        self.repository()
            .activate(ActivateSourceConnection {
                id: id.to_string(),
                project_id: "default".into(),
                provider: "slack".into(),
                display_label: "Seeded workspace".into(),
                provisioning_mode: mode,
                app_ownership: if mode == SourceConnectionMode::ManagedDedicated {
                    "workspace".into()
                } else {
                    "orchestrator".into()
                },
                // The repository refuses a dedicated connection without a workspace App
                // identity, so both fields travel together with the mode.
                app_id_digest: (mode == SourceConnectionMode::ManagedDedicated)
                    .then(|| hex::encode(Sha256::digest(b"A0TESTAPP01"))),
                manifest_version: (mode == SourceConnectionMode::ManagedDedicated)
                    .then(|| "orchestrator-slack-dedicated-v1".to_string()),
                provision_state: (mode == SourceConnectionMode::ManagedDedicated)
                    .then(|| "completed".to_string()),
                provision_error_code: None,
                installation_id: installation_id.to_string(),
                installation_id_digest: hex::encode(Sha256::digest(installation_id.as_bytes())),
                enterprise_id_digest: None,
                owner_daemon_id: self.daemon_id().await,
                generation: 1,
                version: 1,
                last_acked_cursor: 0,
                capabilities: vec!["delivery_v1".into()],
                scopes: vec!["reactions:read".into()],
                trigger_name: None,
                gateway_origin: Some("https://gateway.example".into()),
                pairing_secret_ciphertext: Some(pairing),
                request_id: format!("req-seed-{id}"),
            })
            .await
            .expect("seed connection")
    }

    /// Builds the full dedicated identity chain `read_app_identity` joins across:
    /// a completed provisioning row, the intent it produced, and the dedicated
    /// connection that intent activated.
    pub(super) async fn seed_dedicated_identity(
        &self,
        connection_id: &str,
        provisioning_id: &str,
        app_id: &str,
    ) -> CoreConnection {
        let repository = self.repository();
        let daemon_id = self.daemon_id().await;
        let connection = self
            .seed_connection(
                connection_id,
                &connection_id.replace("conn-", ""),
                SourceConnectionMode::ManagedDedicated,
            )
            .await;
        repository
            .store_dedicated_provisioning(StoreDedicatedProvisioning {
                id: provisioning_id.to_string(),
                project_id: "default".into(),
                display_label: "Seeded workspace".into(),
                owner_daemon_id: daemon_id.clone(),
                target_connection_id: None,
                manifest_version: "orchestrator-slack-dedicated-v1".into(),
                manifest_digest: "seeded-digest".into(),
                expires_at: rfc3339_in(chrono::Duration::minutes(10)),
            })
            .await
            .expect("seed provisioning");
        let intent_id = format!("intent-{provisioning_id}");
        repository
            .store_intent(StoreSourceConnectionIntent {
                id: intent_id.clone(),
                project_id: "default".into(),
                provider: "slack".into(),
                display_label: "Seeded workspace".into(),
                provisioning_mode: SourceConnectionMode::ManagedDedicated,
                owner_daemon_id: daemon_id,
                actor_digest: hex::encode(Sha256::digest(b"seed")),
                gateway_intent_id: format!("gw-{provisioning_id}"),
                authorize_url_ciphertext: self.encrypt(&intent_id, "https://slack.example/oauth"),
                poll_secret_ciphertext: self.encrypt(&intent_id, "poll-secret"),
                expires_at: rfc3339_in(chrono::Duration::minutes(10)),
            })
            .await
            .expect("seed intent");
        repository
            .complete_intent(
                "default",
                &intent_id,
                "completed",
                Some(connection_id),
                None,
            )
            .await
            .expect("complete seeded intent");
        for (expected, next) in [
            ("awaiting_approval", "creating"),
            ("creating", "handoff_pending"),
            ("handoff_pending", "oauth_pending"),
        ] {
            repository
                .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                    project_id: "default".into(),
                    id: provisioning_id.to_string(),
                    expected_status: expected.into(),
                    status: next.into(),
                    app_id_ciphertext: None,
                    app_id_digest: None,
                    oauth_intent_id: None,
                    error_code: None,
                })
                .await
                .expect("advance seeded provisioning");
        }
        repository
            .update_dedicated_provisioning(UpdateDedicatedProvisioning {
                project_id: "default".into(),
                id: provisioning_id.to_string(),
                expected_status: "oauth_pending".into(),
                status: "completed".into(),
                app_id_ciphertext: Some(self.encrypt(provisioning_id, app_id)),
                app_id_digest: Some(hex::encode(Sha256::digest(app_id.as_bytes()))),
                oauth_intent_id: Some(intent_id),
                error_code: None,
            })
            .await
            .expect("complete seeded provisioning");
        connection
    }
}

pub(super) fn rfc3339_in(offset: chrono::Duration) -> String {
    (chrono::Utc::now() + offset).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// A Gateway installation projection with the fields every completed intent needs.
pub(super) fn installation(daemon_id: &str, installation_id: &str, version: i64) -> Value {
    json!({
        "id": installation_id,
        "team_digest": hex::encode(Sha256::digest(installation_id.as_bytes())),
        "enterprise_digest": null,
        "owner_daemon_id": daemon_id,
        "owner_project_id": "default",
        "provisioning_mode": "managed_shared",
        "app_connection_id": null,
        "app_id_digest": null,
        "manifest_version": null,
        "generation": 1,
        "version": version,
        "state": "active",
        "scopes": ["reactions:read"],
        "last_acked_cursor": 0,
        "last_error_code": null
    })
}
