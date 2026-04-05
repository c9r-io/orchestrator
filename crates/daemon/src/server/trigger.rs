use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, authorize, map_core_error};

pub(crate) async fn trigger_suspend(
    server: &OrchestratorServer,
    request: Request<TriggerSuspendRequest>,
) -> Result<Response<TriggerSuspendResponse>, Status> {
    authorize(server, &request, "TriggerSuspend").map_err(Status::from)?;
    let req = request.into_inner();

    agent_orchestrator::service::resource::suspend_trigger(
        &server.state,
        &req.trigger_name,
        req.project.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(TriggerSuspendResponse {
        message: format!("trigger '{}' suspended", req.trigger_name),
    }))
}

pub(crate) async fn trigger_resume(
    server: &OrchestratorServer,
    request: Request<TriggerResumeRequest>,
) -> Result<Response<TriggerResumeResponse>, Status> {
    authorize(server, &request, "TriggerResume").map_err(Status::from)?;
    let req = request.into_inner();

    agent_orchestrator::service::resource::resume_trigger(
        &server.state,
        &req.trigger_name,
        req.project.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(TriggerResumeResponse {
        message: format!("trigger '{}' resumed", req.trigger_name),
    }))
}

pub(crate) async fn trigger_fire(
    server: &OrchestratorServer,
    request: Request<TriggerFireRequest>,
) -> Result<Response<TriggerFireResponse>, Status> {
    authorize(server, &request, "TriggerFire").map_err(Status::from)?;
    if let Some(status) = server.reject_new_work_during_shutdown("TriggerFire") {
        return Err(status);
    }
    let req = request.into_inner();

    // ── Resolve project and trigger config ──────────────────────────────
    let project = req
        .project
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID)
        .to_string();

    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|e| Status::internal(e.to_string()))?;
    let proj_cfg = active
        .config
        .projects
        .get(&project)
        .ok_or_else(|| Status::not_found(format!("project not found: {}", project)))?;
    let trigger_cfg = proj_cfg.triggers.get(&req.trigger_name).ok_or_else(|| {
        Status::not_found(format!(
            "trigger '{}' not found in project '{}'",
            req.trigger_name, project
        ))
    })?;

    // ── Parse optional webhook payload ──���───────────────────────────────
    let webhook_payload = req
        .payload_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    // ── Canonical trigger fire (full engine semantics) ──────────────────
    let task_id = agent_orchestrator::trigger_engine::fire_trigger_canonical(
        &server.state,
        &req.trigger_name,
        &project,
        trigger_cfg,
        webhook_payload.as_ref(),
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))?;

    // ── Broadcast for other event-driven triggers (with correct project) ─
    if let Some(payload) = webhook_payload {
        agent_orchestrator::trigger_engine::broadcast_task_event(
            &server.state,
            agent_orchestrator::trigger_engine::TriggerEventPayload {
                event_type: "webhook".to_string(),
                task_id: String::new(),
                payload: Some(payload),
                project: Some(project.clone()),
                exclude_trigger: Some((req.trigger_name.clone(), project.clone())),
            },
        );
    }

    Ok(Response::new(TriggerFireResponse {
        task_id: task_id.clone(),
        message: format!("trigger '{}' fired — task {}", req.trigger_name, task_id),
    }))
}
