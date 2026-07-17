use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::source::{
    AsyncSourceRepository, CreateSourceBinding, IngestSourceEvent, NormalizedSourceEvent,
};
use agent_orchestrator::source_automation::{
    AdoptSourceAutomationGeneration, AsyncSourceAutomationRepository,
    SourceAutomationRoute as CoreSourceAutomationRoute, SourceAutomationRouteFilter,
};
use base64::Engine as _;
use futures::Stream;
use orchestrator_proto::*;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use tonic::{Request, Response, Status};

use super::OrchestratorServer;
use super::action_audit::{self, ActionDescriptor};

fn event_to_proto(
    value: agent_orchestrator::source::SourceEventRecord,
    route: Option<&CoreSourceAutomationRoute>,
) -> SourceEvent {
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
        automation_route_id: route.map(|route| route.id.clone()),
        automation_status: route.map(|route| route.status.clone()),
        automation_binding_name: route.map(|route| route.binding_name.clone()),
        automation_template_name: route.map(|route| route.template_name.clone()),
        automation_template_hash: route.map(|route| route.template_hash.clone()),
    }
}

pub(crate) type SourceAutomationWatchStream =
    Pin<Box<dyn Stream<Item = Result<SourceAutomationDelta, Status>> + Send>>;

fn automation_route_to_proto(
    value: CoreSourceAutomationRoute,
    include_permalink: bool,
) -> SourceAutomationRoute {
    SourceAutomationRoute {
        id: value.id,
        project_id: value.project_id,
        source_event_id: value.source_event_id,
        provider: value.provider,
        reaction: value.reaction,
        binding_name: value.binding_name,
        binding_revision: value.binding_revision,
        template_name: value.template_name,
        template_hash: value.template_hash,
        status: value.status,
        error_code: value.error_code,
        task_id: value.task_id,
        permalink: include_permalink.then_some(value.permalink).flatten(),
        request_id: value.request_id,
        created_at: value.created_at,
        completed_at: value.completed_at,
        error_category: value.error_category,
        generation: value.generation,
        version: value.version,
        attempt_count: value.attempt_count,
        max_attempts: value.max_attempts,
        next_attempt_at: value.next_attempt_at,
        lease_expires_at: value.lease_expires_at,
        suspended_scope: value.suspended_scope,
        last_attempt_at: value.last_attempt_at,
        updated_at: value.updated_at,
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
    let automation = AsyncSourceAutomationRepository::new(server.state.async_database.clone());
    let mut public_events = Vec::with_capacity(events.len());
    for event in events {
        let route = automation
            .get_for_event(&event.id)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        public_events.push(event_to_proto(event, route.as_ref()));
    }
    Ok(Response::new(SourceEventListResponse {
        events: public_events,
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
    let route = AsyncSourceAutomationRepository::new(server.state.async_database.clone())
        .get_for_event(&event.id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(event_to_proto(event, route.as_ref())))
}

pub(crate) async fn automation_route_get(
    server: &OrchestratorServer,
    request: Request<SourceAutomationRouteGetRequest>,
) -> Result<Response<SourceAutomationRoute>, Status> {
    super::authorize(server, &request, "SourceAutomationRouteGet").map_err(Status::from)?;
    let source_event_id = request.into_inner().source_event_id;
    if source_event_id.trim().is_empty() || source_event_id.len() > 128 {
        return Err(Status::invalid_argument(
            "source_event_id must contain 1-128 bytes",
        ));
    }
    let route = AsyncSourceAutomationRepository::new(server.state.async_database.clone())
        .get_for_event(&source_event_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("source automation route not found"))?;
    Ok(Response::new(automation_route_to_proto(route, true)))
}

#[derive(Debug, Serialize, Deserialize)]
struct RoutePageToken {
    created_at: String,
    id: String,
}

fn decode_route_page_token(value: Option<&str>) -> Result<Option<(String, String)>, Status> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > 1024 {
        return Err(Status::invalid_argument("page_token exceeds 1024 bytes"));
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| Status::invalid_argument("page_token is invalid"))?;
    let token: RoutePageToken = serde_json::from_slice(&bytes)
        .map_err(|_| Status::invalid_argument("page_token is invalid"))?;
    Ok(Some((token.created_at, token.id)))
}

fn encode_route_page_token(route: &CoreSourceAutomationRoute) -> Result<String, Status> {
    let bytes = serde_json::to_vec(&RoutePageToken {
        created_at: route.created_at.clone(),
        id: route.id.clone(),
    })
    .map_err(|error| Status::internal(error.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) async fn automation_list(
    server: &OrchestratorServer,
    request: Request<SourceAutomationListRequest>,
) -> Result<Response<SourceAutomationListResponse>, Status> {
    super::authorize(server, &request, "SourceAutomationList").map_err(Status::from)?;
    let req = request.into_inner();
    let page_size = if req.page_size == 0 {
        50
    } else {
        req.page_size.clamp(1, 200) as usize
    };
    let before = decode_route_page_token(req.page_token.as_deref())?;
    let routes = AsyncSourceAutomationRepository::new(server.state.async_database.clone())
        .list(SourceAutomationRouteFilter {
            project_id: req.project_id,
            state: req.state,
            provider: req.provider,
            binding_name: req.binding_name,
            task_id: req.task_id,
            before,
            limit: page_size,
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    let next_page_token = if routes.len() == page_size {
        routes.last().map(encode_route_page_token).transpose()?
    } else {
        None
    };
    Ok(Response::new(SourceAutomationListResponse {
        routes: routes
            .into_iter()
            .map(|route| automation_route_to_proto(route, false))
            .collect(),
        next_page_token,
    }))
}

pub(crate) async fn automation_get(
    server: &OrchestratorServer,
    request: Request<SourceAutomationGetRequest>,
) -> Result<Response<SourceAutomationDetail>, Status> {
    super::authorize(server, &request, "SourceAutomationGet").map_err(Status::from)?;
    let req = request.into_inner();
    if req.route_id.is_empty() || req.route_id.len() > 128 {
        return Err(Status::invalid_argument(
            "route_id must contain 1-128 bytes",
        ));
    }
    let repository = AsyncSourceAutomationRepository::new(server.state.async_database.clone());
    let route = repository
        .get(&req.route_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("source automation route not found"))?;
    let attempts = repository
        .attempts(
            &req.route_id,
            if req.attempt_limit == 0 {
                50
            } else {
                req.attempt_limit as usize
            },
        )
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(SourceAutomationDetail {
        route: Some(automation_route_to_proto(route, false)),
        attempts: attempts
            .into_iter()
            .map(|attempt| SourceAutomationRouteAttempt {
                id: attempt.id,
                route_id: attempt.route_id,
                generation: attempt.generation,
                attempt_no: attempt.attempt_no,
                started_at: attempt.started_at,
                completed_at: attempt.completed_at,
                result_state: attempt.result_state,
                error_code: attempt.error_code,
                error_category: attempt.error_category,
                retry_after_seconds: attempt.retry_after_seconds,
            })
            .collect(),
    }))
}

pub(crate) async fn automation_watch(
    server: &OrchestratorServer,
    request: Request<SourceAutomationWatchRequest>,
) -> Result<Response<SourceAutomationWatchStream>, Status> {
    super::authorize(server, &request, "SourceAutomationWatch").map_err(Status::from)?;
    let req = request.into_inner();
    let repository = AsyncSourceAutomationRepository::new(server.state.async_database.clone());
    let interval = std::time::Duration::from_millis(if req.interval_millis == 0 {
        500
    } else {
        req.interval_millis.clamp(250, 5_000) as u64
    });
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        let mut cursor = req.after_cursor.max(0);
        loop {
            let changes = match repository
                .changes_since(req.project_id.as_deref(), cursor, 200)
                .await
            {
                Ok(changes) => changes,
                Err(error) => {
                    let _ = tx.send(Err(Status::internal(error.to_string()))).await;
                    return;
                }
            };
            for change in changes {
                cursor = change.id;
                let route = match repository.get(&change.route_id).await {
                    Ok(Some(route)) => route,
                    Ok(None) => continue,
                    Err(error) => {
                        let _ = tx.send(Err(Status::internal(error.to_string()))).await;
                        return;
                    }
                };
                if tx
                    .send(Ok(SourceAutomationDelta {
                        cursor: change.id,
                        route_version: change.route_version,
                        state: change.state,
                        error_code: change.error_code,
                        route: Some(automation_route_to_proto(route, false)),
                        changed_at: change.created_at,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            tokio::time::sleep(interval).await;
        }
    });
    Ok(Response::new(Box::pin(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    )))
}

pub(crate) async fn automation_simulate(
    server: &OrchestratorServer,
    request: Request<SourceAutomationSimulateRequest>,
) -> Result<Response<SourceAutomationSimulateResponse>, Status> {
    super::authorize(server, &request, "SourceAutomationSimulate").map_err(Status::from)?;
    let req = request.into_inner();
    for (field, value) in [
        ("project_id", req.project_id.as_str()),
        ("provider", req.provider.as_str()),
        ("installation_id", req.installation_id.as_str()),
        ("event_kind", req.event_kind.as_str()),
        ("reaction", req.reaction.as_str()),
        ("target_kind", req.target_kind.as_str()),
        ("channel_id", req.channel_id.as_str()),
        ("external_actor_id", req.external_actor_id.as_str()),
        ("message_url", req.message_url.as_str()),
        ("target_id", req.target_id.as_str()),
    ] {
        if value.is_empty() || value.len() > 2048 {
            return Err(Status::invalid_argument(format!(
                "{field} must contain 1-2048 bytes"
            )));
        }
    }
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let simulation = agent_orchestrator::source_automation::simulate_source_automation(
        &active.config,
        &agent_orchestrator::source_automation::SourceAutomationSimulationInput {
            project_id: req.project_id.clone(),
            match_input: agent_orchestrator::source_task_binding::SourceTaskBindingMatchInput {
                provider: req.provider,
                installation_id: req.installation_id,
                event_kind: req.event_kind,
                reaction: req.reaction,
                target_kind: req.target_kind,
                channel_id: req.channel_id,
                external_actor_id: req.external_actor_id,
            },
            message_url: req.message_url,
            event_id: req.event_id,
            target_id: req.target_id,
        },
    )
    .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let match_result = simulation.match_result;
    let public_rendered = simulation.rendered.map(|rendered| {
        let policy = active.config.runtime_policy_for_project(&req.project_id);
        agent_orchestrator::source_task_template::redact_rendered_source_task_template(
            &rendered,
            &policy.runner.redaction_patterns,
        )
    });
    Ok(Response::new(SourceAutomationSimulateResponse {
        match_result: Some(SourceTaskBindingSimulateResponse {
            status: match_result.status,
            reason: match_result.reason,
            trigger_name: match_result.trigger_name,
            resolved_role: match_result.resolved_role,
            binding_id: match_result.binding_id,
            template_ref: match_result.template_ref,
            binding_revision: match_result.binding_revision,
            candidates: match_result
                .candidates
                .into_iter()
                .map(|candidate| SourceTaskBindingCandidate {
                    binding_id: candidate.binding_id,
                    reason: candidate.reason,
                    revision: candidate.revision,
                })
                .collect(),
        }),
        rendered: public_rendered.map(|rendered| SourceTaskTemplatePreviewResponse {
            name: String::new(),
            project_id: req.project_id,
            skill_name: rendered.skill_name,
            skill_invocation: rendered.skill_invocation,
            skill_args: rendered.skill_args,
            goal: rendered.goal,
            workflow: rendered.action.workflow,
            workspace: rendered.action.workspace,
            start: rendered.action.start,
            initial_vars: rendered.action.initial_vars.into_iter().collect(),
            content_hash: rendered.content_hash,
            revision: rendered.revision,
            warnings: rendered.warnings,
        }),
        mutation_performed: false,
        network_performed: false,
    }))
}

pub(crate) async fn automation_replay(
    server: &OrchestratorServer,
    request: Request<SourceAutomationMutationRequest>,
) -> Result<Response<SourceAutomationRoute>, Status> {
    mutate_automation_route(server, request, false).await
}

pub(crate) async fn automation_ignore(
    server: &OrchestratorServer,
    request: Request<SourceAutomationMutationRequest>,
) -> Result<Response<SourceAutomationRoute>, Status> {
    mutate_automation_route(server, request, true).await
}

async fn mutate_automation_route(
    server: &OrchestratorServer,
    mut request: Request<SourceAutomationMutationRequest>,
    ignore: bool,
) -> Result<Response<SourceAutomationRoute>, Status> {
    let req = request.get_ref();
    if req.route_id.is_empty() || req.route_id.len() > 128 {
        return Err(Status::invalid_argument(
            "route_id must contain 1-128 bytes",
        ));
    }
    if req.expected_version < 1 {
        return Err(Status::invalid_argument(
            "expected_version must be positive",
        ));
    }
    if req.reason.trim().is_empty() || req.reason.len() > 500 {
        return Err(Status::invalid_argument("reason must contain 1-500 bytes"));
    }
    if req.idempotency_key.is_empty() || req.idempotency_key.len() > 128 {
        return Err(Status::invalid_argument(
            "idempotency_key must contain 1-128 bytes",
        ));
    }
    if ignore && req.adopt_current_config {
        return Err(Status::invalid_argument(
            "adopt_current_config is only valid for replay",
        ));
    }
    let repository = AsyncSourceAutomationRepository::new(server.state.async_database.clone());
    let current = repository
        .get(&req.route_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("source automation route not found"))?;
    let context = ActionAuditContext {
        reason_code: if ignore {
            "operator_source_automation_ignore".to_string()
        } else {
            "operator_source_automation_replay".to_string()
        },
        operator_reason: Some(req.reason.clone()),
        idempotency_key: Some(req.idempotency_key.clone()),
    };
    let action = if ignore {
        "source.automation.ignore"
    } else {
        "source.automation.replay"
    };
    let rpc = if ignore {
        "SourceAutomationIgnore"
    } else {
        "SourceAutomationReplay"
    };
    let route_id = req.route_id.clone();
    let expected_version = req.expected_version;
    let adopt_current_config = req.adopt_current_config;
    let attempt = action_audit::begin(
        server,
        &mut request,
        rpc,
        Some(&context),
        ActionDescriptor {
            project_id: &current.project_id,
            target_type: "source_automation_route",
            target_id: &route_id,
            action,
            expected_version: Some(expected_version.to_string()),
            fencing_token: None,
            canonical_request: serde_json::json!({
                "route_id":route_id,
                "expected_version":expected_version,
                "generation":current.generation,
                "adopt_current_config":adopt_current_config
            }),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: context.idempotency_key.as_deref(),
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        let route = repository
            .get(&route_id)
            .await
            .map_err(|error| attempt.status(Status::internal(error.to_string())))?
            .ok_or_else(|| attempt.status(Status::not_found("route not found")))?;
        return Ok(attempt.response(automation_route_to_proto(route, false)));
    }
    let mutation = if ignore {
        repository.ignore(&route_id, expected_version).await
    } else if adopt_current_config {
        adopt_current_route_generation(
            server,
            &repository,
            &current,
            expected_version,
            &attempt.request_id,
        )
        .await
    } else {
        repository.replay(&route_id, expected_version).await
    };
    let mutation = match mutation {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("version conflict") {
                Status::aborted(message)
            } else {
                Status::failed_precondition(message)
            };
            return Err(attempt.failed(server, status).await);
        }
    };
    let source_repository = AsyncSourceRepository::new(server.state.async_database.clone());
    if ignore {
        source_repository
            .complete_automation_route(
                &route_id,
                "ignored",
                mutation.route.task_id.as_deref(),
                mutation.route.error_code.as_deref(),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        server
            .state
            .attention_repo
            .resolve_external_candidate(
                &mutation.route.project_id,
                &format!("source-automation:{}", mutation.route.automation_key),
                &mutation.route.source_event_id,
                "operator_ignored_route",
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    } else {
        source_repository
            .requeue_automation_route(&route_id)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }
    attempt
        .succeeded(server, Some("source_automation_route"), Some(&route_id))
        .await?;
    Ok(attempt.response(automation_route_to_proto(mutation.route, false)))
}

async fn adopt_current_route_generation(
    server: &OrchestratorServer,
    repository: &AsyncSourceAutomationRepository,
    route: &CoreSourceAutomationRoute,
    expected_version: i64,
    request_id: &str,
) -> anyhow::Result<agent_orchestrator::source_automation::SourceAutomationMutationResult> {
    let source = AsyncSourceRepository::new(server.state.async_database.clone())
        .get(&route.source_event_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("source event missing"))?;
    let reaction = source
        .normalized
        .reaction
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("source reaction metadata missing"))?;
    let active = agent_orchestrator::config_load::read_active_config(&server.state)?;
    let matched = agent_orchestrator::source_task_binding::match_source_task_binding(
        &active.config,
        &route.project_id,
        &agent_orchestrator::source_task_binding::SourceTaskBindingMatchInput {
            provider: route.provider.clone(),
            installation_id: route.installation_id.clone(),
            event_kind: "reaction_added".to_string(),
            reaction: reaction.name.clone(),
            target_kind: reaction.target.kind.clone(),
            channel_id: route.channel_id.clone(),
            external_actor_id: source.external_actor_id.clone().unwrap_or_default(),
        },
    )?;
    if matched.status != "matched" || matched.binding_id.as_deref() != Some(&route.binding_name) {
        anyhow::bail!("current policy no longer authorizes the same binding");
    }
    let project = active
        .config
        .projects
        .get(&route.project_id)
        .ok_or_else(|| anyhow::anyhow!("project missing"))?;
    let binding = project
        .source_task_bindings
        .get(&route.binding_name)
        .ok_or_else(|| anyhow::anyhow!("binding missing"))?;
    let template_name = matched
        .template_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("template selection missing"))?;
    let template = project
        .source_task_templates
        .get(template_name)
        .ok_or_else(|| anyhow::anyhow!("template missing"))?;
    let trigger_name = matched
        .trigger_name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("trigger selection missing"))?;
    let credential = project
        .triggers
        .get(trigger_name)
        .and_then(|trigger| trigger.event.as_ref())
        .and_then(|event| event.webhook.as_ref())
        .and_then(|webhook| webhook.outbound_credential.as_ref())
        .ok_or_else(|| anyhow::anyhow!("outbound credential reference missing"))?;
    repository
        .adopt_generation(AdoptSourceAutomationGeneration {
            route_id: route.id.clone(),
            expected_version,
            resolved_role: matched
                .resolved_role
                .ok_or_else(|| anyhow::anyhow!("resolved role missing"))?,
            binding_name: route.binding_name.clone(),
            binding_revision: matched
                .binding_revision
                .ok_or_else(|| anyhow::anyhow!("binding revision missing"))?,
            template_name: template_name.to_string(),
            template_hash: agent_orchestrator::source_task_template::template_content_hash(
                template,
            )?,
            binding_snapshot: serde_json::to_value(binding)?,
            template_snapshot: serde_json::to_value(template)?,
            credential_store: credential.from_ref.clone(),
            credential_key: credential.key.clone(),
            created_by_request_id: request_id.to_string(),
        })
        .await
}

pub(crate) async fn automation_status_get(
    server: &OrchestratorServer,
    request: Request<SourceAutomationStatusRequest>,
) -> Result<Response<SourceAutomationStatusResponse>, Status> {
    super::authorize(server, &request, "SourceAutomationStatusGet").map_err(Status::from)?;
    let project_id = request.into_inner().project_id;
    if project_id.is_empty() || project_id.len() > 128 {
        return Err(Status::invalid_argument(
            "project_id must contain 1-128 bytes",
        ));
    }
    let status = AsyncSourceAutomationRepository::new(server.state.async_database.clone())
        .status(&project_id, chrono::Utc::now())
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(SourceAutomationStatusResponse {
        project_id: status.project_id,
        backlog_count: status.backlog_count,
        oldest_age_seconds: status.oldest_age_seconds,
        active_leases: status.active_leases,
        retrying_count: status.retrying_count,
        needs_attention_count: status.needs_attention_count,
        failure_categories: status
            .failure_categories
            .into_iter()
            .map(|(category, count)| SourceAutomationFailureCount { category, count })
            .collect(),
    }))
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

pub(crate) async fn task_binding_simulate(
    server: &OrchestratorServer,
    request: Request<SourceTaskBindingSimulateRequest>,
) -> Result<Response<SourceTaskBindingSimulateResponse>, Status> {
    super::authorize(server, &request, "SourceTaskBindingSimulate").map_err(Status::from)?;
    let req = request.into_inner();
    for (field, value) in [
        ("project_id", req.project_id.as_str()),
        ("provider", req.provider.as_str()),
        ("installation_id", req.installation_id.as_str()),
        ("event_kind", req.event_kind.as_str()),
        ("reaction", req.reaction.as_str()),
        ("target_kind", req.target_kind.as_str()),
        ("channel_id", req.channel_id.as_str()),
        ("external_actor_id", req.external_actor_id.as_str()),
    ] {
        if value.is_empty() || value.len() > 256 {
            return Err(Status::invalid_argument(format!(
                "{field} must contain 1-256 bytes"
            )));
        }
    }
    let active = agent_orchestrator::config_load::read_active_config(&server.state)
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let result = agent_orchestrator::source_task_binding::match_source_task_binding(
        &active.config,
        &req.project_id,
        &agent_orchestrator::source_task_binding::SourceTaskBindingMatchInput {
            provider: req.provider,
            installation_id: req.installation_id,
            event_kind: req.event_kind,
            reaction: req.reaction,
            target_kind: req.target_kind,
            channel_id: req.channel_id,
            external_actor_id: req.external_actor_id,
        },
    )
    .map_err(|error| Status::failed_precondition(error.to_string()))?;
    Ok(Response::new(SourceTaskBindingSimulateResponse {
        status: result.status,
        reason: result.reason,
        trigger_name: result.trigger_name,
        resolved_role: result.resolved_role,
        binding_id: result.binding_id,
        template_ref: result.template_ref,
        binding_revision: result.binding_revision,
        candidates: result
            .candidates
            .into_iter()
            .map(|candidate| SourceTaskBindingCandidate {
                binding_id: candidate.binding_id,
                reason: candidate.reason,
                revision: candidate.revision,
            })
            .collect(),
    }))
}

pub(crate) async fn task_binding_suspend(
    server: &OrchestratorServer,
    request: Request<SourceTaskBindingMutationRequest>,
) -> Result<Response<SourceTaskBindingMutationResponse>, Status> {
    mutate_task_binding(server, request, true).await
}

pub(crate) async fn task_binding_resume(
    server: &OrchestratorServer,
    request: Request<SourceTaskBindingMutationRequest>,
) -> Result<Response<SourceTaskBindingMutationResponse>, Status> {
    mutate_task_binding(server, request, false).await
}

async fn mutate_task_binding(
    server: &OrchestratorServer,
    mut request: Request<SourceTaskBindingMutationRequest>,
    suspend: bool,
) -> Result<Response<SourceTaskBindingMutationResponse>, Status> {
    let rpc = if suspend {
        "SourceTaskBindingSuspend"
    } else {
        "SourceTaskBindingResume"
    };
    let action = if suspend {
        "source.binding.suspend"
    } else {
        "source.binding.resume"
    };
    let project_id = if request.get_ref().project_id.trim().is_empty() {
        agent_orchestrator::config::DEFAULT_PROJECT_ID.to_string()
    } else {
        request.get_ref().project_id.clone()
    };
    let name = request.get_ref().name.clone();
    if name.is_empty() || name.len() > 253 {
        return Err(Status::invalid_argument("name must contain 1-253 bytes"));
    }
    let context = request.get_ref().audit.clone();
    let attempt = action_audit::begin(
        server,
        &mut request,
        rpc,
        context.as_ref(),
        ActionDescriptor {
            project_id: &project_id,
            target_type: "source_task_binding",
            target_id: &name,
            action,
            expected_version: None,
            fencing_token: None,
            canonical_request: serde_json::json!({"name":name,"project_id":project_id,"suspend":suspend}),
            fallback_reason_code: "legacy_client",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption: false,
        },
    )
    .await?;
    if !attempt.should_execute {
        return Err(attempt.status(Status::already_exists(
            "matching SourceTaskBinding mutation already audited",
        )));
    }
    let result = if suspend {
        agent_orchestrator::service::resource::suspend_source_task_binding(
            &server.state,
            &name,
            Some(&project_id),
        )
    } else {
        agent_orchestrator::service::resource::resume_source_task_binding(
            &server.state,
            &name,
            Some(&project_id),
        )
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            return Err(attempt.failed(server, super::map_core_error(error)).await);
        }
    };
    let route_repository =
        AsyncSourceAutomationRepository::new(server.state.async_database.clone());
    let scope = format!("binding:{name}");
    let route_projection = if suspend {
        route_repository
            .suspend_scope(&project_id, None, Some(&name), &scope)
            .await
    } else {
        route_repository
            .resume_scope(&project_id, None, Some(&name), &scope)
            .await
    };
    if let Err(error) = route_projection {
        return Err(attempt
            .failed(server, Status::internal(error.to_string()))
            .await);
    }
    attempt
        .succeeded(server, Some("source_task_binding"), Some(&result.revision))
        .await?;
    Ok(attempt.response(SourceTaskBindingMutationResponse {
        name: result.name.clone(),
        suspend: result.suspend,
        revision: result.revision,
        message: format!(
            "SourceTaskBinding '{}' {}",
            result.name,
            if result.suspend {
                "suspended"
            } else {
                "resumed"
            }
        ),
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
        event: Some(event_to_proto(result.event, None)),
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
    if AsyncSourceAutomationRepository::new(server.state.async_database.clone())
        .get_for_event(&current.id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .is_some()
    {
        return Err(Status::failed_precondition(
            "source event belongs to a durable automation route; use `source automation replay`",
        ));
    }
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
