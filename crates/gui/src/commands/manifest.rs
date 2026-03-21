use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ValidateResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportResult {
    pub content: String,
    pub format: String,
}

/// Validate a YAML manifest (operator+).
#[tauri::command]
pub async fn manifest_validate(
    state: State<'_, Arc<AppState>>,
    content: String,
    project_id: Option<String>,
) -> Result<ValidateResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .manifest_validate(orchestrator_proto::ManifestValidateRequest {
            content,
            project_id,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(ValidateResult {
        valid: inner.valid,
        errors: inner.errors,
        message: inner.message,
    })
}

/// Export all resources as YAML or JSON (read_only+).
#[tauri::command]
pub async fn manifest_export(
    state: State<'_, Arc<AppState>>,
    output_format: Option<String>,
) -> Result<ExportResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .manifest_export(orchestrator_proto::ManifestExportRequest {
            output_format: output_format.unwrap_or_else(|| "yaml".into()),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(ExportResult {
        content: inner.content,
        format: inner.format,
    })
}
