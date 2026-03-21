use serde::Serialize;
use tauri::State;

use crate::client::TransportKind;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct PingInfo {
    pub version: String,
    pub git_hash: String,
    pub uptime_secs: String,
}

/// Connect to the daemon (auto-discover or explicit config).
#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    config_path: Option<String>,
) -> Result<(), String> {
    state.connect(config_path.as_deref()).await
}

/// Ping the daemon and return version info.
#[tauri::command]
pub async fn ping(state: State<'_, AppState>) -> Result<PingInfo, String> {
    let mut client = state.client().await?;
    let resp = client
        .ping(orchestrator_proto::PingRequest {})
        .await
        .map_err(|e| e.message().to_string())?;
    let inner = resp.into_inner();
    Ok(PingInfo {
        version: inner.version,
        git_hash: inner.git_hash,
        uptime_secs: inner.uptime_secs,
    })
}

/// Probe the current user's RBAC role without modifying proto or daemon.
///
/// Strategy:
/// - UDS connections skip RBAC → default to "admin".
/// - TLS connections: attempt admin-only RPC (`ConfigDebug`). If
///   `PermissionDenied`, try operator-only RPC (`TaskCreate` with empty body —
///   will return `InvalidArgument` for operators). If also `PermissionDenied`,
///   role is "read_only".
#[tauri::command]
pub async fn probe_role(state: State<'_, AppState>) -> Result<String, String> {
    // Return cached role if available.
    if let Some(role) = state.get_role().await {
        return Ok(role);
    }

    // UDS → no RBAC enforcement, treat as admin.
    if state.transport_kind().await == Some(TransportKind::Uds) {
        state.set_role("admin".into()).await;
        return Ok("admin".into());
    }

    let mut client = state.client().await?;

    // Try admin-only RPC.
    let admin_result = client
        .config_debug(orchestrator_proto::ConfigDebugRequest {
            component: None,
        })
        .await;

    let role: String = match admin_result {
        Ok(_) => "admin".into(),
        Err(status) if status.code() == tonic::Code::PermissionDenied => {
            // Try operator-only RPC with deliberately empty/invalid payload.
            let operator_result = client
                .task_create(orchestrator_proto::TaskCreateRequest {
                    name: None,
                    goal: None,
                    project_id: None,
                    workspace_id: None,
                    workflow_id: None,
                    target_files: vec![],
                    no_start: true,
                })
                .await;
            match operator_result {
                Err(s) if s.code() == tonic::Code::PermissionDenied => "read_only".into(),
                _ => "operator".into(),
            }
        }
        Err(e) => return Err(format!("role probe failed: {}", e.message())),
    };

    state.set_role(role.clone()).await;
    Ok(role)
}
