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

    // If a payload was provided, broadcast as a webhook event for trigger matching.
    if let Some(ref payload_json) = req.payload_json {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_json) {
            agent_orchestrator::trigger_engine::broadcast_task_event(
                &server.state,
                agent_orchestrator::trigger_engine::TriggerEventPayload {
                    event_type: "webhook".to_string(),
                    task_id: String::new(),
                    payload: Some(payload),
                    project: None,
                },
            );
        }
    }

    let task_id = agent_orchestrator::service::resource::fire_trigger(
        &server.state,
        &req.trigger_name,
        req.project.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(TriggerFireResponse {
        task_id: task_id.clone(),
        message: format!("trigger '{}' fired — task {}", req.trigger_name, task_id),
    }))
}
