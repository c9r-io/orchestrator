use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::source::{
    AsyncSourceRepository, CreateSourceBinding, IngestSourceEvent, NormalizedSourceEvent,
};
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;

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

pub(crate) async fn event_ingest(
    server: &OrchestratorServer,
    request: Request<SourceEventIngestRequest>,
) -> Result<Response<SourceEventIngestResponse>, Status> {
    super::authorize(server, &request, "SourceEventIngest").map_err(Status::from)?;
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
        return Err(Status::failed_precondition("source ingestion is disabled"));
    }
    if req.normalized_json.len() > 64 * 1024 {
        return Err(Status::invalid_argument(
            "normalized_json exceeds 65536 bytes",
        ));
    }
    let event: NormalizedSourceEvent = serde_json::from_str(&req.normalized_json)
        .map_err(|error| Status::invalid_argument(format!("invalid normalized_json: {error}")))?;
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let result = repository
        .ingest(IngestSourceEvent {
            project_id: req.project_id,
            event,
            payload_hash: req.payload_hash,
            raw_payload_ref: None,
        })
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(Response::new(SourceEventIngestResponse {
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
    request: Request<SourceBindRequest>,
) -> Result<Response<SourceBinding>, Status> {
    super::authorize(server, &request, "SourceBind").map_err(Status::from)?;
    let req = request.into_inner();
    let repository = AsyncSourceRepository::new(server.state.async_database.clone());
    let binding = repository
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
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok(Response::new(binding_to_proto(binding)))
}

pub(crate) async fn replay(
    server: &OrchestratorServer,
    request: Request<SourceReplayRequest>,
) -> Result<Response<SourceReplayResponse>, Status> {
    super::authorize(server, &request, "SourceReplay").map_err(Status::from)?;
    let id = request.into_inner().id;
    AsyncSourceRepository::new(server.state.async_database.clone())
        .replay(&id)
        .await
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok(Response::new(SourceReplayResponse {
        id,
        status: "received".to_string(),
    }))
}
