use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::handoff::{
    HandoffSnapshot, ResumeBoundary as CoreResumeBoundary, ResumeMode, ResumePlan as CoreResumePlan,
};
use orchestrator_proto::*;
use serde_json::json;
use std::collections::HashMap;
use tonic::{Request, Response, Status};

use super::action_audit::{self, ActionDescriptor};
use super::{OrchestratorServer, trusted_actor};

fn status(error: anyhow::Error) -> Status {
    let message = error.to_string();
    if message.contains("not found") {
        Status::not_found(message)
    } else if message.contains("denied") || message.contains("disabled") {
        Status::permission_denied(message)
    } else if message.contains("stale")
        || message.contains("expired")
        || message.contains("not executable")
    {
        Status::failed_precondition(message)
    } else {
        Status::invalid_argument(message)
    }
}

fn snapshot_to_proto(snapshot: HandoffSnapshot) -> HandoffSnapshotResponse {
    HandoffSnapshotResponse {
        id: snapshot.id,
        task_id: snapshot.task_id,
        source_event_cursor: snapshot.source_event_cursor,
        projection_version: snapshot.projection_version,
        briefing_json: serde_json::to_string(&snapshot.briefing).unwrap_or_else(|_| "{}".into()),
        content_hash: snapshot.content_hash,
        state_version: snapshot.state_version,
        generated_by: snapshot.generated_by,
        created_at: snapshot.created_at,
    }
}

fn side_effect_label(value: agent_orchestrator::config::SideEffectClass) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "non_idempotent_external".to_string())
}

fn boundary_to_proto(boundary: CoreResumeBoundary) -> ResumeBoundary {
    ResumeBoundary {
        id: boundary.id,
        task_id: boundary.task_id,
        cycle: boundary.cycle,
        step_id: boundary.step_id,
        task_item_id: boundary.task_item_id,
        command_run_id: boundary.command_run_id,
        provider_session_available: boundary.provider_session_available,
        checkpoint_id: boundary.checkpoint_id,
        side_effect_class: side_effect_label(boundary.side_effect_class),
        replay_safe: boundary.replay_safe,
        reason: boundary.reason,
        state_version: boundary.state_version,
    }
}

fn plan_to_proto(plan: CoreResumePlan) -> ResumePlanResponse {
    ResumePlanResponse {
        id: plan.id,
        task_id: plan.task_id,
        boundary: Some(boundary_to_proto(plan.boundary)),
        mode: plan.mode.label().to_string(),
        expected_state_version: plan.expected_state_version,
        consequence_json: plan.consequence.to_string(),
        elevated_confirmation_required: plan.elevated_confirmation_required,
        expires_at: plan.expires_at,
        status: plan.status,
    }
}

fn task_project(server: &OrchestratorServer, task_id: &str) -> Result<String, Status> {
    let conn = agent_orchestrator::db::open_conn(&server.state.db_path)
        .map_err(|error| Status::internal(error.to_string()))?;
    conn.query_row(
        "SELECT project_id FROM tasks WHERE id=?1",
        [task_id],
        |row| row.get(0),
    )
    .map_err(|_| Status::not_found("task not found"))
}

fn runtime_policy(
    server: &OrchestratorServer,
    task_id: &str,
) -> Result<agent_orchestrator::crd::projection::RuntimePolicyProjection, Status> {
    let project = task_project(server, task_id)?;
    let config = agent_orchestrator::config_load::read_loaded_config(&server.state)
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(config.config.runtime_policy_for_project(&project))
}

pub(crate) async fn handoff_generate(
    server: &OrchestratorServer,
    mut request: Request<HandoffGenerateRequest>,
) -> Result<Response<HandoffSnapshotResponse>, Status> {
    let project = task_project(server, &request.get_ref().task_id)?;
    let context = request.get_ref().audit.clone();
    let task_id = request.get_ref().task_id.clone();
    let cursor = request.get_ref().source_event_cursor;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "HandoffGenerate",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "task",
            target_id: &task_id,
            action: "handoff.generate",
            expected_version: None,
            fencing_token: None,
            canonical_request: json!({"source_event_cursor":cursor}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching handoff generation already audited",
        )));
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    if !runtime_policy(server, &req.task_id)?.handoff_enabled {
        return Err(Status::permission_denied("handoff generation is disabled"));
    }
    let snapshot = match server
        .state
        .handoff_repo
        .generate_snapshot(&req.task_id, req.source_event_cursor, &actor)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => return Err(attempt.failed(server, status(error)).await),
    };
    attempt
        .succeeded(server, Some("handoff_snapshot"), Some(&snapshot.id))
        .await?;
    Ok(attempt.response(snapshot_to_proto(snapshot)))
}

pub(crate) async fn handoff_get(
    server: &OrchestratorServer,
    request: Request<HandoffGetRequest>,
) -> Result<Response<HandoffSnapshotResponse>, Status> {
    super::authorize(server, &request, "HandoffGet").map_err(Status::from)?;
    let snapshot = server
        .state
        .handoff_repo
        .get_snapshot(&request.into_inner().id)
        .await
        .map_err(status)?
        .ok_or_else(|| Status::not_found("handoff snapshot not found"))?;
    Ok(Response::new(snapshot_to_proto(snapshot)))
}

pub(crate) async fn resume_boundary_list(
    server: &OrchestratorServer,
    request: Request<ResumeBoundaryListRequest>,
) -> Result<Response<ResumeBoundaryListResponse>, Status> {
    super::authorize(server, &request, "ResumeBoundaryList").map_err(Status::from)?;
    let boundaries = server
        .state
        .handoff_repo
        .list_boundaries(&request.into_inner().task_id)
        .await
        .map_err(status)?
        .into_iter()
        .map(boundary_to_proto)
        .collect();
    Ok(Response::new(ResumeBoundaryListResponse { boundaries }))
}

pub(crate) async fn resume_plan(
    server: &OrchestratorServer,
    mut request: Request<ResumePlanRequest>,
) -> Result<Response<ResumePlanResponse>, Status> {
    let project = task_project(server, &request.get_ref().task_id)?;
    let context = request.get_ref().audit.clone();
    let task_id = request.get_ref().task_id.clone();
    let boundary_id = request.get_ref().boundary_id.clone();
    let mode_name = request.get_ref().mode.clone();
    let attention_item_id = request.get_ref().attention_item_id.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "ResumePlan",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "task",
            target_id: &task_id,
            action: "resume.plan",
            expected_version: None,
            fencing_token: None,
            canonical_request: json!({"boundary_id":boundary_id,"mode":mode_name,"attention_item_id":attention_item_id}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching resume plan already audited",
        )));
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    if !runtime_policy(server, &req.task_id)?.mutating_resume_enabled {
        return Err(Status::permission_denied("mutating resume is disabled"));
    }
    let mode = ResumeMode::parse(&req.mode).map_err(status)?;
    let plan = match server
        .state
        .handoff_repo
        .create_plan(
            &req.task_id,
            &req.boundary_id,
            mode,
            &actor,
            req.attention_item_id.as_deref(),
        )
        .await
    {
        Ok(plan) => plan,
        Err(error) => return Err(attempt.failed(server, status(error)).await),
    };
    attempt
        .succeeded(server, Some("resume_plan"), Some(&plan.id))
        .await?;
    Ok(attempt.response(plan_to_proto(plan)))
}

pub(crate) async fn resume_execute(
    server: &OrchestratorServer,
    mut request: Request<ResumeExecuteRequest>,
) -> Result<Response<ResumeExecuteResponse>, Status> {
    let plan = server
        .state
        .handoff_repo
        .get_plan(&request.get_ref().plan_id)
        .await
        .map_err(status)?
        .ok_or_else(|| Status::not_found("resume plan not found"))?;
    let project = task_project(server, &plan.task_id)?;
    let context = request.get_ref().audit.clone();
    let plan_id = request.get_ref().plan_id.clone();
    let expected = request.get_ref().expected_state_version.clone();
    let operator_reason = request.get_ref().operator_reason.clone();
    let key = request.get_ref().idempotency_key.clone();
    let elevated = request.get_ref().elevated_confirmation;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "ResumeExecute",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project,
            target_type: "resume_plan",
            target_id: &plan_id,
            action: "resume.execute",
            expected_version: Some(expected.clone()),
            fencing_token: None,
            canonical_request: json!({"expected_state_version":expected,"operator_reason":operator_reason,"elevated_confirmation":elevated}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: Some(&operator_reason),
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching resume execution already audited",
        )));
    }
    if let Some(status) = server.reject_new_work_during_shutdown("ResumeExecute") {
        return Err(status);
    }
    let actor = trusted_actor(&request);
    let req = request.into_inner();
    let policy = runtime_policy(server, &plan.task_id)?;
    if !policy.mutating_resume_enabled {
        return Err(Status::permission_denied("mutating resume is disabled"));
    }
    let reservation = match server
        .state
        .handoff_repo
        .reserve_execution(
            &req.plan_id,
            agent_orchestrator::handoff::ResumeExecutionRequest {
                expected_state_version: req.expected_state_version.clone(),
                idempotency_key: req.idempotency_key.clone(),
                actor: actor.clone(),
                operator_reason: req.operator_reason.clone(),
                elevated_confirmation: req.elevated_confirmation,
                elevated_policy_enabled: policy.elevated_resume_enabled,
            },
        )
        .await
    {
        Ok(reservation) => reservation,
        Err(error) => return Err(attempt.failed(server, status(error)).await),
    };
    link_resume_execution(server, &reservation.id, &attempt.request_id).await?;
    if !reservation.should_execute {
        attempt
            .succeeded(server, Some("resume_execution"), Some(&reservation.id))
            .await?;
        return Ok(attempt.response(ResumeExecuteResponse {
            execution_id: reservation.id,
            plan_id: reservation.plan_id,
            accepted: false,
            status: reservation.status,
            child_task_id: None,
        }));
    }

    let outcome = execute_plan(server, &plan).await;
    let (child_task_id, error_code) = match &outcome {
        Ok(child) => (child.clone(), None),
        Err(error) => (None, Some(error.to_string())),
    };
    server
        .state
        .handoff_repo
        .complete_execution(
            &reservation.id,
            child_task_id.as_deref(),
            error_code.as_deref(),
        )
        .await
        .map_err(status)?;
    if let Err(error) = outcome {
        return Err(attempt.failed(server, Status::failed_precondition(format!(
            "resume execution failed: {error}; restart from the logical boundary in a new session"
        ))).await);
    }
    agent_orchestrator::events::insert_event(
        &server.state,
        &plan.task_id,
        plan.boundary.task_item_id.as_deref(),
        "resume_executed",
        json!({
            "plan_id": plan.id,
            "execution_id": reservation.id,
            "mode": plan.mode,
            "boundary_id": plan.boundary.id,
            "child_task_id": child_task_id,
            "actor": actor,
            "operator_reason": req.operator_reason,
            "request_id": attempt.request_id,
        }),
    )
    .await
    .map_err(|error| Status::internal(error.to_string()))?;

    attempt
        .succeeded(server, Some("resume_execution"), Some(&reservation.id))
        .await?;
    Ok(attempt.response(ResumeExecuteResponse {
        execution_id: reservation.id,
        plan_id: reservation.plan_id,
        accepted: true,
        status: "succeeded".to_string(),
        child_task_id,
    }))
}

async fn link_resume_execution(
    server: &OrchestratorServer,
    execution_id: &str,
    request_id: &str,
) -> Result<(), Status> {
    let execution_id = execution_id.to_string();
    let request_id = request_id.to_string();
    server
        .state
        .async_database
        .writer()
        .call(move |conn| {
            conn.execute(
                "UPDATE resume_executions SET request_id=?2 WHERE id=?1",
                rusqlite::params![execution_id, request_id],
            )?;
            Ok(())
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|error| Status::internal(error.to_string()))
}

async fn execute_plan(
    server: &OrchestratorServer,
    plan: &CoreResumePlan,
) -> anyhow::Result<Option<String>> {
    match plan.mode {
        ResumeMode::ContinueTask => {
            orchestrator_scheduler::service::task::enqueue_task(&server.state, &plan.task_id)
                .await
                .map_err(anyhow::Error::from)?;
            Ok(None)
        }
        ResumeMode::RetryItem => {
            let item_id = plan
                .boundary
                .task_item_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("retry_item boundary has no task item"))?;
            let parent =
                orchestrator_scheduler::service::task::retry_task_item(&server.state, item_id)
                    .map_err(anyhow::Error::from)?;
            orchestrator_scheduler::service::task::enqueue_task(&server.state, &parent)
                .await
                .map_err(anyhow::Error::from)?;
            Ok(None)
        }
        ResumeMode::RestartFromBoundary | ResumeMode::ResumeProviderSession => {
            create_resume_child(server, plan).await.map(Some)
        }
    }
}

async fn create_resume_child(
    server: &OrchestratorServer,
    plan: &CoreResumePlan,
) -> anyhow::Result<String> {
    let conn = agent_orchestrator::db::open_conn(&server.state.db_path)?;
    let source: (String, String, String, String, String, String, String) = conn.query_row(
        "SELECT name, goal, project_id, workspace_id, workflow_id, target_files_json,
                execution_plan_json FROM tasks WHERE id=?1",
        [&plan.task_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    )?;
    let target_files: Vec<String> = serde_json::from_str(&source.5).unwrap_or_default();
    let execution_plan: serde_json::Value = serde_json::from_str(&source.6).unwrap_or_default();
    let all_steps = execution_plan
        .get("steps")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let step_filter = plan.boundary.step_id.as_ref().map(|boundary_step| {
        all_steps
            .iter()
            .skip_while(|step| {
                step.get("id").and_then(serde_json::Value::as_str) != Some(boundary_step)
            })
            .filter_map(|step| {
                step.get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
    });
    let mut initial_vars = HashMap::new();
    initial_vars.insert("resume_plan_id".to_string(), plan.id.clone());
    initial_vars.insert("resume_boundary_id".to_string(), plan.boundary.id.clone());
    if plan.mode == ResumeMode::ResumeProviderSession {
        let run_id = plan
            .boundary
            .command_run_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("provider command run is unavailable"))?;
        initial_vars.insert(
            "provider_session_ref".to_string(),
            format!("command-run:{run_id}"),
        );
    }
    let child = orchestrator_scheduler::service::task::create_task(
        &server.state,
        agent_orchestrator::dto::CreateTaskPayload {
            name: Some(format!("{} (resume)", source.0)),
            goal: Some(source.1),
            project_id: Some(source.2),
            workspace_id: Some(source.3),
            workflow_id: Some(source.4),
            target_files: Some(target_files),
            parent_task_id: Some(plan.task_id.clone()),
            spawn_reason: Some(format!("resume_boundary:{}", plan.boundary.id)),
            step_filter,
            initial_vars: Some(initial_vars),
        },
    )
    .map_err(anyhow::Error::from)?;
    if plan.mode == ResumeMode::ResumeProviderSession {
        let run_id = plan
            .boundary
            .command_run_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("provider command run is unavailable"))?;
        conn.execute(
            "UPDATE tasks SET resume_token=?1 WHERE id=?2",
            [format!("command-run:{run_id}"), child.id.clone()],
        )?;
    }
    orchestrator_scheduler::service::task::enqueue_task(&server.state, &child.id)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(child.id)
}
