use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct TriggerFireResult {
    pub task_id: String,
    pub message: String,
}

/// Suspend a trigger (operator+).
#[tauri::command]
pub async fn trigger_suspend(
    state: State<'_, Arc<AppState>>,
    trigger_name: String,
    project: Option<String>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .trigger_suspend(orchestrator_proto::TriggerSuspendRequest {
            trigger_name,
            project,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}

/// Resume a trigger (operator+).
#[tauri::command]
pub async fn trigger_resume(
    state: State<'_, Arc<AppState>>,
    trigger_name: String,
    project: Option<String>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .trigger_resume(orchestrator_proto::TriggerResumeRequest {
            trigger_name,
            project,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}

/// Manually fire a trigger (operator+).
#[tauri::command]
pub async fn trigger_fire(
    state: State<'_, Arc<AppState>>,
    trigger_name: String,
    project: Option<String>,
) -> Result<TriggerFireResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .trigger_fire(orchestrator_proto::TriggerFireRequest {
            trigger_name,
            project,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(TriggerFireResult {
        task_id: inner.task_id,
        message: inner.message,
    })
}
