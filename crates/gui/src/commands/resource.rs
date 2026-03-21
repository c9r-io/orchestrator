use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ResourceResult {
    pub content: String,
    pub format: String,
}

/// Get resources by resource path (read_only+).
#[tauri::command]
pub async fn resource_get(
    state: State<'_, Arc<AppState>>,
    resource: String,
    output_format: Option<String>,
) -> Result<ResourceResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .get(orchestrator_proto::GetRequest {
            resource,
            selector: None,
            output_format: output_format.unwrap_or_else(|| "yaml".into()),
            project: None,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let inner = resp.into_inner();
    Ok(ResourceResult {
        content: inner.content,
        format: inner.format,
    })
}

/// Describe a resource in YAML (read_only+).
#[tauri::command]
pub async fn resource_describe(
    state: State<'_, Arc<AppState>>,
    resource: String,
    output_format: Option<String>,
) -> Result<ResourceResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .describe(orchestrator_proto::DescribeRequest {
            resource,
            output_format: output_format.unwrap_or_else(|| "yaml".into()),
            project: None,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(ResourceResult {
        content: inner.content,
        format: inner.format,
    })
}

/// Apply a resource from YAML (operator+).
#[tauri::command]
pub async fn resource_apply(
    state: State<'_, Arc<AppState>>,
    content: String,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .apply(orchestrator_proto::ApplyRequest {
            content,
            dry_run: false,
            project: None,
            prune: false,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let results: Vec<String> = resp
        .into_inner()
        .results
        .into_iter()
        .map(|r| format!("{} {} {}", r.action, r.kind, r.name))
        .collect();
    Ok(results.join("\n"))
}

/// Delete a resource (admin).
#[tauri::command]
pub async fn resource_delete(
    state: State<'_, Arc<AppState>>,
    resource: String,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .delete(orchestrator_proto::DeleteRequest {
            resource,
            force: false,
            project: None,
            dry_run: false,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}
