use serde::Serialize;
use std::sync::Arc;
use tauri::State;

fn audit_context(
    reason_code: &str,
    operator_reason: Option<String>,
    idempotency_key: &str,
) -> Option<orchestrator_proto::ActionAuditContext> {
    Some(orchestrator_proto::ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason,
        idempotency_key: Some(idempotency_key.to_string()),
    })
}

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct AttentionAction {
    pub id: String,
    pub label: String,
    pub required_role: String,
    pub confirmation: String,
    pub input_schema_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionItem {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub step_id: Option<String>,
    pub session_id: Option<String>,
    pub source_route_id: Option<String>,
    pub source_binding_name: Option<String>,
    pub kind: String,
    pub severity: String,
    pub state: String,
    pub title: String,
    pub summary: String,
    pub requested_decision_json: Option<String>,
    pub actions: Vec<AttentionAction>,
    pub assignee: Option<String>,
    pub occurrence_count: i64,
    pub reopen_count: i64,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub last_occurred_at: String,
    pub snoozed_until: Option<String>,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionListResult {
    pub items: Vec<AttentionItem>,
    pub latest_change_id: i64,
}

pub(crate) fn item_from_proto(item: orchestrator_proto::AttentionItem) -> AttentionItem {
    AttentionItem {
        id: item.id,
        project_id: item.project_id,
        task_id: item.task_id,
        task_item_id: item.task_item_id,
        step_id: item.step_id,
        session_id: item.session_id,
        source_route_id: item.source_route_id,
        source_binding_name: item.source_binding_name,
        kind: item.kind,
        severity: item.severity,
        state: item.state,
        title: item.title,
        summary: item.summary,
        requested_decision_json: item.requested_decision_json,
        actions: item
            .actions
            .into_iter()
            .map(|action| AttentionAction {
                id: action.id,
                label: action.label,
                required_role: action.required_role,
                confirmation: action.confirmation,
                input_schema_json: action.input_schema_json,
            })
            .collect(),
        assignee: item.assignee,
        occurrence_count: item.occurrence_count,
        reopen_count: item.reopen_count,
        version: item.version,
        created_at: item.created_at,
        updated_at: item.updated_at,
        last_occurred_at: item.last_occurred_at,
        snoozed_until: item.snoozed_until,
        resolved_at: item.resolved_at,
    }
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn attention_list(
    state: State<'_, Arc<AppState>>,
    project_id: Option<String>,
    item_state: Option<String>,
    kind: Option<String>,
    severity: Option<String>,
    assignee: Option<String>,
    task_id: Option<String>,
    active_only: Option<bool>,
) -> Result<AttentionListResult, String> {
    let mut client = state.client().await?;
    let response = client
        .attention_list(orchestrator_proto::AttentionListRequest {
            project_id,
            state: item_state,
            kind,
            severity,
            assignee,
            task_id,
            limit: 200,
            active_only: active_only.unwrap_or(false),
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    Ok(AttentionListResult {
        items: response.items.into_iter().map(item_from_proto).collect(),
        latest_change_id: response.latest_change_id,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn attention_get(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<AttentionItem, String> {
    let mut client = state.client().await?;
    client
        .attention_get(orchestrator_proto::AttentionGetRequest { id })
        .await
        .map(|response| item_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn attention_claim(
    state: State<'_, Arc<AppState>>,
    id: String,
    expected_version: i64,
    idempotency_key: String,
) -> Result<AttentionItem, String> {
    let mut client = state.client().await?;
    client
        .attention_claim(orchestrator_proto::AttentionClaimRequest {
            audit: audit_context("operator_triage", None, &idempotency_key),
            id,
            expected_version,
            idempotency_key,
        })
        .await
        .map(|response| item_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn attention_snooze(
    state: State<'_, Arc<AppState>>,
    id: String,
    expected_version: i64,
    idempotency_key: String,
    until: String,
) -> Result<AttentionItem, String> {
    let mut client = state.client().await?;
    client
        .attention_snooze(orchestrator_proto::AttentionSnoozeRequest {
            audit: audit_context("operator_snooze", None, &idempotency_key),
            id,
            expected_version,
            idempotency_key,
            until,
        })
        .await
        .map(|response| item_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn attention_resolve(
    state: State<'_, Arc<AppState>>,
    id: String,
    expected_version: i64,
    idempotency_key: String,
    reason: String,
) -> Result<AttentionItem, String> {
    let mut client = state.client().await?;
    client
        .attention_resolve(orchestrator_proto::AttentionResolveRequest {
            audit: audit_context("operator_resolve", Some(reason.clone()), &idempotency_key),
            id,
            expected_version,
            idempotency_key,
            reason,
        })
        .await
        .map(|response| item_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn attention_execute_action(
    state: State<'_, Arc<AppState>>,
    id: String,
    expected_version: i64,
    idempotency_key: String,
    action_id: String,
    input_json: Option<String>,
) -> Result<AttentionItem, String> {
    let mut client = state.client().await?;
    client
        .attention_execute_action(orchestrator_proto::AttentionExecuteActionRequest {
            audit: audit_context("operator_action", None, &idempotency_key),
            id,
            expected_version,
            idempotency_key,
            action_id,
            input_json: input_json.unwrap_or_else(|| "{}".to_string()),
        })
        .await
        .map(|response| item_from_proto(response.into_inner()))
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}
