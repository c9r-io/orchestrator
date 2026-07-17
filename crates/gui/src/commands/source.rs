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
    pub reaction_name: Option<String>,
    pub reaction_target_kind: Option<String>,
    pub reaction_target_id: Option<String>,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub occurred_at: String,
    pub received_at: String,
    pub normalized_json: String,
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
    pub source_event_id: String,
    pub reaction: String,
    pub binding_name: String,
    pub template_name: String,
    pub status: String,
    pub task_id: Option<String>,
    pub permalink: Option<String>,
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
        normalized_json: value.normalized_json,
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
        .map(|response| {
            let value = response.into_inner();
            SourceAutomationRoute {
                id: value.id,
                source_event_id: value.source_event_id,
                reaction: value.reaction,
                binding_name: value.binding_name,
                template_name: value.template_name,
                status: value.status,
                task_id: value.task_id,
                permalink: value.permalink,
            }
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
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
