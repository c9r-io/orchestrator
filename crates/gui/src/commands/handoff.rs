use serde::Serialize;
use std::sync::Arc;
use tauri::State;

fn audit_context(
    reason_code: &str,
    operator_reason: Option<String>,
    idempotency_key: String,
) -> Option<orchestrator_proto::ActionAuditContext> {
    Some(orchestrator_proto::ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason,
        idempotency_key: Some(idempotency_key),
    })
}

fn generated_key(prefix: &str) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("gui-{prefix}-{nonce}")
}

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct HandoffSnapshot {
    pub id: String,
    pub task_id: String,
    pub source_event_cursor: i64,
    pub projection_version: i64,
    pub briefing: serde_json::Value,
    pub content_hash: String,
    pub state_version: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResumeBoundary {
    pub id: String,
    pub task_id: String,
    pub cycle: i64,
    pub step_id: Option<String>,
    pub task_item_id: Option<String>,
    pub provider_session_available: bool,
    pub side_effect_class: String,
    pub replay_safe: bool,
    pub reason: String,
    pub state_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResumePlan {
    pub id: String,
    pub task_id: String,
    pub boundary: Option<ResumeBoundary>,
    pub mode: String,
    pub expected_state_version: String,
    pub consequence: serde_json::Value,
    pub elevated_confirmation_required: bool,
    pub expires_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResumeExecution {
    pub execution_id: String,
    pub plan_id: String,
    pub accepted: bool,
    pub status: String,
    pub child_task_id: Option<String>,
}

fn boundary_from_proto(boundary: orchestrator_proto::ResumeBoundary) -> ResumeBoundary {
    ResumeBoundary {
        id: boundary.id,
        task_id: boundary.task_id,
        cycle: boundary.cycle,
        step_id: boundary.step_id,
        task_item_id: boundary.task_item_id,
        provider_session_available: boundary.provider_session_available,
        side_effect_class: boundary.side_effect_class,
        replay_safe: boundary.replay_safe,
        reason: boundary.reason,
        state_version: boundary.state_version,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn handoff_generate(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<HandoffSnapshot, String> {
    let mut client = state.client().await?;
    let snapshot = client
        .handoff_generate(orchestrator_proto::HandoffGenerateRequest {
            audit: audit_context("operator_handoff", None, generated_key("handoff")),
            task_id,
            source_event_cursor: None,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    Ok(HandoffSnapshot {
        id: snapshot.id,
        task_id: snapshot.task_id,
        source_event_cursor: snapshot.source_event_cursor,
        projection_version: snapshot.projection_version,
        briefing: serde_json::from_str(&snapshot.briefing_json).unwrap_or_default(),
        content_hash: snapshot.content_hash,
        state_version: snapshot.state_version,
        created_at: snapshot.created_at,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn resume_boundary_list(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<Vec<ResumeBoundary>, String> {
    let mut client = state.client().await?;
    client
        .resume_boundary_list(orchestrator_proto::ResumeBoundaryListRequest { task_id })
        .await
        .map(|response| {
            response
                .into_inner()
                .boundaries
                .into_iter()
                .map(boundary_from_proto)
                .collect()
        })
        .map_err(|error| crate::errors::humanize_grpc_error(&error))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn resume_plan(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    boundary_id: String,
    mode: String,
) -> Result<ResumePlan, String> {
    let mut client = state.client().await?;
    let plan = client
        .resume_plan(orchestrator_proto::ResumePlanRequest {
            audit: audit_context("operator_resume_plan", None, generated_key("resume-plan")),
            task_id,
            boundary_id,
            mode,
            attention_item_id: None,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    Ok(ResumePlan {
        id: plan.id,
        task_id: plan.task_id,
        boundary: plan.boundary.map(boundary_from_proto),
        mode: plan.mode,
        expected_state_version: plan.expected_state_version,
        consequence: serde_json::from_str(&plan.consequence_json).unwrap_or_default(),
        elevated_confirmation_required: plan.elevated_confirmation_required,
        expires_at: plan.expires_at,
        status: plan.status,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn resume_execute(
    state: State<'_, Arc<AppState>>,
    plan_id: String,
    expected_state_version: String,
    operator_reason: String,
    idempotency_key: String,
    elevated_confirmation: bool,
) -> Result<ResumeExecution, String> {
    let mut client = state.client().await?;
    let execution = client
        .resume_execute(orchestrator_proto::ResumeExecuteRequest {
            audit: audit_context(
                "operator_resume_execute",
                Some(operator_reason.clone()),
                idempotency_key.clone(),
            ),
            plan_id,
            expected_state_version,
            operator_reason,
            idempotency_key,
            elevated_confirmation,
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    Ok(ResumeExecution {
        execution_id: execution.execution_id,
        plan_id: execution.plan_id,
        accepted: execution.accepted,
        status: execution.status,
        child_task_id: execution.child_task_id,
    })
}
