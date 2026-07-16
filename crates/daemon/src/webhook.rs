//! Lightweight HTTP webhook server for external trigger ingestion.
//!
//! Runs alongside the gRPC server by default on `127.0.0.1:19090` (loopback).
//! Use `--webhook-bind <addr>` to override or `--webhook-bind none` to disable.
//! Non-loopback addresses require a webhook secret or `--webhook-allow-unsigned`.
//! Accepts `POST /webhook/{trigger_name}` with a JSON body and fires
//! the named trigger with the payload.

use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::source::{
    AsyncSourceRepository, ConversationRef, ExternalActorRef, ExternalArtifactRef,
    IngestSourceEvent, NormalizedSourceEvent, SourceCommand, SourceEventKind, SourceReactionRef,
};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::trigger_engine::{
    TriggerEventPayload, broadcast_task_event, fire_trigger_canonical,
};
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
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
        .route(
            "/source/slack/{project}/{trigger_name}",
            post(handle_slack_source),
        )
        .route("/health", axum::routing::get(health))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1MB
}

const SLACK_BODY_LIMIT: usize = 256 * 1024;

async fn handle_slack_source(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Path((project, trigger_name)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if body.len() > SLACK_BODY_LIMIT {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "Slack body exceeds 262144 bytes",
        )
            .into_response();
    }
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let is_json = content_type.starts_with("application/json");
    let is_form = content_type.starts_with("application/x-www-form-urlencoded");
    if !is_json && !is_form {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported Slack content type",
        )
            .into_response();
    }

    let active = match agent_orchestrator::config_load::read_active_config(&state.inner) {
        Ok(active) => active,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
    };
    if !active
        .config
        .runtime_policy_for_project(&project)
        .source_ingest_enabled
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "source ingestion is disabled",
        )
            .into_response();
    }
    let trigger = active
        .config
        .projects
        .get(&project)
        .and_then(|value| value.triggers.get(&trigger_name));
    let Some(trigger) = trigger else {
        return (StatusCode::NOT_FOUND, "source trigger not found").into_response();
    };
    let Some(webhook) = trigger
        .event
        .as_ref()
        .and_then(|value| value.webhook.as_ref())
        .filter(|value| value.provider.as_deref() == Some("slack"))
    else {
        return (
            StatusCode::NOT_FOUND,
            "trigger is not a Slack source installation",
        )
            .into_response();
    };
    if trigger.suspend {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "source installation is suspended",
        )
            .into_response();
    }
    let Some(secret_ref) = webhook.secret.as_ref() else {
        return (
            StatusCode::UNAUTHORIZED,
            "Slack signing secret is not configured",
        )
            .into_response();
    };
    let secrets = match resolve_store_secret_values(&state.inner, &project, &secret_ref.from_ref) {
        Ok(values) => values,
        Err(error) => return (StatusCode::UNAUTHORIZED, error).into_response(),
    };
    if let Err(error) = verify_slack_signature(
        &secrets,
        webhook.timestamp_tolerance_secs,
        &headers,
        &body,
        chrono::Utc::now().timestamp(),
    ) {
        warn!(
            trigger = %trigger_name,
            project = %project,
            reason = %error,
            "Slack source authentication failed"
        );
        return (StatusCode::UNAUTHORIZED, error).into_response();
    }

    let installation_id = webhook
        .installation_id
        .as_deref()
        .unwrap_or(trigger_name.as_str());
    let normalized = if is_json {
        let payload: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid Slack JSON").into_response(),
        };
        if payload.get("type").and_then(serde_json::Value::as_str) == Some("url_verification") {
            let challenge = payload
                .get("challenge")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            return (
                StatusCode::OK,
                axum::Json(serde_json::json!({"challenge": challenge})),
            )
                .into_response();
        }
        normalize_slack_event(&payload, installation_id)
    } else {
        let form: std::collections::HashMap<String, String> =
            match serde_urlencoded::from_bytes(&body) {
                Ok(value) => value,
                Err(_) => {
                    return (StatusCode::BAD_REQUEST, "invalid Slack form body").into_response();
                }
            };
        let payload = match form
            .get("payload")
            .and_then(|value| serde_json::from_str(value).ok())
        {
            Some(value) => value,
            None => {
                return (StatusCode::BAD_REQUEST, "missing Slack interaction payload")
                    .into_response();
            }
        };
        normalize_slack_interaction(&payload, installation_id, &secrets)
    };
    let normalized = match normalized {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
    };
    let payload_hash = hex::encode(Sha256::digest(&body));
    let repository = AsyncSourceRepository::new(state.inner.async_database.clone());
    let result = match repository
        .ingest(IngestSourceEvent {
            project_id: project.clone(),
            event: normalized,
            payload_hash,
            raw_payload_ref: None,
        })
        .await
    {
        Ok(value) => value,
        Err(error) => {
            warn!(trigger = %trigger_name, error = %error, "Slack source persistence failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "source persistence failed",
            )
                .into_response();
        }
    };
    if !result.inserted {
        crate::server::process_metrics::record_source_dedup(
            &state.inner,
            &result.event.project_id,
            &result.event.provider,
        );
    }
    info!(
        provider = "slack",
        installation_hash = %short_digest(installation_id),
        external_event_hash = %short_digest(&result.event.external_event_id),
        source_event_id = %result.event.id,
        inserted = result.inserted,
        "Slack source event durably accepted"
    );
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": if result.inserted { "accepted" } else { "deduplicated" },
            "source_event_id": result.event.id,
            "deep_link": format!("orchestrator://sources/{}", result.event.id),
        })),
    )
        .into_response()
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
    do_webhook(state, headers, trigger_name, project, body).await
}

async fn handle_webhook_with_project(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Path((project, trigger_name)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    do_webhook(state, headers, trigger_name, project, body).await
}

async fn do_webhook(
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

    // ── Resolve CRD plugins (if crdRef is set) ───────────────────────────
    let crd_plugins = trigger_webhook_cfg
        .and_then(|wh_cfg| wh_cfg.crd_ref.as_ref())
        .and_then(|crd_kind| {
            active_config.as_ref().and_then(|ac| {
                ac.config
                    .custom_resource_definitions
                    .get(crd_kind)
                    .map(|crd| crd.plugins.clone())
            })
        });
    let has_crd_interceptor = crd_plugins.as_ref().is_some_and(|ps| {
        ps.iter().any(|p| {
            p.phase.as_deref() == Some(agent_orchestrator::crd::plugins::PHASE_WEBHOOK_AUTHENTICATE)
        })
    });

    // ── Signature verification (CRD interceptor → per-trigger → global) ─
    if has_crd_interceptor {
        // CRD interceptor handles authentication — run all authenticate-phase plugins
        let crd_kind = trigger_webhook_cfg
            .and_then(|wh_cfg| wh_cfg.crd_ref.as_deref())
            .unwrap_or("");
        let plugins = crd_plugins.as_deref().unwrap_or(&[]);
        let auth_plugins = agent_orchestrator::crd::plugins::plugins_for_phase(
            plugins,
            agent_orchestrator::crd::plugins::PHASE_WEBHOOK_AUTHENTICATE,
        );
        let header_map = extract_headers_map(&headers);
        let body_str = String::from_utf8_lossy(&body);
        let plugin_ctx = agent_orchestrator::crd::plugins::PluginExecutionContext {
            runner: &agent_orchestrator::config::RunnerConfig::default(),
            plugin_policy: &state.inner.plugin_policy,
            db_path: Some(&state.inner.db_path),
        };
        for plugin in auth_plugins {
            if let Err(e) = agent_orchestrator::crd::plugins::execute_interceptor(
                plugin,
                crd_kind,
                &header_map,
                &body_str,
                &plugin_ctx,
            )
            .await
            {
                warn!(
                    trigger = trigger_name.as_str(),
                    plugin = plugin.name.as_str(),
                    reason = %e,
                    "CRD interceptor rejected webhook"
                );
                return (StatusCode::UNAUTHORIZED, e.to_string()).into_response();
            }
        }
    } else {
        // Standard HMAC verification path
        let verification_result = if let Some(wh_cfg) = trigger_webhook_cfg {
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
                Ok(())
            }
        } else if let Some(ref global_secret) = state.secret {
            verify_with_single_secret(global_secret, "x-webhook-signature", &headers, &body)
        } else {
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
    }

    // ── Parse JSON body ─────────────────────────────────────────────────
    let mut payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid JSON body").into_response();
            }
        }
    };

    // ── CRD transformer plugins (payload normalization) ─────────────────
    if let Some(ref plugins) = crd_plugins {
        let crd_kind = trigger_webhook_cfg
            .and_then(|wh_cfg| wh_cfg.crd_ref.as_deref())
            .unwrap_or("");
        let transform_plugins = agent_orchestrator::crd::plugins::plugins_for_phase(
            plugins,
            agent_orchestrator::crd::plugins::PHASE_WEBHOOK_TRANSFORM,
        );
        let transform_ctx = agent_orchestrator::crd::plugins::PluginExecutionContext {
            runner: &agent_orchestrator::config::RunnerConfig::default(),
            plugin_policy: &state.inner.plugin_policy,
            db_path: Some(&state.inner.db_path),
        };
        for plugin in transform_plugins {
            match agent_orchestrator::crd::plugins::execute_transformer(
                plugin,
                crd_kind,
                &payload,
                &transform_ctx,
            )
            .await
            {
                Ok(transformed) => {
                    info!(
                        trigger = trigger_name.as_str(),
                        plugin = plugin.name.as_str(),
                        "CRD transformer applied"
                    );
                    payload = transformed;
                }
                Err(e) => {
                    warn!(
                        trigger = trigger_name.as_str(),
                        plugin = plugin.name.as_str(),
                        error = %e,
                        "CRD transformer failed"
                    );
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
                }
            }
        }
    }

    // ── Resolve trigger config for canonical fire ─────────────────────────
    let trigger_cfg = active_config.as_ref().and_then(|ac| {
        ac.config
            .projects
            .get(&project)
            .and_then(|p| p.triggers.get(&trigger_name))
    });
    let Some(trigger_cfg) = trigger_cfg else {
        let json = serde_json::json!({
            "error": format!("trigger '{}' not found in project '{}'", trigger_name, project),
            "trigger": trigger_name,
        });
        return (StatusCode::NOT_FOUND, axum::Json(json)).into_response();
    };

    // ── Canonical trigger fire (full engine semantics) ──────────────────
    match fire_trigger_canonical(
        &state.inner,
        &trigger_name,
        &project,
        trigger_cfg,
        Some(&payload),
    )
    .await
    {
        Ok(task_id) => {
            info!(
                trigger = trigger_name.as_str(),
                project = project.as_str(),
                task_id = task_id.as_str(),
                "webhook trigger fired"
            );

            // Broadcast for other event-driven triggers; exclude the one we just
            // fired to prevent duplicate task creation.
            broadcast_task_event(
                &state.inner,
                TriggerEventPayload {
                    event_type: "webhook".to_string(),
                    task_id: String::new(),
                    payload: Some(payload),
                    project: Some(project.clone()),
                    exclude_trigger: Some((trigger_name.clone(), project.clone())),
                },
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct SlackActionToken {
    attention_item_id: String,
    expected_version: i64,
    action: String,
    expires_at: i64,
}

fn resolve_store_secret_values(
    state: &InnerState,
    project: &str,
    store_name: &str,
) -> Result<Vec<String>, String> {
    let active = agent_orchestrator::config_load::read_active_config(state)
        .map_err(|error| format!("config error: {error}"))?;
    let store = active
        .config
        .projects
        .get(project)
        .and_then(|value| value.secret_stores.get(store_name))
        .ok_or_else(|| format!("SecretStore '{store_name}' not found"))?;
    if store.data.is_empty() {
        return Err(format!("SecretStore '{store_name}' is empty"));
    }
    Ok(store.data.values().cloned().collect())
}

fn verify_slack_signature(
    secrets: &[String],
    tolerance_secs: u64,
    headers: &HeaderMap,
    body: &[u8],
    now_unix: i64,
) -> Result<(), String> {
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "missing Slack request timestamp".to_string())?;
    let timestamp_unix = timestamp
        .parse::<i64>()
        .map_err(|_| "invalid Slack request timestamp".to_string())?;
    if now_unix.abs_diff(timestamp_unix) > tolerance_secs {
        return Err("stale Slack request timestamp".to_string());
    }
    let signature = headers
        .get("x-slack-signature")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("v0="))
        .ok_or_else(|| "missing or invalid Slack signature".to_string())?;
    let expected = hex::decode(signature).map_err(|_| "invalid Slack signature".to_string())?;
    let mut base = format!("v0:{timestamp}:").into_bytes();
    base.extend_from_slice(body);
    for secret in secrets {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|_| "invalid Slack signing secret".to_string())?;
        mac.update(&base);
        if mac.verify_slice(&expected).is_ok() {
            return Ok(());
        }
    }
    Err("invalid Slack signature".to_string())
}

fn normalize_slack_event(
    payload: &serde_json::Value,
    installation_id: &str,
) -> Result<NormalizedSourceEvent, String> {
    if payload.get("type").and_then(serde_json::Value::as_str) != Some("event_callback") {
        return Err("unsupported Slack event envelope".to_string());
    }
    let external_event_id = payload
        .get("event_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Slack event_id is required".to_string())?;
    let event = payload
        .get("event")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Slack event object is required".to_string())?;
    let event_type = event
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("system");
    if event_type == "reaction_added" {
        return normalize_slack_reaction_added(external_event_id, event, installation_id);
    }
    let actor_id = event
        .get("user")
        .or_else(|| event.get("bot_id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let timestamp = event
        .get("ts")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("0");
    let thread = event.get("thread_ts").and_then(serde_json::Value::as_str);
    let channel = event.get("channel").and_then(serde_json::Value::as_str);
    let text = event
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(bounded_summary);
    let command = text.as_deref().and_then(parse_text_command);
    let is_bot = event.get("bot_id").is_some()
        || event.get("subtype").and_then(serde_json::Value::as_str) == Some("bot_message");
    Ok(NormalizedSourceEvent {
        provider: "slack".to_string(),
        installation_id: installation_id.to_string(),
        external_event_id: external_event_id.to_string(),
        kind: if is_bot {
            SourceEventKind::System
        } else if command.is_some() {
            SourceEventKind::Command
        } else if matches!(event_type, "message" | "app_mention") {
            SourceEventKind::Message
        } else {
            SourceEventKind::System
        },
        reaction: None,
        actor: ExternalActorRef {
            external_id: actor_id.to_string(),
            display_name: None,
        },
        conversation: channel.map(|conversation_id| ConversationRef {
            conversation_id: conversation_id.to_string(),
            thread_id: Some(thread.unwrap_or(timestamp).to_string()),
            top_level: thread.is_none(),
        }),
        text_summary: text,
        command,
        attachments: Vec::new(),
        occurred_at: slack_timestamp_to_rfc3339(timestamp),
    })
}

fn normalize_slack_reaction_added(
    external_event_id: &str,
    event: &serde_json::Map<String, serde_json::Value>,
    installation_id: &str,
) -> Result<NormalizedSourceEvent, String> {
    let actor_id = required_slack_string(event.get("user"), "slack_reaction_missing_actor")?;
    let reaction_name =
        required_slack_string(event.get("reaction"), "slack_reaction_missing_name")?;
    if reaction_name.len() > 128
        || !reaction_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '+' | '-')
        })
    {
        return Err("slack_reaction_invalid_name".to_string());
    }
    let item = event
        .get("item")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "slack_reaction_missing_item".to_string())?;
    let target_kind =
        required_slack_string(item.get("type"), "slack_reaction_missing_target_type")?;
    let (target_external_id, conversation) = match target_kind {
        "message" => {
            let channel = required_slack_string(
                item.get("channel"),
                "slack_reaction_missing_message_channel",
            )?;
            let message_ts =
                required_slack_string(item.get("ts"), "slack_reaction_missing_message_ts")?;
            if !is_valid_slack_timestamp(message_ts) {
                return Err("slack_reaction_invalid_message_ts".to_string());
            }
            (
                format!("{channel}:{message_ts}"),
                Some(ConversationRef {
                    conversation_id: channel.to_string(),
                    thread_id: Some(message_ts.to_string()),
                    top_level: false,
                }),
            )
        }
        "file" => (
            required_slack_string(item.get("file"), "slack_reaction_missing_file_id")?.to_string(),
            None,
        ),
        "file_comment" => {
            let file = required_slack_string(item.get("file"), "slack_reaction_missing_file_id")?;
            let comment = required_slack_string(
                item.get("file_comment"),
                "slack_reaction_missing_file_comment_id",
            )?;
            (format!("{file}:{comment}"), None)
        }
        _ => return Err("slack_reaction_unsupported_target".to_string()),
    };
    let event_ts = required_slack_string(event.get("event_ts"), "slack_reaction_missing_event_ts")?;
    if !is_valid_slack_timestamp(event_ts) {
        return Err("slack_reaction_invalid_event_ts".to_string());
    }
    Ok(NormalizedSourceEvent {
        provider: "slack".to_string(),
        installation_id: installation_id.to_string(),
        external_event_id: external_event_id.to_string(),
        kind: SourceEventKind::ReactionAdded,
        reaction: Some(SourceReactionRef {
            name: reaction_name.to_string(),
            target: ExternalArtifactRef {
                kind: target_kind.to_string(),
                external_id: target_external_id,
                url: None,
            },
        }),
        actor: ExternalActorRef {
            external_id: actor_id.to_string(),
            display_name: None,
        },
        conversation,
        text_summary: None,
        command: None,
        attachments: Vec::new(),
        occurred_at: slack_timestamp_to_rfc3339(event_ts),
    })
}

fn required_slack_string<'a>(
    value: Option<&'a serde_json::Value>,
    error_code: &str,
) -> Result<&'a str, String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error_code.to_string())
}

fn is_valid_slack_timestamp(value: &str) -> bool {
    let Some((seconds, fraction)) = value.split_once('.') else {
        return false;
    };
    !seconds.is_empty()
        && seconds.chars().all(|character| character.is_ascii_digit())
        && seconds.parse::<i64>().is_ok_and(|seconds| seconds > 0)
        && !fraction.is_empty()
        && fraction.len() <= 9
        && fraction.chars().all(|character| character.is_ascii_digit())
}

fn normalize_slack_interaction(
    payload: &serde_json::Value,
    installation_id: &str,
    secrets: &[String],
) -> Result<NormalizedSourceEvent, String> {
    if payload.get("type").and_then(serde_json::Value::as_str) != Some("block_actions") {
        return Err("unsupported Slack interaction type".to_string());
    }
    let action = payload
        .get("actions")
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .ok_or_else(|| "Slack interaction action is required".to_string())?;
    let action_id = action
        .get("action_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Slack action_id is required".to_string())?;
    let value = action
        .get("value")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Slack signed action value is required".to_string())?;
    let token = verify_slack_action_token(value, secrets, chrono::Utc::now().timestamp())?;
    if token.action != action_id {
        return Err("Slack action token does not match action_id".to_string());
    }
    let command = match action_id {
        "approve" | "approve_decision" => SourceCommand::Approve {
            attention_item_id: token.attention_item_id,
            expected_version: token.expected_version,
        },
        "reject" | "reject_decision" => SourceCommand::Reject {
            attention_item_id: token.attention_item_id,
            expected_version: token.expected_version,
        },
        "retry" | "retry_failed_item" => SourceCommand::Retry {
            attention_item_id: token.attention_item_id,
            expected_version: token.expected_version,
        },
        "open_console" => SourceCommand::OpenConsole,
        _ => return Err("unsupported Slack action".to_string()),
    };
    let actor_id = payload
        .pointer("/user/id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let channel = payload
        .pointer("/channel/id")
        .and_then(serde_json::Value::as_str);
    let message_ts = payload
        .pointer("/container/message_ts")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("0");
    let action_ts = action
        .get("action_ts")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(message_ts);
    let identity = format!(
        "{}:{}:{}:{}:{}",
        payload
            .pointer("/team/id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("-"),
        actor_id,
        channel.unwrap_or("-"),
        message_ts,
        action_ts
    );
    Ok(NormalizedSourceEvent {
        provider: "slack".to_string(),
        installation_id: installation_id.to_string(),
        external_event_id: format!("interaction-{}", short_digest(&identity)),
        kind: SourceEventKind::Command,
        reaction: None,
        actor: ExternalActorRef {
            external_id: actor_id.to_string(),
            display_name: None,
        },
        conversation: channel.map(|conversation_id| ConversationRef {
            conversation_id: conversation_id.to_string(),
            thread_id: Some(message_ts.to_string()),
            top_level: false,
        }),
        text_summary: Some(format!("Slack interactive action: {action_id}")),
        command: Some(command),
        attachments: Vec::new(),
        occurred_at: slack_timestamp_to_rfc3339(action_ts),
    })
}

fn verify_slack_action_token(
    token: &str,
    secrets: &[String],
    now_unix: i64,
) -> Result<SlackActionToken, String> {
    let (payload_hex, signature_hex) = token
        .split_once('.')
        .ok_or_else(|| "invalid Slack action token".to_string())?;
    let payload = hex::decode(payload_hex).map_err(|_| "invalid Slack action token".to_string())?;
    let signature =
        hex::decode(signature_hex).map_err(|_| "invalid Slack action token".to_string())?;
    let verified = secrets.iter().any(|secret| {
        HmacSha256::new_from_slice(secret.as_bytes())
            .map(|mut mac| {
                mac.update(&payload);
                mac.verify_slice(&signature).is_ok()
            })
            .unwrap_or(false)
    });
    if !verified {
        return Err("invalid Slack action token signature".to_string());
    }
    let parsed: SlackActionToken =
        serde_json::from_slice(&payload).map_err(|_| "invalid Slack action token".to_string())?;
    if parsed.expires_at < now_unix {
        return Err("expired Slack action token".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
fn sign_slack_action_token(token: &SlackActionToken, secret: &str) -> String {
    let payload = serde_json::to_vec(token).expect("serialize test action token");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("test HMAC secret");
    mac.update(&payload);
    format!(
        "{}.{}",
        hex::encode(&payload),
        hex::encode(mac.finalize().into_bytes())
    )
}

fn parse_text_command(text: &str) -> Option<SourceCommand> {
    match text.trim().to_ascii_lowercase().as_str() {
        "orchestrator branch" | "/orchestrator branch" => Some(SourceCommand::Branch),
        "orchestrator cancel" | "/orchestrator cancel" => Some(SourceCommand::Cancel),
        "orchestrator add-context" | "/orchestrator add-context" => Some(SourceCommand::AddContext),
        "orchestrator open-console" | "/orchestrator open-console" => {
            Some(SourceCommand::OpenConsole)
        }
        _ => None,
    }
}

fn bounded_summary(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(500)
        .collect()
}

fn slack_timestamp_to_rfc3339(value: &str) -> String {
    let (seconds, nanos) = value
        .split_once('.')
        .and_then(|(seconds, fraction)| {
            let seconds = seconds.parse::<i64>().ok()?;
            let nanos = format!("{fraction:0<9}").parse::<u32>().ok()?;
            Some((seconds, nanos))
        })
        .unwrap_or((0, 0));
    chrono::DateTime::from_timestamp(seconds, nanos)
        .unwrap_or(chrono::DateTime::UNIX_EPOCH)
        .to_rfc3339()
}

fn short_digest(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Extract HTTP headers into a HashMap for plugin env injection.
fn extract_headers_map(headers: &HeaderMap) -> std::collections::HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_string(), val.to_string()))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn slack_headers(secret: &str, timestamp: i64, body: &[u8]) -> HeaderMap {
        let base = [format!("v0:{timestamp}:").as_bytes(), body].concat();
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC");
        mac.update(&base);
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-slack-request-timestamp",
            timestamp.to_string().parse().expect("timestamp header"),
        );
        headers.insert(
            "x-slack-signature",
            signature.parse().expect("signature header"),
        );
        headers
    }

    #[test]
    fn slack_signature_accepts_valid_and_rejects_stale_or_tampered() {
        let secret = "signing-secret";
        let timestamp = 1_700_000_000;
        let body = br#"{"type":"event_callback"}"#;
        let headers = slack_headers(secret, timestamp, body);
        assert!(verify_slack_signature(&[secret.into()], 300, &headers, body, timestamp).is_ok());
        assert!(
            verify_slack_signature(&[secret.into()], 300, &headers, body, timestamp + 301)
                .expect_err("stale")
                .contains("stale")
        );
        assert!(
            verify_slack_signature(&[secret.into()], 300, &headers, b"tampered", timestamp)
                .expect_err("tampered")
                .contains("invalid")
        );
    }

    #[test]
    fn slack_message_normalizes_thread_and_explicit_branch() {
        let payload = serde_json::json!({
            "type": "event_callback",
            "event_id": "Ev01",
            "event": {
                "type": "message",
                "user": "U01",
                "channel": "C01",
                "text": "orchestrator branch",
                "ts": "1700000000.100",
                "thread_ts": "1699999999.000"
            }
        });
        let event = normalize_slack_event(&payload, "install-1").expect("normalize");
        assert_eq!(event.provider, "slack");
        assert_eq!(event.kind, SourceEventKind::Command);
        assert_eq!(event.command, Some(SourceCommand::Branch));
        let conversation = event.conversation.expect("conversation");
        assert_eq!(conversation.thread_id.as_deref(), Some("1699999999.000"));
        assert!(!conversation.top_level);
    }

    fn slack_reaction_payload() -> serde_json::Value {
        serde_json::json!({
            "type": "event_callback",
            "event_id": "Ev-reaction-01",
            "event": {
                "type": "reaction_added",
                "user": "U-reactor",
                "reaction": "agent_docs",
                "item": {
                    "type": "message",
                    "channel": "C-source",
                    "ts": "1700000000.000001"
                },
                "event_ts": "1700000001.000002"
            }
        })
    }

    #[test]
    fn slack_reaction_normalizes_actor_name_message_target_and_occurrence() {
        let event =
            normalize_slack_event(&slack_reaction_payload(), "install-1").expect("normalize");
        assert_eq!(event.kind, SourceEventKind::ReactionAdded);
        assert_eq!(event.actor.external_id, "U-reactor");
        assert_eq!(event.occurred_at, "2023-11-14T22:13:21.000002+00:00");
        assert!(event.text_summary.is_none());
        assert!(event.command.is_none());
        let reaction = event.reaction.expect("reaction");
        assert_eq!(reaction.name, "agent_docs");
        assert_eq!(reaction.target.kind, "message");
        assert_eq!(reaction.target.external_id, "C-source:1700000000.000001");
        assert!(reaction.target.url.is_none());
        let conversation = event.conversation.expect("conversation");
        assert_eq!(conversation.conversation_id, "C-source");
        assert_eq!(conversation.thread_id.as_deref(), Some("1700000000.000001"));
        assert!(!conversation.top_level);
    }

    #[test]
    fn slack_file_reaction_is_typed_without_conversation_or_body() {
        let mut payload = slack_reaction_payload();
        payload["event"]["item"] = serde_json::json!({
            "type": "file",
            "file": "F-source"
        });
        let event = normalize_slack_event(&payload, "install-1").expect("normalize");
        assert_eq!(event.kind, SourceEventKind::ReactionAdded);
        assert!(event.conversation.is_none());
        assert!(event.text_summary.is_none());
        let target = event.reaction.expect("reaction").target;
        assert_eq!(target.kind, "file");
        assert_eq!(target.external_id, "F-source");
    }

    #[test]
    fn slack_reaction_missing_or_invalid_fields_return_stable_codes() {
        let mut missing_actor = slack_reaction_payload();
        missing_actor["event"]
            .as_object_mut()
            .expect("event")
            .remove("user");
        let mut missing_reaction = slack_reaction_payload();
        missing_reaction["event"]
            .as_object_mut()
            .expect("event")
            .remove("reaction");
        let mut missing_channel = slack_reaction_payload();
        missing_channel["event"]["item"]
            .as_object_mut()
            .expect("item")
            .remove("channel");
        let mut invalid_name = slack_reaction_payload();
        invalid_name["event"]["reaction"] = serde_json::json!(":agent docs:");
        let mut invalid_event_ts = slack_reaction_payload();
        invalid_event_ts["event"]["event_ts"] = serde_json::json!("not-a-timestamp");

        for (payload, expected) in [
            (missing_actor, "slack_reaction_missing_actor"),
            (missing_reaction, "slack_reaction_missing_name"),
            (missing_channel, "slack_reaction_missing_message_channel"),
            (invalid_name, "slack_reaction_invalid_name"),
            (invalid_event_ts, "slack_reaction_invalid_event_ts"),
        ] {
            assert_eq!(
                normalize_slack_event(&payload, "install-1").expect_err(expected),
                expected
            );
        }
    }

    #[test]
    fn slack_action_token_is_signed_expiring_and_action_bound() {
        let token = SlackActionToken {
            attention_item_id: "attn-1".into(),
            expected_version: 3,
            action: "approve".into(),
            expires_at: 1_700_000_100,
        };
        let encoded = sign_slack_action_token(&token, "secret");
        let parsed = verify_slack_action_token(&encoded, &["secret".into()], 1_700_000_000)
            .expect("verified token");
        assert_eq!(parsed.attention_item_id, "attn-1");
        assert!(verify_slack_action_token(&encoded, &["other".into()], 1_700_000_000).is_err());
        assert!(verify_slack_action_token(&encoded, &["secret".into()], 1_700_000_101).is_err());
    }

    #[test]
    fn slack_interactions_normalize_to_closed_attention_commands() {
        for (action, expected) in [
            (
                "approve",
                SourceCommand::Approve {
                    attention_item_id: "attn-1".into(),
                    expected_version: 3,
                },
            ),
            (
                "retry_failed_item",
                SourceCommand::Retry {
                    attention_item_id: "attn-1".into(),
                    expected_version: 3,
                },
            ),
        ] {
            let token = SlackActionToken {
                attention_item_id: "attn-1".into(),
                expected_version: 3,
                action: action.into(),
                expires_at: chrono::Utc::now().timestamp() + 60,
            };
            let payload = serde_json::json!({
                "type": "block_actions",
                "team": {"id": "T01"},
                "user": {"id": "U01"},
                "channel": {"id": "C01"},
                "container": {"message_ts": "1700000000.100"},
                "actions": [{
                    "action_id": action,
                    "action_ts": "1700000001.100",
                    "value": sign_slack_action_token(&token, "secret")
                }]
            });
            let event = normalize_slack_interaction(&payload, "install-1", &["secret".into()])
                .expect("normalize interaction");
            assert_eq!(event.kind, SourceEventKind::Command);
            assert_eq!(event.command, Some(expected));
        }
    }
}
