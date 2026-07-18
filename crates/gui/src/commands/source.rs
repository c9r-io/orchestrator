use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct SourceEvent {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub installation_id: String,
    pub external_event_id: String,
    pub event_type: String,
    pub reaction_name: Option<String>,
    pub reaction_target_kind: Option<String>,
    pub reaction_target_id: Option<String>,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub occurred_at: String,
    pub received_at: String,
    pub routing_state: String,
    pub routing_attempts: i64,
    pub routed_task_id: Option<String>,
    pub last_error_code: Option<String>,
    pub automation_route_id: Option<String>,
    pub automation_status: Option<String>,
    pub automation_binding_name: Option<String>,
    pub automation_template_name: Option<String>,
    pub automation_template_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationRoute {
    pub id: String,
    pub project_id: String,
    pub source_event_id: String,
    pub provider: String,
    pub reaction: String,
    pub binding_name: String,
    pub binding_revision: String,
    pub template_name: String,
    pub template_hash: String,
    pub status: String,
    pub error_code: Option<String>,
    pub error_category: Option<String>,
    pub task_id: Option<String>,
    pub permalink: Option<String>,
    pub request_id: String,
    pub generation: i64,
    pub version: i64,
    pub attempt_count: i64,
    pub max_attempts: i64,
    pub next_attempt_at: Option<String>,
    pub suspended_scope: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationAttempt {
    pub attempt_no: i64,
    pub generation: i64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub result_state: Option<String>,
    pub error_code: Option<String>,
    pub error_category: Option<String>,
    pub retry_after_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationDetail {
    pub route: SourceAutomationRoute,
    pub attempts: Vec<SourceAutomationAttempt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationPage {
    pub routes: Vec<SourceAutomationRoute>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationStatus {
    pub project_id: String,
    pub backlog_count: u64,
    pub oldest_age_seconds: u64,
    pub active_leases: u64,
    pub retrying_count: u64,
    pub needs_attention_count: u64,
    pub failure_categories: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationDelta {
    pub cursor: i64,
    pub route_version: i64,
    pub state: String,
    pub error_code: Option<String>,
    pub route: Option<SourceAutomationRoute>,
    pub changed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceConnection {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub display_label: String,
    pub provisioning_mode: String,
    pub installation_id: String,
    pub installation_id_digest: String,
    pub enterprise_id_digest: Option<String>,
    pub owner_daemon_id: String,
    pub generation: i64,
    pub version: i64,
    pub state: String,
    pub capabilities: Vec<String>,
    pub scopes: Vec<String>,
    pub trigger_name: Option<String>,
    pub last_delivery_at: Option<String>,
    pub last_acked_cursor: i64,
    pub delivery_lag: i64,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub reauthorized_at: Option<String>,
    pub disconnected_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceConnectionIntent {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub provisioning_mode: String,
    pub status: String,
    pub connection_id: Option<String>,
    pub error_code: Option<String>,
    pub expires_at: String,
    pub authorize_url: Option<String>,
    pub connection: Option<SourceConnection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceConnectionCatalog {
    pub protocol_version: u32,
    pub gateway_configured: bool,
    pub permalink_proxy: bool,
    pub modes: Vec<SourceConnectionMode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceConnectionMode {
    pub mode: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceConnectionDelta {
    pub cursor: i64,
    pub connection_version: i64,
    pub state: String,
    pub error_code: Option<String>,
    pub request_id: Option<String>,
    pub connection: Option<SourceConnection>,
    pub changed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceTemplatePreview {
    pub name: String,
    pub skill_name: String,
    pub skill_invocation: String,
    pub skill_args: Vec<String>,
    pub goal: String,
    pub workflow: String,
    pub workspace: String,
    pub start: bool,
    pub initial_vars: std::collections::HashMap<String, String>,
    pub revision: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceBindingSimulation {
    pub status: String,
    pub reason: String,
    pub resolved_role: Option<String>,
    pub binding_id: Option<String>,
    pub template_ref: Option<String>,
    pub binding_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationSimulation {
    pub match_result: Option<SourceBindingSimulation>,
    pub rendered: Option<SourceTemplatePreview>,
    pub mutation_performed: bool,
    pub network_performed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationCatalog {
    pub project_id: String,
    pub templates: Vec<SourceAutomationTemplate>,
    pub bindings: Vec<SourceAutomationBinding>,
    pub installations: Vec<SourceAutomationInstallation>,
    pub workflows: Vec<String>,
    pub workspaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationTemplate {
    pub name: String,
    pub revision: String,
    pub skill_name: String,
    pub skill_invocation: String,
    pub skill_args: Vec<String>,
    pub workflow: String,
    pub workspace: String,
    pub start: bool,
    pub initial_vars: std::collections::HashMap<String, String>,
    pub goal_template: String,
    pub allowed_variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationBinding {
    pub name: String,
    pub revision: String,
    pub trigger_ref: String,
    pub installation_id: String,
    pub reaction: String,
    pub channels: Vec<String>,
    pub all_channels: bool,
    pub template_ref: String,
    pub allowed_actor_roles: Vec<String>,
    pub suspended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceAutomationInstallation {
    pub trigger_name: String,
    pub installation_id: String,
    pub actor_ids: Vec<String>,
    pub actor_roles: Vec<String>,
    pub suspended: bool,
    pub reaction_routing: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceBinding {
    pub id: String,
    pub task_id: String,
    pub provider: String,
    pub installation_id: String,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub binding_type: String,
    pub created_at: String,
}

fn event_from_proto(value: orchestrator_proto::SourceEvent) -> SourceEvent {
    let reaction = serde_json::from_str::<serde_json::Value>(&value.normalized_json)
        .ok()
        .and_then(|normalized| normalized.get("reaction").cloned());
    let reaction_field = |field: &str| {
        reaction
            .as_ref()
            .and_then(|value| value.get(field))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let reaction_target_field = |field: &str| {
        reaction
            .as_ref()
            .and_then(|value| value.get("target"))
            .and_then(|target| target.get(field))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };

    SourceEvent {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        installation_id: value.installation_id,
        external_event_id: value.external_event_id,
        event_type: value.event_type,
        reaction_name: reaction_field("name"),
        reaction_target_kind: reaction_target_field("kind"),
        reaction_target_id: reaction_target_field("external_id"),
        conversation_id: value.conversation_id,
        thread_id: value.thread_id,
        occurred_at: value.occurred_at,
        received_at: value.received_at,
        routing_state: value.routing_state,
        routing_attempts: value.routing_attempts,
        routed_task_id: value.routed_task_id,
        last_error_code: value.last_error_code,
        automation_route_id: value.automation_route_id,
        automation_status: value.automation_status,
        automation_binding_name: value.automation_binding_name,
        automation_template_name: value.automation_template_name,
        automation_template_hash: value.automation_template_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_only_bounded_reaction_provenance() {
        let event = event_from_proto(orchestrator_proto::SourceEvent {
            event_type: "reaction_added".into(),
            normalized_json: serde_json::json!({
                "reaction": {
                    "name": "agent_fix",
                    "target": {
                        "kind": "message",
                        "external_id": "C123:1712345678.000100",
                        "url": "https://example.invalid/private"
                    }
                },
                "text_summary": "private body"
            })
            .to_string(),
            ..Default::default()
        });

        assert_eq!(event.reaction_name.as_deref(), Some("agent_fix"));
        assert_eq!(event.reaction_target_kind.as_deref(), Some("message"));
        assert_eq!(
            event.reaction_target_id.as_deref(),
            Some("C123:1712345678.000100")
        );
    }

    #[test]
    fn malformed_normalized_json_has_no_reaction_projection() {
        let event = event_from_proto(orchestrator_proto::SourceEvent {
            normalized_json: "not-json".into(),
            ..Default::default()
        });

        assert!(event.reaction_name.is_none());
        assert!(event.reaction_target_kind.is_none());
        assert!(event.reaction_target_id.is_none());
    }

    #[test]
    fn oauth_browser_allowlist_rejects_lookalike_and_credential_urls() {
        assert!(oauth_authorize_url_allowed(
            "https://slack.com/oauth/v2/authorize?state=opaque"
        ));
        assert!(!oauth_authorize_url_allowed(
            "https://slack.com.evil.example/oauth/v2/authorize?state=opaque"
        ));
        assert!(!oauth_authorize_url_allowed(
            "https://slack.com@evil.example/oauth/v2/authorize?state=opaque"
        ));
        assert!(!oauth_authorize_url_allowed(
            "http://127.0.0.1:9999/oauth/v2/authorize?state=opaque"
        ));
    }
}

fn binding_from_proto(value: orchestrator_proto::SourceBinding) -> SourceBinding {
    SourceBinding {
        id: value.id,
        task_id: value.task_id,
        provider: value.provider,
        installation_id: value.installation_id,
        conversation_id: value.conversation_id,
        thread_id: value.thread_id,
        binding_type: value.binding_type,
        created_at: value.created_at,
    }
}

fn route_from_proto(value: orchestrator_proto::SourceAutomationRoute) -> SourceAutomationRoute {
    SourceAutomationRoute {
        id: value.id,
        project_id: value.project_id,
        source_event_id: value.source_event_id,
        provider: value.provider,
        reaction: value.reaction,
        binding_name: value.binding_name,
        binding_revision: value.binding_revision,
        template_name: value.template_name,
        template_hash: value.template_hash,
        status: value.status,
        error_code: value.error_code,
        error_category: value.error_category,
        task_id: value.task_id,
        permalink: value.permalink,
        request_id: value.request_id,
        generation: value.generation,
        version: value.version,
        attempt_count: value.attempt_count,
        max_attempts: value.max_attempts,
        next_attempt_at: value.next_attempt_at,
        suspended_scope: value.suspended_scope,
        created_at: value.created_at,
        updated_at: value.updated_at,
        completed_at: value.completed_at,
    }
}

fn preview_from_proto(
    value: orchestrator_proto::SourceTaskTemplatePreviewResponse,
) -> SourceTemplatePreview {
    SourceTemplatePreview {
        name: value.name,
        skill_name: value.skill_name,
        skill_invocation: value.skill_invocation,
        skill_args: value.skill_args,
        goal: value.goal,
        workflow: value.workflow,
        workspace: value.workspace,
        start: value.start,
        initial_vars: value.initial_vars,
        revision: value.revision,
        warnings: value.warnings,
    }
}

fn simulation_from_proto(
    value: orchestrator_proto::SourceTaskBindingSimulateResponse,
) -> SourceBindingSimulation {
    SourceBindingSimulation {
        status: value.status,
        reason: value.reason,
        resolved_role: value.resolved_role,
        binding_id: value.binding_id,
        template_ref: value.template_ref,
        binding_revision: value.binding_revision,
    }
}

fn connection_from_proto(value: orchestrator_proto::SourceConnection) -> SourceConnection {
    SourceConnection {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        display_label: value.display_label,
        provisioning_mode: value.provisioning_mode,
        installation_id: value.installation_id,
        installation_id_digest: value.installation_id_digest,
        enterprise_id_digest: value.enterprise_id_digest,
        owner_daemon_id: value.owner_daemon_id,
        generation: value.generation,
        version: value.version,
        state: value.state,
        capabilities: value.capabilities,
        scopes: value.scopes,
        trigger_name: value.trigger_name,
        last_delivery_at: value.last_delivery_at,
        last_acked_cursor: value.last_acked_cursor,
        delivery_lag: value.delivery_lag,
        last_error_code: value.last_error_code,
        created_at: value.created_at,
        updated_at: value.updated_at,
        reauthorized_at: value.reauthorized_at,
        disconnected_at: value.disconnected_at,
    }
}

fn intent_from_proto(
    value: orchestrator_proto::SourceConnectionIntentResponse,
) -> SourceConnectionIntent {
    SourceConnectionIntent {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        provisioning_mode: value.provisioning_mode,
        status: value.status,
        connection_id: value.connection_id,
        error_code: value.error_code,
        expires_at: value.expires_at,
        authorize_url: value.authorize_url,
        connection: value.connection.map(connection_from_proto),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_catalog_get(
    state: State<'_, Arc<AppState>>,
) -> Result<SourceConnectionCatalog, String> {
    let mut client = state.client().await?;
    client
        .source_connection_catalog_get(orchestrator_proto::SourceConnectionCatalogRequest {})
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceConnectionCatalog {
                protocol_version: value.protocol_version,
                gateway_configured: value.gateway_configured,
                permalink_proxy: value.permalink_proxy,
                modes: value
                    .modes
                    .into_iter()
                    .map(|mode| SourceConnectionMode {
                        mode: mode.mode,
                        available: mode.available,
                        unavailable_reason: mode.unavailable_reason,
                    })
                    .collect(),
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_list(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    include_disconnected: Option<bool>,
) -> Result<Vec<SourceConnection>, String> {
    let mut client = state.client().await?;
    client
        .source_connection_list(orchestrator_proto::SourceConnectionListRequest {
            project_id,
            provider: Some("slack".into()),
            include_disconnected: include_disconnected.unwrap_or(false),
            limit: 200,
        })
        .await
        .map(|response| {
            response
                .into_inner()
                .connections
                .into_iter()
                .map(connection_from_proto)
                .collect()
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_get(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    id: String,
) -> Result<SourceConnection, String> {
    let mut client = state.client().await?;
    client
        .source_connection_get(orchestrator_proto::SourceConnectionGetRequest { project_id, id })
        .await
        .map(|response| connection_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_connect(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    display_label: String,
    reason: String,
    idempotency_key: String,
) -> Result<SourceConnectionIntent, String> {
    let mut client = state.client().await?;
    client
        .source_connection_connect(orchestrator_proto::SourceConnectionConnectRequest {
            project_id,
            provider: "slack".into(),
            provisioning_mode: "managed_shared".into(),
            display_label,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| intent_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_intent_get(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    intent_id: String,
) -> Result<SourceConnectionIntent, String> {
    let mut client = state.client().await?;
    client
        .source_connection_intent_get(orchestrator_proto::SourceConnectionIntentGetRequest {
            project_id,
            intent_id,
        })
        .await
        .map(|response| intent_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_cancel(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    intent_id: String,
    reason: String,
    idempotency_key: String,
) -> Result<SourceConnectionIntent, String> {
    let mut client = state.client().await?;
    client
        .source_connection_cancel(orchestrator_proto::SourceConnectionIntentMutationRequest {
            project_id,
            intent_id,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| intent_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_reauthorize(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    id: String,
    expected_version: i64,
    reason: String,
    idempotency_key: String,
) -> Result<SourceConnectionIntent, String> {
    let mut client = state.client().await?;
    client
        .source_connection_reauthorize(orchestrator_proto::SourceConnectionMutationRequest {
            project_id,
            id,
            expected_version,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| intent_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_disconnect(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    id: String,
    expected_version: i64,
    reason: String,
    idempotency_key: String,
) -> Result<SourceConnection, String> {
    let mut client = state.client().await?;
    client
        .source_connection_disconnect(orchestrator_proto::SourceConnectionMutationRequest {
            project_id,
            id,
            expected_version,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| connection_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_connection_transfer(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    id: String,
    expected_version: i64,
    target_daemon_id: String,
    reason: String,
    idempotency_key: String,
) -> Result<SourceConnection, String> {
    let mut client = state.client().await?;
    client
        .source_connection_transfer(orchestrator_proto::SourceConnectionTransferRequest {
            project_id,
            id,
            expected_version,
            target_daemon_id,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| connection_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_source_connection_oauth(authorize_url: String) -> Result<(), String> {
    if !oauth_authorize_url_allowed(&authorize_url) {
        return Err("OAuth URL host is not allowlisted".into());
    }
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg(&authorize_url)
        .status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open")
        .arg(&authorize_url)
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", &authorize_url])
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let status: std::io::Result<std::process::ExitStatus> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "system browser is not supported",
    ));
    match status {
        Ok(status) if status.success() => Ok(()),
        _ => Err("Failed to open the system browser".into()),
    }
}

fn oauth_authorize_url_allowed(value: &str) -> bool {
    let Ok(value) = url::Url::parse(value) else {
        return false;
    };
    value.scheme() == "https"
        && value.host_str() == Some("slack.com")
        && matches!(value.port(), None | Some(443))
        && value.path() == "/oauth/v2/authorize"
        && value.query().is_some()
        && value.username().is_empty()
        && value.password().is_none()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn start_source_connection_watch(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project_id: String,
    after_cursor: Option<i64>,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let response = client
        .source_connection_watch(orchestrator_proto::SourceConnectionWatchRequest {
            project_id,
            after_cursor: after_cursor.unwrap_or_default(),
            interval_millis: 1000,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?;
    let mut stream = response.into_inner();
    let cancel = state.register_stream("source-connection").await;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                message = stream.message() => match message {
                    Ok(Some(value)) => {
                        let payload = SourceConnectionDelta {
                            cursor: value.cursor,
                            connection_version: value.connection_version,
                            state: value.state,
                            error_code: value.error_code,
                            request_id: value.request_id,
                            connection: value.connection.map(connection_from_proto),
                            changed_at: value.changed_at,
                        };
                        let _ = app.emit("source-connection-delta", &payload);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let message = crate::errors::humanize_grpc_error(&error);
                        let _ = app.emit("source-connection-watch-error", &message);
                        break;
                    }
                },
                _ = cancel.cancelled() => break,
            }
        }
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_source_connection_watch(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.cancel_stream("source-connection").await;
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_event_list(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
    task_id: Option<String>,
    routing_state: Option<String>,
) -> Result<Vec<SourceEvent>, String> {
    let mut client = state.client().await?;
    client
        .source_event_list(orchestrator_proto::SourceEventListRequest {
            project_id,
            task_id,
            routing_state,
            limit: 200,
        })
        .await
        .map(|response| {
            response
                .into_inner()
                .events
                .into_iter()
                .map(event_from_proto)
                .collect()
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_event_get(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<SourceEvent, String> {
    let mut client = state.client().await?;
    client
        .source_event_get(orchestrator_proto::SourceEventGetRequest { id })
        .await
        .map(|response| event_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_binding_list(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<Vec<SourceBinding>, String> {
    let mut client = state.client().await?;
    client
        .source_binding_list(orchestrator_proto::SourceBindingListRequest { task_id })
        .await
        .map(|response| {
            response
                .into_inner()
                .bindings
                .into_iter()
                .map(binding_from_proto)
                .collect()
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_route_get(
    state: State<'_, Arc<AppState>>,
    source_event_id: String,
) -> Result<SourceAutomationRoute, String> {
    let mut client = state.client().await?;
    client
        .source_automation_route_get(orchestrator_proto::SourceAutomationRouteGetRequest {
            source_event_id,
        })
        .await
        .map(|response| route_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_catalog_get(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<SourceAutomationCatalog, String> {
    let mut client = state.client().await?;
    client
        .source_automation_catalog_get(orchestrator_proto::SourceAutomationCatalogRequest {
            project_id,
        })
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationCatalog {
                project_id: value.project_id,
                templates: value
                    .templates
                    .into_iter()
                    .map(|item| SourceAutomationTemplate {
                        name: item.name,
                        revision: item.revision,
                        skill_name: item.skill_name,
                        skill_invocation: item.skill_invocation,
                        skill_args: item.skill_args,
                        workflow: item.workflow,
                        workspace: item.workspace,
                        start: item.start,
                        initial_vars: item.initial_vars,
                        goal_template: item.goal_template,
                        allowed_variables: item.allowed_variables,
                    })
                    .collect(),
                bindings: value
                    .bindings
                    .into_iter()
                    .map(|item| SourceAutomationBinding {
                        name: item.name,
                        revision: item.revision,
                        trigger_ref: item.trigger_ref,
                        installation_id: item.installation_id,
                        reaction: item.reaction,
                        channels: item.channels,
                        all_channels: item.all_channels,
                        template_ref: item.template_ref,
                        allowed_actor_roles: item.allowed_actor_roles,
                        suspended: item.suspended,
                    })
                    .collect(),
                installations: value
                    .installations
                    .into_iter()
                    .map(|item| SourceAutomationInstallation {
                        trigger_name: item.trigger_name,
                        installation_id: item.installation_id,
                        actor_ids: item.actor_ids,
                        actor_roles: item.actor_roles,
                        suspended: item.suspended,
                        reaction_routing: item.reaction_routing,
                    })
                    .collect(),
                workflows: value.workflows,
                workspaces: value.workspaces,
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn source_task_template_preview(
    state: State<'_, Arc<AppState>>,
    name: String,
    project_id: String,
    provider: String,
    installation_id: String,
    message_url: String,
    event_id: Option<String>,
    reaction: Option<String>,
    target_id: Option<String>,
    draft_content: Option<String>,
) -> Result<SourceTemplatePreview, String> {
    let mut client = state.client().await?;
    client
        .source_task_template_preview(orchestrator_proto::SourceTaskTemplatePreviewRequest {
            name,
            project_id,
            provider,
            installation_id,
            message_url,
            event_id,
            reaction,
            target_id,
            draft_content,
        })
        .await
        .map(|response| preview_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn source_task_binding_simulate(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    provider: String,
    installation_id: String,
    event_kind: String,
    reaction: String,
    target_kind: String,
    channel_id: String,
    external_actor_id: String,
    draft_content: Option<String>,
) -> Result<SourceBindingSimulation, String> {
    let mut client = state.client().await?;
    client
        .source_task_binding_simulate(orchestrator_proto::SourceTaskBindingSimulateRequest {
            project_id,
            provider,
            installation_id,
            event_kind,
            reaction,
            target_kind,
            channel_id,
            external_actor_id,
            draft_content,
        })
        .await
        .map(|response| simulation_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

async fn mutate_binding(
    state: &AppState,
    name: String,
    project_id: String,
    expected_revision: Option<String>,
    reason: String,
    suspend: bool,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let request = orchestrator_proto::SourceTaskBindingMutationRequest {
        name,
        project_id,
        audit: Some(orchestrator_proto::ActionAuditContext {
            reason_code: if suspend {
                "operator_source_binding_suspend".into()
            } else {
                "operator_source_binding_resume".into()
            },
            operator_reason: Some(reason),
            idempotency_key: None,
        }),
        expected_revision,
    };
    let response = if suspend {
        client.source_task_binding_suspend(request).await
    } else {
        client.source_task_binding_resume(request).await
    }
    .map_err(|error| crate::errors::humanize_grpc_error(&error))?;
    Ok(response.into_inner().message)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_task_binding_suspend(
    state: State<'_, Arc<AppState>>,
    name: String,
    project_id: String,
    expected_revision: Option<String>,
    reason: String,
) -> Result<String, String> {
    mutate_binding(&state, name, project_id, expected_revision, reason, true).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_task_binding_resume(
    state: State<'_, Arc<AppState>>,
    name: String,
    project_id: String,
    expected_revision: Option<String>,
    reason: String,
) -> Result<String, String> {
    mutate_binding(&state, name, project_id, expected_revision, reason, false).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_list(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
    route_state: Option<String>,
    provider: Option<String>,
    binding_name: Option<String>,
    task_id: Option<String>,
    page_token: Option<String>,
) -> Result<SourceAutomationPage, String> {
    let mut client = state.client().await?;
    client
        .source_automation_list(orchestrator_proto::SourceAutomationListRequest {
            project_id,
            state: route_state,
            provider,
            binding_name,
            task_id,
            page_size: 100,
            page_token,
        })
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationPage {
                routes: value.routes.into_iter().map(route_from_proto).collect(),
                next_page_token: value.next_page_token,
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_get(
    state: State<'_, Arc<AppState>>,
    route_id: String,
) -> Result<SourceAutomationDetail, String> {
    let mut client = state.client().await?;
    client
        .source_automation_get(orchestrator_proto::SourceAutomationGetRequest {
            route_id,
            attempt_limit: 100,
        })
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationDetail {
                route: route_from_proto(value.route.unwrap_or_default()),
                attempts: value
                    .attempts
                    .into_iter()
                    .map(|attempt| SourceAutomationAttempt {
                        attempt_no: attempt.attempt_no,
                        generation: attempt.generation,
                        started_at: attempt.started_at,
                        completed_at: attempt.completed_at,
                        result_state: attempt.result_state,
                        error_code: attempt.error_code,
                        error_category: attempt.error_category,
                        retry_after_seconds: attempt.retry_after_seconds,
                    })
                    .collect(),
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn source_automation_simulate(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    provider: String,
    installation_id: String,
    event_kind: String,
    reaction: String,
    target_kind: String,
    channel_id: String,
    external_actor_id: String,
    message_url: String,
    event_id: Option<String>,
    target_id: String,
    draft_binding_content: Option<String>,
) -> Result<SourceAutomationSimulation, String> {
    let mut client = state.client().await?;
    client
        .source_automation_simulate(orchestrator_proto::SourceAutomationSimulateRequest {
            project_id,
            provider,
            installation_id,
            event_kind,
            reaction,
            target_kind,
            channel_id,
            external_actor_id,
            message_url,
            event_id,
            target_id,
            draft_binding_content,
        })
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationSimulation {
                match_result: value.match_result.map(simulation_from_proto),
                rendered: value.rendered.map(preview_from_proto),
                mutation_performed: value.mutation_performed,
                network_performed: value.network_performed,
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

async fn mutate_route(
    state: &AppState,
    route_id: String,
    expected_version: i64,
    reason: String,
    idempotency_key: String,
    adopt_current_config: bool,
    replay: bool,
) -> Result<SourceAutomationRoute, String> {
    let mut client = state.client().await?;
    let request = orchestrator_proto::SourceAutomationMutationRequest {
        route_id,
        expected_version,
        reason,
        idempotency_key,
        adopt_current_config,
    };
    let response = if replay {
        client.source_automation_replay(request).await
    } else {
        client.source_automation_ignore(request).await
    }
    .map_err(|error| crate::errors::humanize_grpc_error(&error))?;
    Ok(route_from_proto(response.into_inner()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_replay(
    state: State<'_, Arc<AppState>>,
    route_id: String,
    expected_version: i64,
    reason: String,
    idempotency_key: String,
    adopt_current_config: bool,
) -> Result<SourceAutomationRoute, String> {
    mutate_route(
        &state,
        route_id,
        expected_version,
        reason,
        idempotency_key,
        adopt_current_config,
        true,
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_ignore(
    state: State<'_, Arc<AppState>>,
    route_id: String,
    expected_version: i64,
    reason: String,
    idempotency_key: String,
) -> Result<SourceAutomationRoute, String> {
    mutate_route(
        &state,
        route_id,
        expected_version,
        reason,
        idempotency_key,
        false,
        false,
    )
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_automation_status_get(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<SourceAutomationStatus, String> {
    let mut client = state.client().await?;
    client
        .source_automation_status_get(orchestrator_proto::SourceAutomationStatusRequest {
            project_id,
        })
        .await
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationStatus {
                project_id: value.project_id,
                backlog_count: value.backlog_count,
                oldest_age_seconds: value.oldest_age_seconds,
                active_leases: value.active_leases,
                retrying_count: value.retrying_count,
                needs_attention_count: value.needs_attention_count,
                failure_categories: value
                    .failure_categories
                    .into_iter()
                    .map(|item| (item.category, item.count))
                    .collect(),
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn start_source_automation_watch(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
    after_cursor: Option<i64>,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let response = client
        .source_automation_watch(orchestrator_proto::SourceAutomationWatchRequest {
            project_id,
            after_cursor: after_cursor.unwrap_or_default(),
            interval_millis: 1000,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?;
    let mut stream = response.into_inner();
    let cancel = state.register_stream("source-automation").await;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                message = stream.message() => match message {
                    Ok(Some(value)) => {
                        let payload = SourceAutomationDelta {
                            cursor: value.cursor,
                            route_version: value.route_version,
                            state: value.state,
                            error_code: value.error_code,
                            route: value.route.map(route_from_proto),
                            changed_at: value.changed_at,
                        };
                        let _ = app.emit("source-automation-delta", &payload);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let message = crate::errors::humanize_grpc_error(&error);
                        let _ = app.emit("source-automation-watch-error", &message);
                        break;
                    }
                },
                _ = cancel.cancelled() => break,
            }
        }
    });
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_source_automation_watch(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.cancel_stream("source-automation").await;
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn source_replay(state: State<'_, Arc<AppState>>, id: String) -> Result<String, String> {
    let mut client = state.client().await?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    client
        .source_replay(orchestrator_proto::SourceReplayRequest {
            id,
            audit: Some(orchestrator_proto::ActionAuditContext {
                reason_code: "operator_source_replay".into(),
                operator_reason: None,
                idempotency_key: Some(format!("gui-source-replay-{nonce}")),
            }),
        })
        .await
        .map(|response| response.into_inner().status)
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}
