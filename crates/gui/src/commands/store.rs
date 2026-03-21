use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct StoreEntry {
    pub key: String,
    pub value_json: String,
    pub updated_at: String,
}

/// List store entries (read_only+).
#[tauri::command]
pub async fn store_list(
    state: State<'_, Arc<AppState>>,
    store: String,
    project: Option<String>,
) -> Result<Vec<StoreEntry>, String> {
    let mut client = state.client().await?;
    let resp = client
        .store_list(orchestrator_proto::StoreListRequest {
            store,
            project: project.unwrap_or_default(),
            limit: 100,
            offset: 0,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let entries = resp
        .into_inner()
        .entries
        .into_iter()
        .map(|e| StoreEntry {
            key: e.key,
            value_json: e.value_json,
            updated_at: e.updated_at,
        })
        .collect();
    Ok(entries)
}

/// Get a store value (read_only+).
#[tauri::command]
pub async fn store_get(
    state: State<'_, Arc<AppState>>,
    store: String,
    key: String,
    project: Option<String>,
) -> Result<Option<String>, String> {
    let mut client = state.client().await?;
    let resp = client
        .store_get(orchestrator_proto::StoreGetRequest {
            store,
            key,
            project: project.unwrap_or_default(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    if inner.found {
        Ok(inner.value_json)
    } else {
        Ok(None)
    }
}

/// Put a store value (operator+).
#[tauri::command]
pub async fn store_put(
    state: State<'_, Arc<AppState>>,
    store: String,
    key: String,
    value_json: String,
    project: Option<String>,
    task_id: Option<String>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .store_put(orchestrator_proto::StorePutRequest {
            store,
            key,
            value_json,
            project: project.unwrap_or_default(),
            task_id: task_id.unwrap_or_default(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}

/// Delete a store value (operator+).
#[tauri::command]
pub async fn store_delete(
    state: State<'_, Arc<AppState>>,
    store: String,
    key: String,
    project: Option<String>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .store_delete(orchestrator_proto::StoreDeleteRequest {
            store,
            key,
            project: project.unwrap_or_default(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}
