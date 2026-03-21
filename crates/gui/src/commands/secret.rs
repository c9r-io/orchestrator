use serde::Serialize;
use tauri::State;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct SecretKeyInfo {
    pub key_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretKeyStatusResult {
    pub active_key: Option<SecretKeyInfo>,
    pub all_keys: Vec<SecretKeyInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RotateResult {
    pub message: String,
    pub resources_updated: u64,
    pub versions_updated: u64,
    pub errors: Vec<String>,
}

fn map_key(k: orchestrator_proto::SecretKeyRecord) -> SecretKeyInfo {
    SecretKeyInfo {
        key_id: k.key_id,
        status: k.state,
        created_at: k.created_at,
    }
}

/// List all secret keys (admin).
#[tauri::command]
pub async fn secret_key_list(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<SecretKeyInfo>, String> {
    let mut client = state.client().await?;
    let resp = client
        .secret_key_list(orchestrator_proto::SecretKeyListRequest {})
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().keys.into_iter().map(map_key).collect())
}

/// Get secret key status (admin).
#[tauri::command]
pub async fn secret_key_status(
    state: State<'_, Arc<AppState>>,
) -> Result<SecretKeyStatusResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .secret_key_status(orchestrator_proto::SecretKeyStatusRequest {})
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(SecretKeyStatusResult {
        active_key: inner.active_key.map(map_key),
        all_keys: inner.all_keys.into_iter().map(map_key).collect(),
    })
}

/// Rotate secret keys (admin).
#[tauri::command]
pub async fn secret_key_rotate(
    state: State<'_, Arc<AppState>>,
    resume: Option<bool>,
) -> Result<RotateResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .secret_key_rotate(orchestrator_proto::SecretKeyRotateRequest {
            resume: resume.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(RotateResult {
        message: inner.message,
        resources_updated: inner.resources_updated,
        versions_updated: inner.versions_updated,
        errors: inner.errors,
    })
}

/// Revoke a secret key (admin).
#[tauri::command]
pub async fn secret_key_revoke(
    state: State<'_, Arc<AppState>>,
    key_id: String,
    force: Option<bool>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .secret_key_revoke(orchestrator_proto::SecretKeyRevokeRequest {
            key_id,
            force: force.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(resp.into_inner().message)
}
