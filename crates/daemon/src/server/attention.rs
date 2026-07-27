use std::pin::Pin;

use agent_orchestrator::attention::{AttentionFilter, AttentionMutation, attention_filter_matches};
use futures::Stream;
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

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
        source_route_id: item.source_route_id,
        source_binding_name: item.source_binding_name,
    }
}

fn notification_descriptor(
    change: &agent_orchestrator::attention::AttentionChange,
    item: &agent_orchestrator::attention::AttentionItem,
) -> Option<AttentionNotificationDescriptor> {
    if !matches!(change.change_kind.as_str(), "open" | "reopen")
        || item.state == "resolved"
        || (item.severity != "intervention" && item.kind != "approval_required")
    {
        return None;
    }
    let title = item.title.chars().take(96).collect::<String>();
    Some(AttentionNotificationDescriptor {
        dedupe_key: format!("{}:{}", item.id, change.item_version),
        attention_item_id: item.id.clone(),
        item_version: change.item_version,
        title,
        severity: item.severity.clone(),
        process_id: item.task_id.clone(),
        deep_link: format!("#/attention/{}", item.id),
    })
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
                active_only: req.active_only,
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
    mut request: Request<AttentionClaimRequest>,
) -> Result<Response<AttentionItem>, Status> {
    let current = load_for_audit(server, &request.get_ref().id).await?;
    let context = request.get_ref().audit.clone();
    let key = request.get_ref().idempotency_key.clone();
    let expected = request.get_ref().expected_version;
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AttentionClaim",
        context.as_ref(),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "attention_item",
            target_id: &current.id,
            action: "attention.claim",
            expected_version: Some(expected.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"expected_version":expected}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching attention claim already audited",
        )));
    }
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    audited_mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Claim,
        &attempt,
        &current.task_id,
        "attention_claimed",
    )
    .await
}

pub(crate) async fn attention_snooze(
    server: &OrchestratorServer,
    mut request: Request<AttentionSnoozeRequest>,
) -> Result<Response<AttentionItem>, Status> {
    let current = load_for_audit(server, &request.get_ref().id).await?;
    let context = request.get_ref().audit.clone();
    let key = request.get_ref().idempotency_key.clone();
    let expected = request.get_ref().expected_version;
    let until = request.get_ref().until.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AttentionSnooze",
        context.as_ref(),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "attention_item",
            target_id: &current.id,
            action: "attention.snooze",
            expected_version: Some(expected.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"expected_version":expected,"until":until}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching attention snooze already audited",
        )));
    }
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    let until = chrono::DateTime::parse_from_rfc3339(&req.until)
        .map_err(|_| Status::invalid_argument("until must be RFC3339"))?;
    if until <= chrono::Utc::now() {
        return Err(Status::invalid_argument("until must be in the future"));
    }
    audited_mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Snooze { until: req.until },
        &attempt,
        &current.task_id,
        "attention_snoozed",
    )
    .await
}

pub(crate) async fn attention_resolve(
    server: &OrchestratorServer,
    mut request: Request<AttentionResolveRequest>,
) -> Result<Response<AttentionItem>, Status> {
    let current = load_for_audit(server, &request.get_ref().id).await?;
    let context = request.get_ref().audit.clone();
    let key = request.get_ref().idempotency_key.clone();
    let expected = request.get_ref().expected_version;
    let reason = request.get_ref().reason.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AttentionResolve",
        context.as_ref(),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "attention_item",
            target_id: &current.id,
            action: "attention.resolve",
            expected_version: Some(expected.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"expected_version":expected,"reason":reason}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: Some(&reason),
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching attention resolve already audited",
        )));
    }
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    validate_idempotency(&req.idempotency_key)?;
    if req.reason.trim().is_empty() || req.reason.len() > 500 {
        return Err(Status::invalid_argument(
            "reason must contain 1-500 characters",
        ));
    }
    audited_mutate(
        server,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        AttentionMutation::Resolve { reason: req.reason },
        &attempt,
        &current.task_id,
        "attention_resolved",
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn audited_mutate(
    server: &OrchestratorServer,
    id: &str,
    expected_version: i64,
    key: &str,
    actor: &str,
    mutation: AttentionMutation,
    attempt: &action_audit::ActionAttempt,
    task_id: &str,
    event_type: &str,
) -> Result<Response<AttentionItem>, Status> {
    let item = match server
        .state
        .attention_repo
        .mutate(id, expected_version, key, actor, mutation)
        .await
    {
        Ok(item) => item,
        Err(error) => return Err(attempt.failed(server, mutation_error(error)).await),
    };
    link_domain_action(server, id, key, &attempt.request_id).await?;
    agent_orchestrator::events::insert_event(
        &server.state,
        task_id,
        item.task_item_id.as_deref(),
        event_type,
        serde_json::json!({"request_id":attempt.request_id,"attention_item_id":id,"actor":actor}),
    )
    .await
    .map_err(|error| attempt.status(Status::internal(error.to_string())))?;
    attempt
        .succeeded(server, Some("attention_action"), Some(id))
        .await?;
    Ok(attempt.response(item_to_proto(item)))
}

pub(crate) async fn attention_execute_action(
    server: &OrchestratorServer,
    mut request: Request<AttentionExecuteActionRequest>,
) -> Result<Response<AttentionItem>, Status> {
    let current = load_for_audit(server, &request.get_ref().id).await?;
    let context = request.get_ref().audit.clone();
    let key = request.get_ref().idempotency_key.clone();
    let expected = request.get_ref().expected_version;
    let action_id = request.get_ref().action_id.clone();
    let input_json = request.get_ref().input_json.clone();
    let action = format!("attention.execute.{action_id}");
    let attempt = action_audit::begin(
        server,
        &mut request,
        "AttentionExecuteAction",
        context.as_ref(),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "attention_item",
            target_id: &current.id,
            action: &action,
            expected_version: Some(expected.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({"expected_version":expected,"action_id":action_id,"input_json":input_json}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: Some(&key),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching attention action already audited",
        )));
    }
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
    let item = match orchestrator_scheduler::service::attention::execute_allowlisted_action(
        &server.state,
        &req.id,
        req.expected_version,
        &req.idempotency_key,
        &actor,
        &req.action_id,
        &input,
    )
    .await
    {
        Ok(item) => item,
        Err(error) => return Err(attempt.failed(server, mutation_error(error)).await),
    };
    link_domain_action(server, &req.id, &req.idempotency_key, &attempt.request_id).await?;
    agent_orchestrator::events::insert_event(
        &server.state,
        &current.task_id,
        current.task_item_id.as_deref(),
        "attention_action_executed",
        serde_json::json!({"request_id":attempt.request_id,"attention_item_id":req.id,"action_id":req.action_id,"actor":actor}),
    )
    .await
    .map_err(|error| attempt.status(Status::internal(error.to_string())))?;
    attempt
        .succeeded(server, Some("attention_action"), Some(&req.id))
        .await?;
    Ok(attempt.response(item_to_proto(item)))
}

async fn load_for_audit(
    server: &OrchestratorServer,
    id: &str,
) -> Result<agent_orchestrator::attention::AttentionItem, Status> {
    server
        .state
        .attention_repo
        .get(id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("attention item not found"))
}

async fn link_domain_action(
    server: &OrchestratorServer,
    id: &str,
    key: &str,
    request_id: &str,
) -> Result<(), Status> {
    let id = id.to_string();
    let key = key.to_string();
    let request_id = request_id.to_string();
    agent_orchestrator::audit_links::link_attention_action(
        &server.state.async_database,
        id,
        key,
        request_id,
    )
    .await
    .map_err(|error| Status::internal(error.to_string()))
}

pub(crate) async fn attention_follow(
    server: &OrchestratorServer,
    request: Request<AttentionFollowRequest>,
) -> Result<Response<AttentionFollowStream>, Status> {
    super::authorize(server, &request, "AttentionFollow").map_err(Status::from)?;
    let actor = super::trusted_actor(&request);
    let req = request.into_inner();
    let filter = AttentionFilter {
        project_id: req.project_id,
        state: req.state,
        active_only: req.active_only,
        kind: req.kind,
        severity: req.severity,
        assignee: req.assignee,
        task_id: req.task_id,
        limit: 200,
    };
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
                    let matches = attention_filter_matches(&item, &filter, Some(&actor));
                    let notification = matches
                        .then(|| notification_descriptor(&change, &item))
                        .flatten();
                    if tx
                        .send(Ok(AttentionDelta {
                            kind: if matches { "upsert" } else { "remove" }.to_string(),
                            change_id: change.id,
                            item: Some(item_to_proto(item)),
                            notification,
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::attention::{
        AttentionActionDescriptor, AttentionChange, AttentionItem as CoreAttentionItem,
    };

    fn item() -> CoreAttentionItem {
        CoreAttentionItem {
            id: "attention-1".into(),
            project_id: "project-1".into(),
            task_id: "process-1".into(),
            task_item_id: None,
            step_id: Some("qa".into()),
            session_id: None,
            kind: "step_failed".into(),
            severity: "intervention".into(),
            state: "open".into(),
            title: format!("{} secret body", "A".repeat(100)),
            summary: "must never enter notification".into(),
            requested_decision: None,
            actions: Vec::new(),
            dedupe_key: "failure".into(),
            assignee: None,
            source_event_id: "event-1".into(),
            source_route_id: None,
            source_binding_name: None,
            occurrence_count: 1,
            reopen_count: 0,
            version: 1,
            created_at: "2026-07-14T00:00:00Z".into(),
            updated_at: "2026-07-14T00:00:00Z".into(),
            last_occurred_at: "2026-07-14T00:00:00Z".into(),
            snoozed_until: None,
            sla_deadline: None,
            resolved_at: None,
            resolution: None,
        }
    }

    #[test]
    fn descriptor_is_bounded_allowlisted_and_transition_scoped() {
        let open = AttentionChange {
            id: 1,
            attention_item_id: "attention-1".into(),
            change_kind: "open".into(),
            item_version: 1,
        };
        let descriptor = notification_descriptor(&open, &item()).expect("descriptor");
        assert_eq!(descriptor.dedupe_key, "attention-1:1");
        assert_eq!(descriptor.process_id, "process-1");
        assert_eq!(descriptor.deep_link, "#/attention/attention-1");
        assert_eq!(descriptor.title.chars().count(), 96);
        assert!(!descriptor.title.contains("secret body"));

        let update = AttentionChange {
            change_kind: "upsert".into(),
            item_version: 2,
            ..open
        };
        assert!(notification_descriptor(&update, &item()).is_none());
    }

    #[test]
    fn descriptor_requires_an_actionable_open_or_reopen_transition() {
        let change = AttentionChange {
            id: 1,
            attention_item_id: "attention-1".into(),
            change_kind: "open".into(),
            item_version: 1,
        };

        let mut resolved = item();
        resolved.state = "resolved".into();
        assert!(notification_descriptor(&change, &resolved).is_none());

        let mut informational = item();
        informational.severity = "attention".into();
        assert!(notification_descriptor(&change, &informational).is_none());

        informational.kind = "approval_required".into();
        assert!(notification_descriptor(&change, &informational).is_some());

        let remove = AttentionChange {
            change_kind: "remove".into(),
            ..change
        };
        assert!(notification_descriptor(&remove, &item()).is_none());
    }

    #[test]
    fn proto_projection_preserves_typed_fields_and_serializes_json() {
        let mut source = item();
        source.requested_decision = Some(serde_json::json!({"question":"Retry?"}));
        source.resolution = Some(serde_json::json!({"reason":"fixed"}));
        source.source_route_id = Some("route-1".into());
        source.source_binding_name = Some("binding-1".into());
        source.actions.push(AttentionActionDescriptor {
            id: "retry".into(),
            label: "Retry".into(),
            required_role: "operator".into(),
            confirmation: "required".into(),
            input_schema: serde_json::json!({"type":"object"}),
        });

        let projected = item_to_proto(source);

        assert_eq!(
            projected.requested_decision_json.as_deref(),
            Some(r#"{"question":"Retry?"}"#)
        );
        assert_eq!(
            projected.resolution_json.as_deref(),
            Some(r#"{"reason":"fixed"}"#)
        );
        assert_eq!(projected.source_route_id.as_deref(), Some("route-1"));
        assert_eq!(projected.source_binding_name.as_deref(), Some("binding-1"));
        assert_eq!(projected.actions.len(), 1);
        assert_eq!(projected.actions[0].id, "retry");
        assert_eq!(
            projected.actions[0].input_schema_json,
            r#"{"type":"object"}"#
        );
    }

    #[test]
    fn mutation_validation_and_error_mapping_are_stable() {
        assert!(validate_idempotency("retry-1").is_ok());
        assert_eq!(
            validate_idempotency("").expect_err("empty key").code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            validate_idempotency(&"x".repeat(129))
                .expect_err("oversized key")
                .code(),
            tonic::Code::InvalidArgument
        );

        assert_eq!(
            mutation_error(anyhow::anyhow!("attention item not found")).code(),
            tonic::Code::NotFound
        );
        assert_eq!(
            mutation_error(anyhow::anyhow!("version conflict")).code(),
            tonic::Code::Aborted
        );
        assert_eq!(
            mutation_error(anyhow::anyhow!("mutation cannot be applied")).code(),
            tonic::Code::Aborted
        );
        assert_eq!(
            mutation_error(anyhow::anyhow!("idempotency replay")).code(),
            tonic::Code::AlreadyExists
        );
        assert_eq!(
            mutation_error(anyhow::anyhow!("invalid transition")).code(),
            tonic::Code::InvalidArgument
        );
    }
}
