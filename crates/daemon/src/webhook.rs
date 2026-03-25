//! Lightweight HTTP webhook server for external trigger ingestion.
//!
//! Runs alongside the gRPC server when `--webhook-bind` is specified.
//! Accepts `POST /webhook/{trigger_name}` with a JSON body and fires
//! the named trigger with the payload.

use agent_orchestrator::state::InnerState;
use agent_orchestrator::trigger_engine::{TriggerEventPayload, broadcast_task_event};
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use tracing::{info, warn};

type HmacSha256 = Hmac<Sha256>;

/// Shared state for the webhook HTTP server.
#[derive(Clone)]
pub struct WebhookState {
    /// Reference to the daemon's inner state.
    pub inner: Arc<InnerState>,
    /// Optional shared secret for HMAC-SHA256 signature verification.
    pub secret: Option<String>,
}

/// Build the axum router for webhook ingestion.
pub fn router(state: WebhookState) -> Router {
    Router::new()
        .route("/webhook/{trigger_name}", post(handle_webhook))
        .route(
            "/webhook/{project}/{trigger_name}",
            post(handle_webhook_with_project),
        )
        .route("/health", axum::routing::get(health))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1MB
}

async fn health() -> &'static str {
    "ok"
}

async fn handle_webhook(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Path(trigger_name): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let project = agent_orchestrator::config::DEFAULT_PROJECT_ID.to_string();
    do_webhook(state, headers, trigger_name, project, body)
}

async fn handle_webhook_with_project(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Path((project, trigger_name)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    do_webhook(state, headers, trigger_name, project, body)
}

fn do_webhook(
    state: WebhookState,
    headers: HeaderMap,
    trigger_name: String,
    project: String,
    body: axum::body::Bytes,
) -> Response {
    // ── Signature verification ───────────────────────────────────────────
    if let Some(ref secret) = state.secret {
        let signature = headers
            .get("x-webhook-signature")
            .and_then(|v| v.to_str().ok());
        match signature {
            Some(sig) => {
                if !verify_hmac(secret.as_bytes(), &body, sig) {
                    warn!(trigger = trigger_name.as_str(), "webhook signature failed");
                    return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
                }
            }
            None => {
                warn!(trigger = trigger_name.as_str(), "webhook missing signature");
                return (StatusCode::UNAUTHORIZED, "missing signature").into_response();
            }
        }
    }

    // ── Parse JSON body ─────────────────────────────────────────────────
    let payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid JSON body").into_response();
            }
        }
    };

    // ── Broadcast webhook event to trigger engine ───────────────────────
    broadcast_task_event(
        &state.inner,
        TriggerEventPayload {
            event_type: "webhook".to_string(),
            task_id: String::new(),
            payload: Some(payload.clone()),
        },
    );

    // ── Direct trigger fire ─────────────────────────────────────────────
    match agent_orchestrator::service::resource::fire_trigger(
        &state.inner,
        &trigger_name,
        Some(&project),
    ) {
        Ok(task_id) => {
            info!(
                trigger = trigger_name.as_str(),
                project = project.as_str(),
                task_id = task_id.as_str(),
                "webhook trigger fired"
            );
            let json = serde_json::json!({
                "task_id": task_id,
                "trigger": trigger_name,
                "status": "fired"
            });
            (StatusCode::OK, axum::Json(json)).into_response()
        }
        Err(e) => {
            warn!(
                trigger = trigger_name.as_str(),
                error = %e,
                "webhook trigger fire failed"
            );
            let json = serde_json::json!({
                "error": e.to_string(),
                "trigger": trigger_name,
            });
            (StatusCode::NOT_FOUND, axum::Json(json)).into_response()
        }
    }
}

/// Verify HMAC-SHA256 signature.
fn verify_hmac(secret: &[u8], body: &[u8], signature: &str) -> bool {
    let hex_sig = signature.strip_prefix("sha256=").unwrap_or(signature);
    let expected = match hex::decode(hex_sig) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}
