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

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub content: String,
    pub format: String,
    pub exit_code: i32,
}

/// Run preflight checks (read_only+).
#[tauri::command]
pub async fn check(
    state: State<'_, AppState>,
    workflow: Option<String>,
    project_id: Option<String>,
) -> Result<CheckResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .check(orchestrator_proto::CheckRequest {
            workflow,
            output_format: "json".into(),
            project_id,
        })
        .await
        .map_err(|e| e.message().to_string())?;
    let inner = resp.into_inner();
    Ok(CheckResult {
        content: inner.content,
        format: inner.format,
        exit_code: inner.exit_code,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerStatusResult {
    pub pending_tasks: i64,
    pub active_workers: i64,
    pub idle_workers: i64,
    pub running_tasks: i64,
    pub configured_workers: i64,
    pub lifecycle_state: String,
    pub shutdown_requested: bool,
}

/// Get worker thread status (read_only+).
#[tauri::command]
pub async fn worker_status(state: State<'_, AppState>) -> Result<WorkerStatusResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .worker_status(orchestrator_proto::WorkerStatusRequest {})
        .await
        .map_err(|e| e.message().to_string())?;
    let inner = resp.into_inner();
    Ok(WorkerStatusResult {
        pending_tasks: inner.pending_tasks,
        active_workers: inner.active_workers,
        idle_workers: inner.idle_workers,
        running_tasks: inner.running_tasks,
        configured_workers: inner.configured_workers,
        lifecycle_state: inner.lifecycle_state,
        shutdown_requested: inner.shutdown_requested,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct DbStatusResult {
    pub db_path: String,
    pub current_version: u32,
    pub target_version: u32,
    pub is_current: bool,
    pub pending_names: Vec<String>,
}

/// Get database status (read_only+).
#[tauri::command]
pub async fn db_status(state: State<'_, AppState>) -> Result<DbStatusResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .db_status(orchestrator_proto::DbStatusRequest {})
        .await
        .map_err(|e| e.message().to_string())?;
    let inner = resp.into_inner();
    Ok(DbStatusResult {
        db_path: inner.db_path,
        current_version: inner.current_version,
        target_version: inner.target_version,
        is_current: inner.is_current,
        pending_names: inner.pending_names,
    })
}

/// Gracefully shutdown the daemon (admin).
#[tauri::command]
pub async fn shutdown(
    state: State<'_, AppState>,
    graceful: Option<bool>,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .shutdown(orchestrator_proto::ShutdownRequest {
            graceful: graceful.unwrap_or(true),
        })
        .await
        .map_err(|e| e.message().to_string())?;
    Ok(resp.into_inner().message)
}

#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceModeResult {
    pub maintenance_mode: bool,
    pub message: String,
}

/// Toggle maintenance mode (admin).
#[tauri::command]
pub async fn maintenance_mode(
    state: State<'_, AppState>,
    enable: bool,
) -> Result<MaintenanceModeResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .maintenance_mode(orchestrator_proto::MaintenanceModeRequest { enable })
        .await
        .map_err(|e| e.message().to_string())?;
    let inner = resp.into_inner();
    Ok(MaintenanceModeResult {
        maintenance_mode: inner.maintenance_mode,
        message: inner.message,
    })
}
