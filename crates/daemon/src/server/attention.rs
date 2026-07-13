use std::pin::Pin;

use agent_orchestrator::attention::{AttentionFilter, AttentionMutation};
use futures::Stream;
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;

pub(crate) type AttentionFollowStream =
    Pin<Box<dyn Stream<Item = Result<AttentionDelta, Status>> + Send>>;

fn item_to_proto(item: agent_orchestrator::attention::AttentionItem) -> AttentionItem {
    AttentionItem {
        id: item.id,
        project_id: item.project_id,
        task_id: item.task_id,
        task_item_id: item.task_item_id,
        step_id: item.step_id,
        session_id: item.session_id,
        kind: item.kind,
        severity: item.severity,
        state: item.state,
        title: item.title,
        summary: item.summary,
        requested_decision_json: item.requested_decision.map(|value| value.to_string()),
        actions: item
            .actions
            .into_iter()
            .map(|action| AttentionActionDescriptor {
                id: action.id,
                label: action.label,
                required_role: action.required_role,
                confirmation: action.confirmation,
                input_schema_json: action.input_schema.to_string(),
            })
            .collect(),
        assignee: item.assignee,
        source_event_id: item.source_event_id,
        occurrence_count: item.occurrence_count,
        reopen_count: item.reopen_count,
        version: item.version,
        created_at: item.created_at,
        updated_at: item.updated_at,
        last_occurred_at: item.last_occurred_at,
        snoozed_until: item.snoozed_until,
        sla_deadline: item.sla_deadline,
        resolved_at: item.resolved_at,
        resolution_json: item.resolution.map(|value| value.to_string()),
    }
}

fn validate_idempotency(key: &str) -> Result<(), Status> {
    if key.is_empty() || key.len() > 128 {
        return Err(Status::invalid_argument(
            "idempotency_key must contain 1-128 characters",
        ));
    }
    Ok(())
}

fn mutation_error(error: anyhow::Error) -> Status {
    let message = error.to_string();
    if message.contains("not found") {
        Status::not_found(message)
    } else if message.contains("version conflict") || message.contains("cannot be applied") {
        Status::aborted(message)
    } else if message.contains("idempotency") {
        Status::already_exists(message)
    } else {
        Status::invalid_argument(message)
    }
}

pub(crate) async fn attention_list(
    server: &OrchestratorServer,
    request: Request<AttentionListRequest>,
) -> Result<Response<AttentionListResponse>, Status> {
    super::authorize(server, &request, "AttentionList").map_err(Status::from)?;
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    let items = server
        .state
        .attention_repo
        .list(
            AttentionFilter {
                project_id: req.project_id,
                state: req.state,
                kind: req.kind,
                severity: req.severity,
                assignee: req.assignee,
                task_id: req.task_id,
                limit: if req.limit == 0 {
                    100
                } else {
                    req.limit as usize
                },
            },
            Some(&actor),
        )
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let latest_change_id = server
        .state
        .attention_repo
        .latest_change_id()
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(AttentionListResponse {
        items: items.into_iter().map(item_to_proto).collect(),
        latest_change_id,
    }))
}

pub(crate) async fn attention_get(
    server: &OrchestratorServer,
    request: Request<AttentionGetRequest>,
) -> Result<Response<AttentionItem>, Status> {
    super::authorize(server, &request, "AttentionGet").map_err(Status::from)?;
    let item = server
        .state
        .attention_repo
        .get(&request.into_inner().id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("attention item not found"))?;
    Ok(Response::new(item_to_proto(item)))
}

pub(crate) async fn attention_claim(
    server: &OrchestratorServer,
    request: Request<AttentionClaimRequest>,
) -> Result<Response<AttentionItem>, Status> {
    super::authorize(server, &request, "AttentionClaim").map_err(Status::from)?;
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Claim,
    )
    .await
}

pub(crate) async fn attention_snooze(
    server: &OrchestratorServer,
    request: Request<AttentionSnoozeRequest>,
) -> Result<Response<AttentionItem>, Status> {
    super::authorize(server, &request, "AttentionSnooze").map_err(Status::from)?;
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    let until = chrono::DateTime::parse_from_rfc3339(&req.until)
        .map_err(|_| Status::invalid_argument("until must be RFC3339"))?;
    if until <= chrono::Utc::now() {
        return Err(Status::invalid_argument("until must be in the future"));
    }
    mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Snooze { until: req.until },
    )
    .await
}

pub(crate) async fn attention_resolve(
    server: &OrchestratorServer,
    request: Request<AttentionResolveRequest>,
) -> Result<Response<AttentionItem>, Status> {
    super::authorize(server, &request, "AttentionResolve").map_err(Status::from)?;
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    if req.reason.trim().is_empty() || req.reason.len() > 500 {
        return Err(Status::invalid_argument(
            "reason must contain 1-500 characters",
        ));
    }
    mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Resolve { reason: req.reason },
    )
    .await
}

async fn mutate(
    server: &OrchestratorServer,
    id: &str,
    expected_version: i64,
    key: &str,
    actor: &str,
    mutation: AttentionMutation,
) -> Result<Response<AttentionItem>, Status> {
    let item = server
        .state
        .attention_repo
        .mutate(id, expected_version, key, actor, mutation)
        .await
        .map_err(mutation_error)?;
    Ok(Response::new(item_to_proto(item)))
}

pub(crate) async fn attention_execute_action(
    server: &OrchestratorServer,
    request: Request<AttentionExecuteActionRequest>,
) -> Result<Response<AttentionItem>, Status> {
    super::authorize(server, &request, "AttentionExecuteAction").map_err(Status::from)?;
    if let Some(status) = server.reject_new_work_during_shutdown("AttentionExecuteAction") {
        return Err(status);
    }
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    if req.input_json.len() > 4096 {
        return Err(Status::invalid_argument("action input exceeds 4096 bytes"));
    }
    let input: serde_json::Value = serde_json::from_str(&req.input_json)
        .map_err(|_| Status::invalid_argument("input_json must be valid JSON"))?;
    if !input.is_object() {
        return Err(Status::invalid_argument("action input must be an object"));
    }
    let item = orchestrator_scheduler::service::attention::execute_allowlisted_action(
        &server.state,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        &req.action_id,
        &input,
    )
    .await
    .map_err(mutation_error)?;
    Ok(Response::new(item_to_proto(item)))
}

pub(crate) async fn attention_follow(
    server: &OrchestratorServer,
    request: Request<AttentionFollowRequest>,
) -> Result<Response<AttentionFollowStream>, Status> {
    super::authorize(server, &request, "AttentionFollow").map_err(Status::from)?;
    let req = request.into_inner();
    let state = server.state.clone();
    let interval = std::time::Duration::from_millis(if req.interval_millis == 0 {
        500
    } else {
        req.interval_millis.clamp(250, 5_000)
    });
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut cursor = req.after_change_id.max(0);
        loop {
            let changes = match state.attention_repo.changes_since(cursor, 200).await {
                Ok(changes) => changes,
                Err(error) => {
                    let _ = tx.send(Err(Status::internal(error.to_string()))).await;
                    break;
                }
            };
            for change in changes {
                cursor = change.id;
                let item = match state.attention_repo.get(&change.attention_item_id).await {
                    Ok(item) => item,
                    Err(error) => {
                        let _ = tx.send(Err(Status::internal(error.to_string()))).await;
                        return;
                    }
                };
                if let Some(item) = item {
                    if req
                        .project_id
                        .as_ref()
                        .is_some_and(|project| project != &item.project_id)
                    {
                        continue;
                    }
                    if tx
                        .send(Ok(AttentionDelta {
                            kind: change.change_kind,
                            change_id: change.id,
                            item: Some(item_to_proto(item)),
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            tokio::time::sleep(interval).await;
        }
    });
    Ok(Response::new(Box::pin(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    )))
}
