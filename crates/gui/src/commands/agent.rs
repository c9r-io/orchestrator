use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub enabled: bool,
    pub lifecycle_state: String,
    pub in_flight_items: i32,
    pub capabilities: Vec<String>,
    pub is_healthy: bool,
}

/// List all registered agents (read_only+).
#[tauri::command]
pub async fn agent_list(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    let mut client = state.client().await?;
    let resp = client
        .agent_list(orchestrator_proto::AgentListRequest {
            project_id: None,
        })
        .await
        .map_err(|e| e.message().to_string())?;

    let agents = resp
        .into_inner()
        .agents
        .into_iter()
        .map(|a| AgentInfo {
            name: a.name,
            enabled: a.enabled,
            lifecycle_state: a.lifecycle_state,
            in_flight_items: a.in_flight_items,
            capabilities: a.capabilities,
            is_healthy: a.is_healthy,
        })
        .collect();
    Ok(agents)
}

/// Cordon an agent (admin).
#[tauri::command]
pub async fn agent_cordon(
    state: State<'_, AppState>,
    agent_name: String,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .agent_cordon(orchestrator_proto::AgentCordonRequest {
            agent_name: agent_name.clone(),
            project_id: None,
        })
        .await
        .map_err(|e| e.message().to_string())?;
    Ok(resp.into_inner().message)
}

/// Uncordon an agent (admin).
#[tauri::command]
pub async fn agent_uncordon(
    state: State<'_, AppState>,
    agent_name: String,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .agent_uncordon(orchestrator_proto::AgentUncordonRequest {
            agent_name: agent_name.clone(),
            project_id: None,
        })
        .await
        .map_err(|e| e.message().to_string())?;
    Ok(resp.into_inner().message)
}

/// Drain an agent (admin).
#[tauri::command]
pub async fn agent_drain(
    state: State<'_, AppState>,
    agent_name: String,
) -> Result<String, String> {
    let mut client = state.client().await?;
    let resp = client
        .agent_drain(orchestrator_proto::AgentDrainRequest {
            agent_name: agent_name.clone(),
            project_id: None,
            timeout_secs: None,
        })
        .await
        .map_err(|e| e.message().to_string())?;
    Ok(resp.into_inner().message)
}
