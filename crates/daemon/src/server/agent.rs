use agent_orchestrator::agent_lifecycle;
use agent_orchestrator::config_load::read_active_config;
use agent_orchestrator::selection::resolve_effective_agents;
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{authorize, OrchestratorServer};

pub(crate) async fn agent_list(
    server: &OrchestratorServer,
    request: Request<AgentListRequest>,
) -> Result<Response<AgentListResponse>, Status> {
    authorize(server, &request, "AgentList").map_err(Status::from)?;
    let req = request.into_inner();

    let active = read_active_config(&server.state).map_err(|e| Status::internal(e.to_string()))?;
    let project_id = req.project_id.as_deref().unwrap_or("");
    let agents = resolve_effective_agents(project_id, &active.config, None);
    let lifecycle_map = server.state.agent_lifecycle.read().await;

    let mut statuses: Vec<AgentStatus> = agents
        .iter()
        .map(|(id, cfg)| {
            let runtime = lifecycle_map
                .get(id)
                .cloned()
                .unwrap_or_default();
            AgentStatus {
                name: id.clone(),
                enabled: cfg.enabled,
                lifecycle_state: runtime.lifecycle.as_str().to_string(),
                in_flight_items: runtime.in_flight_items as i32,
                capabilities: cfg.capabilities.clone(),
                drain_requested_at: runtime
                    .drain_requested_at
                    .map(|dt| dt.to_rfc3339()),
            }
        })
        .collect();
    statuses.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Response::new(AgentListResponse { agents: statuses }))
}

pub(crate) async fn agent_cordon(
    server: &OrchestratorServer,
    request: Request<AgentCordonRequest>,
) -> Result<Response<AgentCordonResponse>, Status> {
    authorize(server, &request, "AgentCordon").map_err(Status::from)?;
    let req = request.into_inner();

    // Validate agent exists
    validate_agent_exists(server, &req.agent_name, req.project_id.as_deref())?;

    // Check last-active warning
    emit_last_active_warning(server, &req.agent_name, req.project_id.as_deref()).await;

    agent_lifecycle::cordon_agent(&server.state, &req.agent_name)
        .await
        .map_err(|e| Status::failed_precondition(e))?;

    Ok(Response::new(AgentCordonResponse {
        message: format!("agent '{}' cordoned", req.agent_name),
    }))
}

pub(crate) async fn agent_uncordon(
    server: &OrchestratorServer,
    request: Request<AgentUncordonRequest>,
) -> Result<Response<AgentUncordonResponse>, Status> {
    authorize(server, &request, "AgentUncordon").map_err(Status::from)?;
    let req = request.into_inner();

    validate_agent_exists(server, &req.agent_name, req.project_id.as_deref())?;

    agent_lifecycle::uncordon_agent(&server.state, &req.agent_name)
        .await
        .map_err(|e| Status::failed_precondition(e))?;

    Ok(Response::new(AgentUncordonResponse {
        message: format!("agent '{}' uncordoned", req.agent_name),
    }))
}

pub(crate) async fn agent_drain(
    server: &OrchestratorServer,
    request: Request<AgentDrainRequest>,
) -> Result<Response<AgentDrainResponse>, Status> {
    authorize(server, &request, "AgentDrain").map_err(Status::from)?;
    let req = request.into_inner();

    validate_agent_exists(server, &req.agent_name, req.project_id.as_deref())?;

    // Check last-active warning
    emit_last_active_warning(server, &req.agent_name, req.project_id.as_deref()).await;

    let result_state =
        agent_lifecycle::drain_agent(&server.state, &req.agent_name, req.timeout_secs)
            .await
            .map_err(|e| Status::failed_precondition(e))?;

    Ok(Response::new(AgentDrainResponse {
        message: format!(
            "agent '{}' drain initiated — state: {}",
            req.agent_name,
            result_state.as_str()
        ),
        lifecycle_state: result_state.as_str().to_string(),
    }))
}

fn validate_agent_exists(
    server: &OrchestratorServer,
    agent_name: &str,
    project_id: Option<&str>,
) -> Result<(), Status> {
    let active = read_active_config(&server.state).map_err(|e| Status::internal(e.to_string()))?;
    let pid = project_id.unwrap_or("");
    let agents = resolve_effective_agents(pid, &active.config, None);
    if !agents.contains_key(agent_name) {
        return Err(Status::not_found(format!(
            "agent '{}' not found in project '{}'",
            agent_name,
            if pid.is_empty() { "default" } else { pid }
        )));
    }
    Ok(())
}

async fn emit_last_active_warning(
    server: &OrchestratorServer,
    agent_name: &str,
    project_id: Option<&str>,
) {
    if let Ok(active) = read_active_config(&server.state) {
        let pid = project_id.unwrap_or("");
        let agents = resolve_effective_agents(pid, &active.config, None);
        let lifecycle_map = server.state.agent_lifecycle.read().await;
        let orphaned =
            agent_lifecycle::warn_if_last_active_agent(agent_name, agents, &lifecycle_map);
        if !orphaned.is_empty() {
            server.state.emit_event(
                "",
                None,
                "agent_last_active_warning",
                serde_json::json!({
                    "agent_id": agent_name,
                    "orphaned_capabilities": orphaned,
                    "warning": "cordoning/draining this agent leaves no active agents for these capabilities",
                }),
            );
        }
    }
}
