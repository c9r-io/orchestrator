use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct SourceEvent {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub installation_id: String,
    pub external_event_id: String,
    pub event_type: String,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub occurred_at: String,
    pub received_at: String,
    pub normalized_json: String,
    pub routing_state: String,
    pub routing_attempts: i64,
    pub routed_task_id: Option<String>,
    pub last_error_code: Option<String>,
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
    SourceEvent {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        installation_id: value.installation_id,
        external_event_id: value.external_event_id,
        event_type: value.event_type,
        conversation_id: value.conversation_id,
        thread_id: value.thread_id,
        occurred_at: value.occurred_at,
        received_at: value.received_at,
        normalized_json: value.normalized_json,
        routing_state: value.routing_state,
        routing_attempts: value.routing_attempts,
        routed_task_id: value.routed_task_id,
        last_error_code: value.last_error_code,
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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
