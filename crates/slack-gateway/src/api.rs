//! Thin HTTP handlers for OAuth, signed events, delivery, and provider proxy.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;

use crate::config::GatewayConfig;
use crate::domain::{DeliveryProjection, GatewayCapabilities, NormalizedSlackEvent};
use crate::slack::{SlackClient, SlackError, SlackEvent, parse_event, verify_request};
use crate::store::{
    GatewayStore, IntentStatus, NewIntent, OAuthInstallation, OfficialAppCredentials,
};

const MAX_REQUEST_BODY_BYTES: usize = 128 * 1024;
const REQUIRED_SCOPES: &[&str] = &["reactions:read"];

/// Shared gateway API dependencies.
#[derive(Clone)]
pub struct GatewayState {
    /// Validated runtime configuration.
    pub config: GatewayConfig,
    /// Gateway-private persistence.
    pub store: GatewayStore,
    /// Strict Slack provider client.
    pub slack: SlackClient,
    limiter: Arc<Mutex<FixedWindowLimiter>>,
}

impl std::fmt::Debug for GatewayState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayState")
            .field("config", &self.config)
            .field("store", &self.store)
            .field("slack", &self.slack)
            .finish_non_exhaustive()
    }
}

impl GatewayState {
    /// Builds API state with a bounded pre-auth intent limiter.
    pub fn new(config: GatewayConfig, store: GatewayStore, slack: SlackClient) -> Self {
        Self {
            config,
            store,
            slack,
            limiter: Arc::new(Mutex::new(FixedWindowLimiter::new(60))),
        }
    }

    fn allow_intent(&self) -> bool {
        self.limiter
            .lock()
            .map(|mut limiter| limiter.allow())
            .unwrap_or(false)
    }
}

/// Builds the public and daemon-facing gateway router.
pub fn router(state: GatewayState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/oauth/intents", post(create_intent))
        .route(
            "/v1/oauth/intents/{intent_id}",
            get(intent_status).delete(cancel_intent),
        )
        .route("/slack/oauth/callback", get(oauth_callback))
        .route("/slack/events", post(slack_events))
        .route("/v1/deliveries/claim", post(claim_deliveries))
        .route("/v1/deliveries/ack", post(acknowledge_deliveries))
        .route("/v1/provider/permalink", post(resolve_permalink))
        .route(
            "/v1/installations/disconnect",
            post(disconnect_installation),
        )
        .route("/v1/installations/transfer", post(transfer_installation))
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    protocol_version: u32,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        protocol_version: 1,
    })
}

async fn capabilities() -> Json<GatewayCapabilities> {
    Json(GatewayCapabilities::default())
}

#[derive(Debug, Deserialize)]
struct CreateIntentRequest {
    daemon_id: String,
    project_id: String,
    actor_id: String,
    #[serde(default)]
    requested_scopes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CreateIntentResponse {
    intent_id: String,
    authorize_url: String,
    poll_secret: String,
    expires_at: String,
}

async fn create_intent(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<CreateIntentRequest>,
) -> Result<Json<CreateIntentResponse>, ApiError> {
    authenticate_enrollment(&headers, &state.config.enrollment_key)?;
    if !state.allow_intent() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "intent_rate_limited",
        ));
    }
    let requested_scopes = if request.requested_scopes.is_empty() {
        REQUIRED_SCOPES
            .iter()
            .map(|scope| (*scope).to_string())
            .collect()
    } else {
        request.requested_scopes
    };
    if requested_scopes != REQUIRED_SCOPES {
        return Err(ApiError::bad_request("oauth_scope_mismatch"));
    }
    let redirect_uri = state
        .config
        .oauth_callback_url()
        .map_err(|_| ApiError::internal())?;
    let created = state
        .store
        .create_intent(NewIntent {
            daemon_id: &request.daemon_id,
            project_id: &request.project_id,
            actor_id: &request.actor_id,
            redirect_uri: &redirect_uri,
            requested_scopes: &requested_scopes,
            ttl: Duration::from_secs(state.config.intent_ttl_secs),
        })
        .map_err(map_store_error)?;
    let credentials = state
        .store
        .official_app_credentials()
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "official_app_not_ready"))?;
    let authorize_url = oauth_authorize_url(
        &state.config.slack_api_base,
        &credentials,
        &created.oauth_state,
        &redirect_uri,
        &requested_scopes,
    )?;
    Ok(Json(CreateIntentResponse {
        intent_id: created.id,
        authorize_url,
        poll_secret: created.poll_secret,
        expires_at: created.expires_at,
    }))
}

fn authenticate_enrollment(headers: &HeaderMap, expected: &str) -> Result<(), ApiError> {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    let provided = bearer(headers)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "enrollment_unauthorized"))?;
    let mut verifier = <Hmac<Sha256> as KeyInit>::new_from_slice(expected.as_bytes())
        .map_err(|_| ApiError::internal())?;
    verifier.update(b"orchestrator-slack-gateway-enrollment-v1");
    let expected_mac = verifier.finalize().into_bytes();
    let mut candidate = <Hmac<Sha256> as KeyInit>::new_from_slice(provided.as_bytes())
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "enrollment_unauthorized"))?;
    candidate.update(b"orchestrator-slack-gateway-enrollment-v1");
    candidate
        .verify_slice(&expected_mac)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "enrollment_unauthorized"))
}

#[derive(Debug, Serialize)]
struct IntentStatusResponse {
    intent_id: String,
    status: String,
    expires_at: String,
    error_code: Option<String>,
    installation: Option<crate::domain::InstallationProjection>,
    pairing_secret: Option<String>,
}

async fn intent_status(
    State(state): State<GatewayState>,
    Path(intent_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<IntentStatusResponse>, ApiError> {
    let poll_secret = bearer(&headers)?;
    let status = state
        .store
        .intent_status(&intent_id, poll_secret)
        .map_err(map_store_error)?;
    Ok(Json(intent_response(status)))
}

async fn cancel_intent(
    State(state): State<GatewayState>,
    Path(intent_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let poll_secret = bearer(&headers)?;
    let changed = state
        .store
        .cancel_intent(&intent_id, poll_secret)
        .map_err(map_store_error)?;
    if changed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(StatusCode::CONFLICT, "intent_not_pending"))
    }
}

fn intent_response(status: IntentStatus) -> IntentStatusResponse {
    IntentStatusResponse {
        intent_id: status.id,
        status: status.status,
        expires_at: status.expires_at,
        error_code: status.error_code,
        installation: status.installation,
        pairing_secret: status.pairing_secret,
    }
}

#[derive(Debug, Deserialize)]
struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oauth_callback(
    State(state): State<GatewayState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Html<&'static str>, ApiError> {
    let oauth_state = query
        .state
        .as_deref()
        .filter(|value| value.len() <= 256)
        .ok_or_else(|| ApiError::bad_request("oauth_state_missing"))?;
    if let Some(provider_error) = query.error.as_deref() {
        let safe_code = if provider_error == "access_denied" {
            "oauth_denied"
        } else {
            "oauth_provider_error"
        };
        state
            .store
            .fail_intent_by_state(oauth_state, safe_code)
            .map_err(map_store_error)?;
        return Err(ApiError::bad_request(safe_code));
    }
    let code = query
        .code
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("oauth_code_missing"))?;
    let pending = state
        .store
        .pending_intent_by_state(oauth_state)
        .map_err(map_store_error)?;
    let credentials = state
        .store
        .official_app_credentials()
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "official_app_not_ready"))?;
    let exchange = match state
        .slack
        .exchange_oauth(code, &pending.redirect_uri, &credentials)
        .await
    {
        Ok(exchange) => exchange,
        Err(error) => {
            state
                .store
                .fail_intent_by_state(oauth_state, error.code())
                .map_err(map_store_error)?;
            return Err(map_slack_error(error));
        }
    };
    let mut granted = exchange.scopes.clone();
    granted.sort();
    let expected = pending.requested_scopes.clone();
    if granted != expected {
        state
            .store
            .fail_intent_by_state(oauth_state, "oauth_scope_mismatch")
            .map_err(map_store_error)?;
        return Err(ApiError::bad_request("oauth_scope_mismatch"));
    }
    state
        .store
        .complete_intent(
            &pending,
            OAuthInstallation {
                team_id: &exchange.team_id,
                enterprise_id: exchange.enterprise_id.as_deref(),
                scopes: &exchange.scopes,
                bot_token: &exchange.bot_token,
            },
        )
        .map_err(map_store_error)?;
    Ok(Html(
        "<!doctype html><meta charset=utf-8><title>Orchestrator Slack connection</title><p>Slack workspace connected. You can return to Orchestrator.</p>",
    ))
}

async fn slack_events(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let timestamp = required_header(&headers, "x-slack-request-timestamp")?;
    let signature = required_header(&headers, "x-slack-signature")?;
    let credentials = state
        .store
        .official_app_credentials()
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "official_app_not_ready"))?;
    verify_request(
        &credentials.signing_secret,
        timestamp,
        signature,
        &body,
        SystemTime::now(),
    )
    .map_err(map_slack_error)?;
    let event = match parse_event(&body) {
        Ok(event) => event,
        Err(error) if error.code() == "slack_event_unsupported" => {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        Err(error) => return Err(map_slack_error(error)),
    };
    match event {
        SlackEvent::UrlVerification { challenge } => {
            Ok(Json(serde_json::json!({"challenge": challenge})).into_response())
        }
        SlackEvent::ReactionAdded {
            event_id,
            team_id,
            enterprise_id,
            actor_id,
            reaction,
            channel_id,
            message_ts,
            event_ts,
        } => {
            let installation = state
                .store
                .installation_for_team(&team_id)
                .map_err(map_store_error)?;
            let enterprise_digest = enterprise_id
                .as_deref()
                .map(|value| state.store_identity_digest("slack-enterprise", value));
            if enterprise_digest != installation.enterprise_digest {
                return Err(ApiError::not_found("unknown_installation"));
            }
            state
                .store
                .enqueue_delivery(&NormalizedSlackEvent {
                    external_event_id: event_id,
                    event_type: "reaction_added".into(),
                    installation_id: installation.id,
                    external_actor_id: Some(actor_id),
                    reaction: Some(reaction),
                    channel_id: Some(channel_id),
                    message_ts: Some(message_ts),
                    event_ts,
                    team_digest: installation.team_digest,
                    enterprise_digest: installation.enterprise_digest,
                })
                .map_err(map_store_error)?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        SlackEvent::AppUninstalled {
            event_id,
            team_id,
            enterprise_id,
            event_ts,
        }
        | SlackEvent::TokensRevoked {
            event_id,
            team_id,
            enterprise_id,
            event_ts,
        } => {
            let installation = state
                .store
                .installation_for_team(&team_id)
                .map_err(map_store_error)?;
            let enterprise_digest = enterprise_id
                .as_deref()
                .map(|value| state.store_identity_digest("slack-enterprise", value));
            if enterprise_digest != installation.enterprise_digest {
                return Err(ApiError::not_found("unknown_installation"));
            }
            state
                .store
                .revoke_team(&team_id, "slack_credential_revoked")
                .map_err(map_store_error)?;
            state
                .store
                .enqueue_delivery(&NormalizedSlackEvent {
                    external_event_id: event_id,
                    event_type: "installation_revoked".into(),
                    installation_id: installation.id,
                    external_actor_id: None,
                    reaction: None,
                    channel_id: None,
                    message_ts: None,
                    event_ts,
                    team_digest: installation.team_digest,
                    enterprise_digest: installation.enterprise_digest,
                })
                .map_err(map_store_error)?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaimRequest {
    installation_id: String,
    daemon_id: String,
    #[serde(default)]
    after_cursor: i64,
    #[serde(default = "default_claim_limit")]
    limit: u32,
    #[serde(default = "default_lease_secs")]
    lease_secs: u64,
}

fn default_claim_limit() -> u32 {
    50
}

fn default_lease_secs() -> u64 {
    30
}

#[derive(Debug, Serialize)]
struct ClaimResponse {
    deliveries: Vec<DeliveryProjection>,
}

async fn claim_deliveries(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, ApiError> {
    let pairing = bearer(&headers)?;
    state
        .store
        .authenticate_pairing(&request.installation_id, &request.daemon_id, pairing)
        .map_err(map_store_error)?;
    if request.lease_secs == 0 || request.lease_secs > state.config.max_lease_secs {
        return Err(ApiError::bad_request("delivery_lease_invalid"));
    }
    let deliveries = state
        .store
        .claim_deliveries(
            &request.installation_id,
            &request.daemon_id,
            request.after_cursor,
            request.limit,
            Duration::from_secs(request.lease_secs),
        )
        .map_err(map_store_error)?;
    Ok(Json(ClaimResponse { deliveries }))
}

#[derive(Debug, Deserialize)]
struct AckRequest {
    installation_id: String,
    daemon_id: String,
    cursors: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct AckResponse {
    last_acked_cursor: i64,
}

async fn acknowledge_deliveries(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<AckRequest>,
) -> Result<Json<AckResponse>, ApiError> {
    let pairing = bearer(&headers)?;
    state
        .store
        .authenticate_pairing(&request.installation_id, &request.daemon_id, pairing)
        .map_err(map_store_error)?;
    let last_acked_cursor = state
        .store
        .acknowledge_deliveries(
            &request.installation_id,
            &request.daemon_id,
            &request.cursors,
        )
        .map_err(map_store_error)?;
    Ok(Json(AckResponse { last_acked_cursor }))
}

#[derive(Debug, Deserialize)]
struct PermalinkRequest {
    installation_id: String,
    daemon_id: String,
    generation: i64,
    channel_id: String,
    message_ts: String,
}

#[derive(Debug, Serialize)]
struct PermalinkResponse {
    permalink: String,
    generation: i64,
}

async fn resolve_permalink(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<PermalinkRequest>,
) -> Result<Json<PermalinkResponse>, ApiError> {
    let pairing = bearer(&headers)?;
    let credential = state
        .store
        .installation_credential(&request.installation_id, &request.daemon_id, pairing)
        .map_err(map_store_error)?;
    if credential.projection.generation != request.generation {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "credential_generation_stale",
        ));
    }
    let permalink = state
        .slack
        .get_permalink(
            &credential.bot_token,
            &request.channel_id,
            &request.message_ts,
        )
        .await
        .map_err(map_slack_error)?;
    Ok(Json(PermalinkResponse {
        permalink,
        generation: credential.projection.generation,
    }))
}

#[derive(Debug, Deserialize)]
struct InstallationMutationRequest {
    installation_id: String,
    daemon_id: String,
    expected_version: i64,
}

async fn disconnect_installation(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<InstallationMutationRequest>,
) -> Result<Json<crate::domain::InstallationProjection>, ApiError> {
    let pairing = bearer(&headers)?;
    state
        .store
        .disconnect_installation(
            &request.installation_id,
            &request.daemon_id,
            pairing,
            request.expected_version,
        )
        .map(Json)
        .map_err(map_store_error)
}

#[derive(Debug, Deserialize)]
struct TransferRequest {
    installation_id: String,
    daemon_id: String,
    expected_version: i64,
    target_daemon_id: String,
}

#[derive(Debug, Serialize)]
struct TransferResponse {
    installation: crate::domain::InstallationProjection,
    pairing_secret: String,
}

async fn transfer_installation(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<TransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    let pairing = bearer(&headers)?;
    let (installation, pairing_secret) = state
        .store
        .transfer_installation(
            &request.installation_id,
            &request.daemon_id,
            pairing,
            request.expected_version,
            &request.target_daemon_id,
        )
        .map_err(map_store_error)?;
    Ok(Json(TransferResponse {
        installation,
        pairing_secret,
    }))
}

impl GatewayState {
    fn store_identity_digest(&self, purpose: &str, value: &str) -> String {
        self.store.identity_digest(purpose, value)
    }
}

fn oauth_authorize_url(
    base: &str,
    credentials: &OfficialAppCredentials,
    oauth_state: &str,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<String, ApiError> {
    let mut url = Url::parse(base)
        .and_then(|base| base.join("/oauth/v2/authorize"))
        .map_err(|_| ApiError::internal())?;
    url.query_pairs_mut()
        .append_pair("client_id", &credentials.client_id)
        .append_pair("scope", &scopes.join(","))
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", oauth_state);
    Ok(url.to_string())
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, ApiError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 1024)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "provider_auth_missing"))
}

fn bearer(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .ok_or_else(|| ApiError::not_found("installation_not_found"))
}

fn map_store_error(error: anyhow::Error) -> ApiError {
    let code = error.to_string();
    match code.as_str() {
        "intent_not_found" | "installation_not_found" | "unknown_installation" => {
            ApiError::not_found(code)
        }
        "oauth_state_invalid_or_expired" => ApiError::bad_request(code),
        "installation_owner_conflict" | "oauth_state_already_consumed" => {
            ApiError::new(StatusCode::CONFLICT, code)
        }
        "installation_not_active" | "credential_generation_stale" => {
            ApiError::new(StatusCode::CONFLICT, code)
        }
        _ => ApiError::internal(),
    }
}

fn map_slack_error(error: SlackError) -> ApiError {
    let status = match error.code() {
        "slack_signature_invalid" | "slack_timestamp_invalid" | "slack_timestamp_expired" => {
            StatusCode::UNAUTHORIZED
        }
        "slack_oauth_rate_limited" | "slack_manifest_rate_limited" | "slack_rate_limited" => {
            StatusCode::TOO_MANY_REQUESTS
        }
        "oauth_denied" | "oauth_redirect_mismatch" | "oauth_scope_mismatch" => {
            StatusCode::BAD_REQUEST
        }
        "slack_oauth_unavailable" | "slack_manifest_unavailable" | "slack_proxy_unavailable" => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::BAD_REQUEST,
    };
    ApiError {
        status,
        code: error.code().to_string(),
        retry_after: error.retry_after(),
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: String,
    retry_after: Option<u64>,
}

impl ApiError {
    fn new(status: StatusCode, code: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            retry_after: None,
        }
    }

    fn bad_request(code: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }

    fn not_found(code: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code)
    }

    fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "gateway_internal_error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response =
            (self.status, Json(serde_json::json!({"error": self.code}))).into_response();
        if let Some(retry_after) = self.retry_after {
            if let Ok(value) = retry_after.to_string().parse() {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[derive(Debug)]
struct FixedWindowLimiter {
    started_at: Instant,
    used: u32,
    limit: u32,
}

impl FixedWindowLimiter {
    fn new(limit: u32) -> Self {
        Self {
            started_at: Instant::now(),
            used: 0,
            limit,
        }
    }

    fn allow(&mut self) -> bool {
        if self.started_at.elapsed() >= Duration::from_secs(60) {
            self.started_at = Instant::now();
            self.used = 0;
        }
        if self.used >= self.limit {
            return false;
        }
        self.used += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use tower::ServiceExt;

    fn test_state() -> (tempfile::TempDir, GatewayState) {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = GatewayConfig {
            bind: "127.0.0.1:19440".parse().expect("bind"),
            public_url: "http://127.0.0.1:19440".into(),
            database: temp.path().join("gateway.db"),
            master_key: base64::engine::general_purpose::STANDARD.encode([4_u8; 32]),
            enrollment_key: "0123456789abcdef0123456789abcdef".into(),
            slack_api_base: "http://127.0.0.1:19441".into(),
            intent_ttl_secs: 600,
            max_lease_secs: 60,
            provider_timeout_secs: 1,
        };
        let crypto = crate::crypto::GatewayCrypto::from_base64(&config.master_key).expect("crypto");
        let store = GatewayStore::open(&config.database, crypto).expect("store");
        store
            .put_official_app_credentials(&OfficialAppCredentials {
                app_id: "A123".into(),
                client_id: "123.456".into(),
                client_secret: "secret".into(),
                signing_secret: "signing".into(),
            })
            .expect("credentials");
        let slack =
            SlackClient::new(&config.slack_api_base, Duration::from_secs(1)).expect("slack");
        (temp, GatewayState::new(config, store, slack))
    }

    #[tokio::test]
    async fn capabilities_reserve_no_dedicated_fallback() {
        let (_temp, state) = test_state();
        let response = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/v1/capabilities")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body");
        let payload: GatewayCapabilities = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload.supported_modes, ["managed_shared"]);
    }

    #[tokio::test]
    async fn intent_creation_uses_exact_reviewed_scope_and_hides_credentials() {
        let (_temp, state) = test_state();
        let response = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/v1/oauth/intents")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer 0123456789abcdef0123456789abcdef")
                    .body(axum::body::Body::from(
                        r#"{"daemon_id":"daemon-a","project_id":"default","actor_id":"admin-a"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .expect("body");
        let text = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(text.contains("reactions%3Aread"));
        assert!(text.contains("intent_id"));
        assert!(!text.contains("client_secret"));
        assert!(!text.contains("signing"));
    }

    #[tokio::test]
    async fn intent_creation_requires_enrollment_authentication() {
        let (_temp, state) = test_state();
        let response = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/v1/oauth/intents")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"daemon_id":"attacker","project_id":"victim","actor_id":"unknown"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn global_intent_limiter_is_bounded() {
        let mut limiter = FixedWindowLimiter::new(2);
        assert!(limiter.allow());
        assert!(limiter.allow());
        assert!(!limiter.allow());
    }
}
