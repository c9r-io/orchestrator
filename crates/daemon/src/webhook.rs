//! Lightweight HTTP webhook server for external trigger ingestion.
//!
//! Runs alongside the gRPC server by default on `127.0.0.1:19090` (loopback).
//! Use `--webhook-bind <addr>` to override or `--webhook-bind none` to disable.
//! Non-loopback addresses require a webhook secret or `--webhook-allow-unsigned`.
//! Accepts `POST /webhook/{trigger_name}` with a JSON body and fires
//! the named trigger with the payload.

use agent_orchestrator::state::InnerState;
use agent_orchestrator::trigger_engine::{TriggerEventPayload, broadcast_task_event};
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use hmac::{Hmac, KeyInit, Mac};
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
    // ── Resolve per-trigger webhook config ───────────────────────────────
    let active_config = agent_orchestrator::config_load::read_active_config(&state.inner).ok();
    let trigger_webhook_cfg = active_config.as_ref().and_then(|ac| {
        ac.config
            .projects
            .get(&project)
            .and_then(|p| p.triggers.get(&trigger_name))
            .and_then(|t| t.event.as_ref())
            .and_then(|e| e.webhook.as_ref())
    });

    // ── Signature verification (per-trigger → global fallback) ──────────
    let verification_result = if let Some(wh_cfg) = trigger_webhook_cfg {
        // Per-trigger secret from SecretStore
        if let Some(ref secret_ref) = wh_cfg.secret {
            let header_name = wh_cfg
                .signature_header
                .as_deref()
                .unwrap_or("x-webhook-signature");
            verify_with_store_secrets(
                &state.inner,
                &project,
                &secret_ref.from_ref,
                header_name,
                &headers,
                &body,
            )
        } else {
            // Per-trigger config exists but no secret → no verification
            Ok(())
        }
    } else if let Some(ref global_secret) = state.secret {
        // Global fallback
        verify_with_single_secret(global_secret, "x-webhook-signature", &headers, &body)
    } else {
        // No secret configured anywhere → allow
        Ok(())
    };

    if let Err(msg) = verification_result {
        warn!(
            trigger = trigger_name.as_str(),
            reason = msg.as_str(),
            "webhook auth failed"
        );
        return (StatusCode::UNAUTHORIZED, msg).into_response();
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
            project: Some(project.clone()),
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

/// Verify signature against a single secret string.
fn verify_with_single_secret(
    secret: &str,
    header_name: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), String> {
    let signature = headers.get(header_name).and_then(|v| v.to_str().ok());
    match signature {
        Some(sig) => {
            if verify_hmac(secret.as_bytes(), body, sig) {
                Ok(())
            } else {
                Err("invalid signature".to_string())
            }
        }
        None => Err("missing signature".to_string()),
    }
}

/// Verify signature against all values in a SecretStore (multi-key rotation).
fn verify_with_store_secrets(
    state: &InnerState,
    project: &str,
    store_name: &str,
    header_name: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), String> {
    let signature = headers
        .get(header_name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "missing signature".to_string())?;

    // Read active config to resolve SecretStore
    let active = agent_orchestrator::config_load::read_active_config(state)
        .map_err(|e| format!("config error: {e}"))?;
    let secret_stores = active
        .config
        .projects
        .get(project)
        .map(|p| &p.secret_stores)
        .ok_or_else(|| format!("project '{project}' not found"))?;
    let store = secret_stores
        .get(store_name)
        .ok_or_else(|| format!("SecretStore '{store_name}' not found"))?;

    // Try all values in the store — any match is accepted (rotation support)
    for secret_value in store.data.values() {
        if verify_hmac(secret_value.as_bytes(), body, signature) {
            return Ok(());
        }
    }
    Err("invalid signature (no matching key)".to_string())
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
