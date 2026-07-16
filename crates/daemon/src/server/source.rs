use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::source::{
    AsyncSourceRepository, CreateSourceBinding, IngestSourceEvent, NormalizedSourceEvent,
};
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

fn event_to_proto(value: agent_orchestrator::source::SourceEventRecord) -> SourceEvent {
    SourceEvent {
        id: value.id,
        project_id: value.project_id,
        provider: value.provider,
        installation_id: value.installation_id,
        external_event_id: value.external_event_id,
        event_type: value.event_type,
        external_actor_id: value.external_actor_id,
        conversation_id: value.conversation_id,
        thread_id: value.thread_id,
        occurred_at: value.occurred_at,
        received_at: value.received_at,
        normalized_json: serde_json::to_string(&value.normalized).unwrap_or_default(),
        payload_hash: value.payload_hash,
        routing_state: value.routing_state,
        routing_attempts: value.routing_attempts,
        routed_task_id: value.routed_task_id,
        last_error_code: value.last_error_code,
    }
}

fn binding_to_proto(value: agent_orchestrator::source::SourceBinding) -> SourceBinding {
    SourceBinding {
        id: value.id,
        project_id: value.project_id,
        task_id: value.task_id,
        provider: value.provider,
        installation_id: value.installation_id,
        conversation_id: value.conversation_id,
        thread_id: value.thread_id,
        binding_type: value.binding_type,
        created_by_event_id: value.created_by_event_id,
        created_at: value.created_at,
    }
}

pub(crate) async fn event_list(
    server: &OrchestratorServer,
    request: Request<SourceEventListRequest>,
) -> Result<Response<SourceEventListResponse>, Status> {
    super::authorize(server, &request, "SourceEventList").map_err(Status::from)?;
    let req = request.into_inner();
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let events = repository
        .list(
            req.project_id.as_deref(),
            req.task_id.as_deref(),
            req.routing_state.as_deref(),
            if req.limit == 0 {
                100
            } else {
                req.limit as usize
            },
        )
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(SourceEventListResponse {
        events: events.into_iter().map(event_to_proto).collect(),
    }))
}

pub(crate) async fn event_get(
    server: &OrchestratorServer,
    request: Request<SourceEventGetRequest>,
) -> Result<Response<SourceEvent>, Status> {
    super::authorize(server, &request, "SourceEventGet").map_err(Status::from)?;
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let event = repository
        .get(&request.into_inner().id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("source event not found"))?;
    Ok(Response::new(event_to_proto(event)))
}

pub(crate) async fn task_template_preview(
    server: &OrchestratorServer,
    request: Request<SourceTaskTemplatePreviewRequest>,
) -> Result<Response<SourceTaskTemplatePreviewResponse>, Status> {
    super::authorize(server, &request, "SourceTaskTemplatePreview").map_err(Status::from)?;
    let req = request.into_inner();
    let project_id = if req.project_id.trim().is_empty() {
        agent_orchestrator::config::DEFAULT_PROJECT_ID.to_string()
    } else {
        req.project_id.clone()
    };
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let rendered =
        agent_orchestrator::source_task_template::render_source_task_template_from_config(
            &active.config,
            &project_id,
            &req.name,
            &agent_orchestrator::source_task_template::SourceTaskTemplateRenderInput {
                provider: req.provider,
                installation_id: req.installation_id,
                message_url: req.message_url,
                event_id: req.event_id,
                reaction: req.reaction,
                target_id: req.target_id,
                installation_verified: false,
            },
        )
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("not found") {
                Status::not_found(message)
            } else {
                Status::invalid_argument(message)
            }
        })?;
    let policy = active.config.runtime_policy_for_project(&project_id);
    let public = agent_orchestrator::source_task_template::redact_rendered_source_task_template(
        &rendered,
        &policy.runner.redaction_patterns,
    );
    Ok(Response::new(SourceTaskTemplatePreviewResponse {
        name: req.name,
        project_id,
        skill_name: public.skill_name,
        skill_invocation: public.skill_invocation,
        skill_args: public.skill_args,
        goal: public.goal,
        workflow: public.action.workflow,
        workspace: public.action.workspace,
        start: public.action.start,
        initial_vars: public.action.initial_vars.into_iter().collect(),
        content_hash: public.content_hash,
        revision: public.revision,
        warnings: public.warnings,
    }))
}

pub(crate) async fn event_ingest(
    server: &OrchestratorServer,
    mut request: Request<SourceEventIngestRequest>,
) -> Result<Response<SourceEventIngestResponse>, Status> {
    if request.get_ref().normalized_json.len() > 64 * 1024 {
        return Err(Status::invalid_argument(
            "normalized_json exceeds 65536 bytes",
        ));
    }
    let event: NormalizedSourceEvent = serde_json::from_str(&request.get_ref().normalized_json)
        .map_err(|error| Status::invalid_argument(format!("invalid normalized_json: {error}")))?;
    let context = request.get_ref().audit.clone();
    let project_id = request.get_ref().project_id.clone();
    let payload_hash = request.get_ref().payload_hash.clone();
    let target_id = format!(
        "{}:{}:{}",
        event.provider, event.installation_id, event.external_event_id
    );
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceEventIngest",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project_id,
            target_type: "source_delivery",
            target_id: &target_id,
            action: "source.ingest",
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({"provider":event.provider,"installation_id":event.installation_id,"external_event_id":event.external_event_id,"payload_hash":payload_hash}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching source ingest already audited",
        )));
    }
    if let Some(status) = server.reject_new_work_during_shutdown("SourceEventIngest") {
        return Err(status);
    }
    let req = request.into_inner();
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    if !active
        .config
        .runtime_policy_for_project(&req.project_id)
        .source_ingest_enabled
    {
        return Err(attempt
            .failed(
                server,
                Status::failed_precondition("source ingestion is disabled"),
            )
            .await);
    }
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let result = match repository
        .ingest(IngestSourceEvent {
            project_id: req.project_id,
            event,
            payload_hash: req.payload_hash,
            raw_payload_ref: None,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return Err(attempt
                .failed(server, Status::invalid_argument(error.to_string()))
                .await);
        }
    };
    if !result.inserted {
        super::process_metrics::record_source_dedup(
            &server.state,
            &result.event.project_id,
            &result.event.provider,
        );
    }
    link_source_row(
        server,
        "source_events",
        &result.event.id,
        &attempt.request_id,
    )
    .await?;
    attempt
        .succeeded(server, Some("source_event"), Some(&result.event.id))
        .await?;
    Ok(attempt.response(SourceEventIngestResponse {
        event: Some(event_to_proto(result.event)),
        inserted: result.inserted,
    }))
}

pub(crate) async fn binding_list(
    server: &OrchestratorServer,
    request: Request<SourceBindingListRequest>,
) -> Result<Response<SourceBindingListResponse>, Status> {
    super::authorize(server, &request, "SourceBindingList").map_err(Status::from)?;
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let bindings = repository
        .list_bindings(&request.into_inner().task_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(SourceBindingListResponse {
        bindings: bindings.into_iter().map(binding_to_proto).collect(),
    }))
}

pub(crate) async fn bind(
    server: &OrchestratorServer,
    mut request: Request<SourceBindRequest>,
) -> Result<Response<SourceBinding>, Status> {
    let context = request.get_ref().audit.clone();
    let project_id = request.get_ref().project_id.clone();
    let task_id = request.get_ref().task_id.clone();
    let target_id = format!(
        "{}:{}:{}:{}",
        request.get_ref().provider,
        request.get_ref().installation_id,
        request.get_ref().conversation_id.as_deref().unwrap_or(""),
        request.get_ref().thread_id.as_deref().unwrap_or("")
    );
    let canonical = serde_json::json!({"task_id":task_id,"provider":request.get_ref().provider,"installation_id":request.get_ref().installation_id,"conversation_id":request.get_ref().conversation_id,"thread_id":request.get_ref().thread_id,"binding_type":request.get_ref().binding_type,"created_by_event_id":request.get_ref().created_by_event_id});
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceBind",
        context.as_ref(),
        ActionDescriptor {
            project_id: &project_id,
            target_type: "source_binding",
            target_id: &target_id,
            action: "source.bind",
            expected_version: None,
            fencing_token: None,
            canonical_request: canonical,
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching source binding already audited",
        )));
    }
    let req = request.into_inner();
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let binding = match repository
        .create_binding(CreateSourceBinding {
            project_id: req.project_id,
            task_id: req.task_id,
            provider: req.provider,
            installation_id: req.installation_id,
            conversation_id: req.conversation_id,
            thread_id: req.thread_id,
            binding_type: req.binding_type,
            created_by_event_id: req.created_by_event_id,
        })
        .await
    {
        Ok(binding) => binding,
        Err(error) => {
            return Err(attempt
                .failed(server, Status::failed_precondition(error.to_string()))
                .await);
        }
    };
    link_source_row(server, "source_bindings", &binding.id, &attempt.request_id).await?;
    attempt
        .succeeded(server, Some("source_binding"), Some(&binding.id))
        .await?;
    Ok(attempt.response(binding_to_proto(binding)))
}

pub(crate) async fn replay(
    server: &OrchestratorServer,
    mut request: Request<SourceReplayRequest>,
) -> Result<Response<SourceReplayResponse>, Status> {
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let current = repository
        .get(&request.get_ref().id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("source event not found"))?;
    let context = request.get_ref().audit.clone();
    let id = request.get_ref().id.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "SourceReplay",
        context.as_ref(),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "source_event",
            target_id: &id,
            action: "source.replay",
            expected_version: Some(current.routing_attempts.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"routing_attempts":current.routing_attempts,"routing_state":current.routing_state}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching source replay already audited",
        )));
    }
    if let Err(error) = repository.replay(&id).await {
        return Err(attempt
            .failed(server, Status::failed_precondition(error.to_string()))
            .await);
    }
    link_source_row(server, "source_events", &id, &attempt.request_id).await?;
    attempt
        .succeeded(server, Some("source_event"), Some(&id))
        .await?;
    Ok(attempt.response(SourceReplayResponse {
        id,
        status: "received".to_string(),
    }))
}

async fn link_source_row(
    server: &OrchestratorServer,
    table: &str,
    id: &str,
    request_id: &str,
) -> Result<(), Status> {
    let sql = match table {
        "source_events" => "UPDATE source_events SET request_id=?2 WHERE id=?1",
        "source_bindings" => "UPDATE source_bindings SET request_id=?2 WHERE id=?1",
        _ => return Err(Status::internal("invalid source audit table")),
    };
    let id = id.to_string();
    let request_id = request_id.to_string();
    server
        .state
        .async_database
        .writer()
        .call(move |conn| {
            conn.execute(sql, rusqlite::params![id, request_id])?;
            Ok(())
        })
        .await
        .map_err(agent_orchestrator::async_database::flatten_err)
        .map_err(|error| Status::internal(error.to_string()))
}
