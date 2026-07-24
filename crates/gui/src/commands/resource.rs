use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ResourceResult {
    pub content: String,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceSummary {
    pub kind: String,
    pub name: String,
    pub project_id: String,
    pub revision: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceCatalogResult {
    pub resources: Vec<ResourceSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceDescribeResult {
    pub content: String,
    pub format: String,
    pub resource: Option<ResourceSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceApplyResult {
    pub message: String,
    pub request_id: Option<String>,
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

/// List stable resource summaries without parsing YAML in the frontend (read_only+).
#[tauri::command]
pub async fn resource_list(
    state: State<'_, Arc<AppState>>,
    resource_type: String,
    project_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<ResourceCatalogResult, String> {
    let mut client = state.client().await?;
    let response = client
        .resource_catalog_list(orchestrator_proto::ResourceCatalogListRequest {
            resource_type,
            project: project_id,
            cursor,
            limit: limit.unwrap_or(100),
        })
        .await
        .map_err(|error| crate::errors::humanize_grpc_error(&error))?
        .into_inner();
    Ok(ResourceCatalogResult {
        resources: response
            .resources
            .into_iter()
            .map(|resource| ResourceSummary {
                kind: resource.kind,
                name: resource.name,
                project_id: resource.project_id,
                revision: resource.revision,
                source: resource.source,
            })
            .collect(),
        next_cursor: response.next_cursor,
    })
}

/// Describe a resource in YAML (read_only+).
#[tauri::command]
pub async fn resource_describe(
    state: State<'_, Arc<AppState>>,
    resource: String,
    output_format: Option<String>,
    project_id: Option<String>,
) -> Result<ResourceDescribeResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .describe(orchestrator_proto::DescribeRequest {
            resource,
            output_format: output_format.unwrap_or_else(|| "yaml".into()),
            project: project_id,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(ResourceDescribeResult {
        content: inner.content,
        format: inner.format,
        resource: inner.resource.map(|resource| ResourceSummary {
            kind: resource.kind,
            name: resource.name,
            project_id: resource.project_id,
            revision: resource.revision,
            source: resource.source,
        }),
    })
}

/// Apply a resource from YAML (operator+).
#[tauri::command(rename_all = "snake_case")]
pub async fn resource_apply(
    state: State<'_, Arc<AppState>>,
    content: String,
    project_id: Option<String>,
    expected_revision: Option<String>,
    require_absent: Option<bool>,
    reason: Option<String>,
    idempotency_key: Option<String>,
) -> Result<ResourceApplyResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .apply(orchestrator_proto::ApplyRequest {
            content,
            dry_run: false,
            project: project_id,
            prune: false,
            audit: Some(orchestrator_proto::ActionAuditContext {
                reason_code: "operator_resource_apply".into(),
                operator_reason: reason,
                idempotency_key,
            }),
            expected_revision,
            require_absent: require_absent.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let request_id = ["x-request-id", "request-id"]
        .into_iter()
        .find_map(|key| resp.metadata().get(key))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let results: Vec<String> = resp
        .into_inner()
        .results
        .into_iter()
        .map(|r| format!("{} {} {}", r.action, r.kind, r.name))
        .collect();
    Ok(ResourceApplyResult {
        message: results.join("\n"),
        request_id,
    })
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
            force_references: false,
            audit: None,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}
