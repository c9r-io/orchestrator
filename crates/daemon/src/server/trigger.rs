use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::action_audit::{self, ActionDescriptor};
use super::{OrchestratorServer, authorize, map_core_error};

pub(crate) async fn trigger_suspend(
    server: &OrchestratorServer,
    mut request: Request<TriggerSuspendRequest>,
) -> Result<Response<TriggerSuspendResponse>, Status> {
    mutate_trigger_suspend(server, &mut request, true).await?;
    let req = request.into_inner();

    Ok(Response::new(TriggerSuspendResponse {
        message: format!("trigger '{}' suspended", req.trigger_name),
    }))
}

async fn mutate_trigger_suspend<T>(
    server: &OrchestratorServer,
    request: &mut Request<T>,
    _suspend: bool,
) -> Result<(), Status>
where
    T: TriggerMutationRequest,
{
    let project_id = request
        .get_ref()
        .project()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID)
        .to_string();
    let trigger_name = request.get_ref().trigger_name().to_string();
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|error| Status::internal(error.to_string()))?;
    let trigger = active
        .config
        .projects
        .get(&project_id)
        .and_then(|project| project.triggers.get(&trigger_name))
        .ok_or_else(|| Status::not_found("trigger not found"))?;
    let installation = trigger
        .event
        .as_ref()
        .and_then(|event| event.webhook.as_ref())
        .and_then(|webhook| webhook.installation_id.clone());
    let context = request.get_ref().audit().cloned();
    let suspend = request.get_ref().suspend();
    let rpc = if suspend {
        "TriggerSuspend"
    } else {
        "TriggerResume"
    };
    let action = if suspend {
        "trigger.suspend"
    } else {
        "trigger.resume"
    };
    let attempt = action_audit::begin(
        server,
        request,
        rpc,
        context.as_ref(),
        ActionDescriptor {
            project_id: &project_id,
            target_type: "trigger",
            target_id: &trigger_name,
            action,
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({
                "project_id":project_id,"trigger_name":trigger_name,"suspend":suspend
            }),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Ok(());
    }

    let changed = if suspend {
        agent_orchestrator::service::resource::suspend_trigger(
            &server.state,
            &trigger_name,
            Some(&project_id),
        )
    } else {
        agent_orchestrator::service::resource::resume_trigger(
            &server.state,
            &trigger_name,
            Some(&project_id),
        )
    };
    if let Err(error) = changed {
        return Err(attempt.failed(server, map_core_error(error)).await);
    }
    if let Some(installation) = installation {
        let repository =
            agent_orchestrator::source_automation::AsyncSourceAutomationRepository::new(
                server.state.async_database.clone(),
            );
        let scope = format!("installation:{installation}");
        let projection = if suspend {
            repository
                .suspend_scope(&project_id, Some(&installation), None, &scope)
                .await
        } else {
            repository
                .resume_scope(&project_id, Some(&installation), None, &scope)
                .await
        };
        if let Err(error) = projection {
            return Err(attempt
                .failed(server, Status::internal(error.to_string()))
                .await);
        }
    }
    attempt
        .succeeded(server, Some("trigger"), Some(&trigger_name))
        .await?;
    Ok(())
}

trait TriggerMutationRequest {
    fn trigger_name(&self) -> &str;
    fn project(&self) -> Option<&str>;
    fn audit(&self) -> Option<&ActionAuditContext>;
    fn suspend(&self) -> bool;
}

impl TriggerMutationRequest for TriggerSuspendRequest {
    fn trigger_name(&self) -> &str {
        &self.trigger_name
    }
    fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }
    fn audit(&self) -> Option<&ActionAuditContext> {
        self.audit.as_ref()
    }
    fn suspend(&self) -> bool {
        true
    }
}

impl TriggerMutationRequest for TriggerResumeRequest {
    fn trigger_name(&self) -> &str {
        &self.trigger_name
    }
    fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }
    fn audit(&self) -> Option<&ActionAuditContext> {
        self.audit.as_ref()
    }
    fn suspend(&self) -> bool {
        false
    }
}

pub(crate) async fn trigger_resume(
    server: &OrchestratorServer,
    mut request: Request<TriggerResumeRequest>,
) -> Result<Response<TriggerResumeResponse>, Status> {
    mutate_trigger_suspend(server, &mut request, false).await?;
    let req = request.into_inner();

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
        .ok_or_else(|| Status::not_found(format!("project not found: {project}")))?;
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
