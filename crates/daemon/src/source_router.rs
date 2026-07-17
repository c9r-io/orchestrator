//! Provider-neutral durable source routing worker.

use agent_orchestrator::action_audit::{ActionAuditReservation, AsyncActionAuditRepository};
use agent_orchestrator::attention::{
    AttentionActionDescriptor, AttentionCandidate, AttentionSeverity,
};
use agent_orchestrator::config_load::read_active_config;
use agent_orchestrator::source::{
    AsyncSourceRepository, CreateSourceBinding, SourceCommand, SourceCommandActionInput,
    SourceEventRecord, deterministic_task_id,
};
use agent_orchestrator::source_automation::{
    AsyncSourceAutomationRepository, ReserveSourceAutomationRoute, SourceAutomationRoute,
};
use agent_orchestrator::source_task_template::{
    SourceTaskTemplateRenderInput, render_source_task_template, template_content_hash,
};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::trigger_engine::{TriggerFireContext, fire_trigger_canonical_with_context};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

use crate::slack_api::SlackApiClient;

/// Routes one bounded batch of durably accepted source events.
pub async fn reconcile_source_once(state: &Arc<InnerState>) -> Result<usize> {
    let repository = AsyncSourceRepository::new(state.async_database.clone());
    let events = repository.claim_pending(100).await?;
    for event in &events {
        if let Err(error) = route_one(state, &repository, event).await {
            tracing::warn!(
                provider = %event.provider,
                installation_hash = %short_hash(&event.installation_id),
                external_event_hash = %short_hash(&event.external_event_id),
                source_event_id = %event.id,
                error = %error,
                "source event routing failed"
            );
            repository
                .complete_routing(&event.id, "failed", None, Some(stable_error_code(&error)))
                .await?;
        }
    }
    Ok(events.len())
}

async fn route_one(
    state: &Arc<InnerState>,
    repository: &AsyncSourceRepository,
    event: &SourceEventRecord,
) -> Result<()> {
    if event.normalized.kind == agent_orchestrator::source::SourceEventKind::ReactionAdded {
        return route_reaction_automation(state, repository, event).await;
    }
    if event.normalized.kind == agent_orchestrator::source::SourceEventKind::System {
        repository
            .complete_routing(&event.id, "ignored", None, Some("provider_system_event"))
            .await?;
        return Ok(());
    }
    let active = read_active_config(state)?;
    let project = active
        .config
        .projects
        .get(&event.project_id)
        .with_context(|| format!("source project not found: {}", event.project_id))?;
    let matching_triggers = project
        .triggers
        .iter()
        .filter(|(_, trigger)| {
            trigger
                .event
                .as_ref()
                .and_then(|value| value.webhook.as_ref())
                .is_some_and(|webhook| {
                    webhook.provider.as_deref() == Some(event.provider.as_str())
                        && webhook.installation_id.as_deref()
                            == Some(event.installation_id.as_str())
                })
        })
        .collect::<Vec<_>>();
    if matching_triggers.len() != 1 {
        materialize_ambiguity(
            state,
            event,
            "Source installation does not resolve to exactly one Trigger.",
        )
        .await?;
        repository
            .complete_routing(
                &event.id,
                "needs_attention",
                None,
                Some("trigger_ambiguous"),
            )
            .await?;
        return Ok(());
    }
    let (trigger_name, trigger) = matching_triggers[0];
    if trigger.suspend {
        repository
            .complete_routing(&event.id, "ignored", None, Some("installation_suspended"))
            .await?;
        return Ok(());
    }

    let conversation = event.normalized.conversation.as_ref();
    let bindings = if let Some(conversation) = conversation {
        repository
            .find_bindings(
                &event.provider,
                &event.installation_id,
                Some(&conversation.conversation_id),
                conversation.thread_id.as_deref(),
            )
            .await?
    } else {
        Vec::new()
    };

    if bindings.len() > 1 || (bindings.is_empty() && conversation.is_some_and(|c| !c.top_level)) {
        materialize_ambiguity(
            state,
            event,
            "External conversation correlation is ambiguous or unbound.",
        )
        .await?;
        repository
            .complete_routing(
                &event.id,
                "needs_attention",
                None,
                Some("correlation_ambiguous"),
            )
            .await?;
        return Ok(());
    }

    if let Some(binding) = bindings.first() {
        if let Some(command) = event.normalized.command.as_ref() {
            execute_bound_command(
                state,
                repository,
                event,
                binding.task_id.as_str(),
                trigger_name,
                trigger,
                command,
            )
            .await?;
        } else {
            append_source_context(state, event, &binding.task_id);
        }
        repository
            .complete_routing(&event.id, "routed", Some(&binding.task_id), None)
            .await?;
        return Ok(());
    }

    let task_id = deterministic_task_id(&event.id);
    let goal = event
        .normalized
        .text_summary
        .clone()
        .unwrap_or_else(|| format!("External {} source event", event.provider));
    let created = fire_trigger_canonical_with_context(
        state,
        trigger_name,
        &event.project_id,
        trigger,
        None,
        TriggerFireContext {
            requested_task_id: Some(task_id),
            parent_task_id: None,
            source_event_id: Some(event.id.clone()),
            goal: Some(goal),
            initial_vars: Some(source_initial_vars(event)),
        },
    )
    .await?;
    if let Some(conversation) = conversation {
        repository
            .create_binding(CreateSourceBinding {
                project_id: event.project_id.clone(),
                task_id: created.clone(),
                provider: event.provider.clone(),
                installation_id: event.installation_id.clone(),
                conversation_id: Some(conversation.conversation_id.clone()),
                thread_id: conversation.thread_id.clone(),
                binding_type: "primary".to_string(),
                created_by_event_id: event.id.clone(),
            })
            .await?;
    }
    append_source_context(state, event, &created);
    repository
        .complete_routing(&event.id, "routed", Some(&created), None)
        .await?;
    tracing::info!(
        provider = %event.provider,
        installation_hash = %short_hash(&event.installation_id),
        external_event_hash = %short_hash(&event.external_event_id),
        task_id = %created,
        routing_state = "routed",
        "source event routed"
    );
    Ok(())
}

async fn route_reaction_automation(
    state: &Arc<InnerState>,
    source_repository: &AsyncSourceRepository,
    event: &SourceEventRecord,
) -> Result<()> {
    let route_repository = AsyncSourceAutomationRepository::new(state.async_database.clone());
    if let Some(existing) = route_repository.get_for_event(&event.id).await? {
        let reservation = route_repository.claim_existing(&existing.id).await?;
        if reservation.route.status == "completed" {
            source_repository
                .complete_routing(
                    &event.id,
                    "routed",
                    reservation.route.task_id.as_deref(),
                    None,
                )
                .await?;
            return Ok(());
        }
        if !reservation.should_execute {
            source_repository
                .complete_routing(
                    &event.id,
                    "failed",
                    None,
                    Some("automation_route_in_progress"),
                )
                .await?;
            return Ok(());
        }
        return execute_reserved_automation_route(
            state,
            source_repository,
            &route_repository,
            event,
            reservation.route,
        )
        .await;
    }

    let Some(reaction) = event.normalized.reaction.as_ref() else {
        source_repository
            .complete_routing(
                &event.id,
                "ignored",
                None,
                Some("reaction_metadata_missing"),
            )
            .await?;
        return Ok(());
    };
    if reaction.target.kind != "message" {
        source_repository
            .complete_routing(
                &event.id,
                "ignored",
                None,
                Some("unsupported_reaction_target"),
            )
            .await?;
        return Ok(());
    }
    let Some(conversation) = event.normalized.conversation.as_ref() else {
        source_repository
            .complete_routing(
                &event.id,
                "ignored",
                None,
                Some("reaction_message_identity_invalid"),
            )
            .await?;
        return Ok(());
    };
    let channel_id = conversation.conversation_id.clone();
    let Some(message_ts) = conversation.thread_id.clone() else {
        source_repository
            .complete_routing(
                &event.id,
                "ignored",
                None,
                Some("reaction_message_identity_invalid"),
            )
            .await?;
        return Ok(());
    };
    let message_identity = format!("{channel_id}:{message_ts}");
    if reaction.target.external_id != message_identity {
        source_repository
            .complete_routing(
                &event.id,
                "ignored",
                None,
                Some("reaction_message_identity_invalid"),
            )
            .await?;
        return Ok(());
    }
    let active = read_active_config(state)?;
    let match_result = agent_orchestrator::source_task_binding::match_source_task_binding(
        &active.config,
        &event.project_id,
        &agent_orchestrator::source_task_binding::SourceTaskBindingMatchInput {
            provider: event.provider.clone(),
            installation_id: event.installation_id.clone(),
            event_kind: "reaction_added".to_string(),
            reaction: reaction.name.clone(),
            target_kind: reaction.target.kind.clone(),
            channel_id: channel_id.clone(),
            external_actor_id: event.external_actor_id.clone().unwrap_or_default(),
        },
    )?;
    if match_result.status != "matched" {
        let reason = if match_result.reason == "reaction_automation_disabled" {
            "reaction_routing_not_enabled"
        } else {
            match_result.reason.as_str()
        };
        source_repository
            .complete_routing(&event.id, "ignored", None, Some(reason))
            .await?;
        return Ok(());
    }

    let project = active
        .config
        .projects
        .get(&event.project_id)
        .context("source automation project missing")?;
    let binding_name = match_result
        .binding_id
        .as_deref()
        .context("matched binding name missing")?;
    let binding = project
        .source_task_bindings
        .get(binding_name)
        .context("matched binding snapshot missing")?;
    let template_name = match_result
        .template_ref
        .as_deref()
        .context("matched template name missing")?;
    let template = project
        .source_task_templates
        .get(template_name)
        .context("matched template snapshot missing")?;
    let trigger_name = match_result
        .trigger_name
        .as_deref()
        .context("matched trigger name missing")?;
    let credential = project
        .triggers
        .get(trigger_name)
        .and_then(|trigger| trigger.event.as_ref())
        .and_then(|trigger_event| trigger_event.webhook.as_ref())
        .and_then(|webhook| webhook.outbound_credential.as_ref())
        .context("Slack outbound credential reference missing")?;
    let reservation = route_repository
        .reserve(ReserveSourceAutomationRoute {
            project_id: event.project_id.clone(),
            source_event_id: event.id.clone(),
            provider: event.provider.clone(),
            installation_id: event.installation_id.clone(),
            message_identity,
            channel_id,
            message_ts,
            reaction: reaction.name.clone(),
            resolved_role: match_result
                .resolved_role
                .context("matched actor role missing")?,
            binding_name: binding_name.to_string(),
            binding_revision: match_result
                .binding_revision
                .context("matched binding revision missing")?,
            template_name: template_name.to_string(),
            template_hash: template_content_hash(template)?,
            binding_snapshot: serde_json::to_value(binding)?,
            template_snapshot: serde_json::to_value(template)?,
            credential_store: credential.from_ref.clone(),
            credential_key: credential.key.clone(),
        })
        .await?;
    if reservation.route.status == "completed" {
        source_repository
            .complete_routing(
                &event.id,
                "routed",
                reservation.route.task_id.as_deref(),
                None,
            )
            .await?;
        return Ok(());
    }
    if !reservation.should_execute {
        source_repository
            .complete_routing(
                &event.id,
                "failed",
                None,
                Some("automation_route_in_progress"),
            )
            .await?;
        return Ok(());
    }
    execute_reserved_automation_route(
        state,
        source_repository,
        &route_repository,
        event,
        reservation.route,
    )
    .await
}

async fn execute_reserved_automation_route(
    state: &Arc<InnerState>,
    source_repository: &AsyncSourceRepository,
    route_repository: &AsyncSourceAutomationRepository,
    event: &SourceEventRecord,
    route: SourceAutomationRoute,
) -> Result<()> {
    let route_id = route.id.clone();
    match execute_automation_route(state, source_repository, route_repository, event, route).await {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(
                source_event_id = %event.id,
                automation_route_id = %route_id,
                error = %error,
                "source automation execution failed"
            );
            let _ = route_repository
                .fail(&route_id, "automation_task_route_failed", None)
                .await;
            source_repository
                .complete_routing(
                    &event.id,
                    "failed",
                    None,
                    Some("automation_task_route_failed"),
                )
                .await
        }
    }
}

async fn execute_automation_route(
    state: &Arc<InnerState>,
    source_repository: &AsyncSourceRepository,
    route_repository: &AsyncSourceAutomationRepository,
    event: &SourceEventRecord,
    route: SourceAutomationRoute,
) -> Result<()> {
    let snapshot = route_repository
        .execution_snapshot(&route.id)
        .await?
        .context("automation execution snapshot missing")?;
    let fresh = read_active_config(state)?;
    let token = fresh
        .config
        .projects
        .get(&route.project_id)
        .and_then(|project| project.secret_stores.get(&snapshot.credential_store))
        .and_then(|store| store.data.get(&snapshot.credential_key))
        .cloned();
    let Some(token) = token else {
        return fail_automation_route(
            route_repository,
            source_repository,
            event,
            &route.id,
            "slack_credential_missing",
            None,
        )
        .await;
    };
    let permalink = if route.permalink_status == "resolved" {
        route
            .permalink
            .clone()
            .context("resolved automation route has no permalink")?
    } else {
        let client = match SlackApiClient::new() {
            Ok(client) => client,
            Err(error) => {
                return fail_automation_route(
                    route_repository,
                    source_repository,
                    event,
                    &route.id,
                    error.code(),
                    None,
                )
                .await;
            }
        };
        match client
            .get_permalink(&token, &route.channel_id, &route.message_ts)
            .await
        {
            Ok(permalink) => {
                route_repository
                    .record_permalink(&route.id, &permalink.url)
                    .await?;
                permalink.url
            }
            Err(error) => {
                let retry_after = error.retry_after().map(|value| value.as_secs().to_string());
                let _is_transient = error.is_transient();
                return fail_automation_route(
                    route_repository,
                    source_repository,
                    event,
                    &route.id,
                    error.code(),
                    retry_after.as_deref(),
                )
                .await;
            }
        }
    };
    let template: agent_orchestrator::config::SourceTaskTemplateConfig =
        serde_json::from_value(snapshot.template).context("frozen source template is invalid")?;
    let source_message_url = permalink.clone();
    let rendered = match render_source_task_template(
        &template,
        &SourceTaskTemplateRenderInput {
            provider: route.provider.clone(),
            installation_id: route.installation_id.clone(),
            message_url: source_message_url.clone(),
            event_id: Some(route.source_event_id.clone()),
            reaction: Some(route.reaction.clone()),
            target_id: Some(route.message_identity.clone()),
            installation_verified: true,
        },
    ) {
        Ok(rendered) => rendered,
        Err(_) => {
            return fail_automation_route(
                route_repository,
                source_repository,
                event,
                &route.id,
                "source_template_render_failed",
                None,
            )
            .await;
        }
    };

    let actor = event
        .external_actor_id
        .as_ref()
        .map(|actor| format!("{}:{}:{actor}", route.provider, route.installation_id));
    let audit_repository = AsyncActionAuditRepository::new(state.async_database.clone());
    let audit = audit_repository
        .reserve(ActionAuditReservation {
            request_id: route.request_id.clone(),
            project_id: route.project_id.clone(),
            actor,
            resolved_role: Some(route.resolved_role.clone()),
            transport: "slack_adapter".to_string(),
            target_type: "source_automation_route".to_string(),
            target_id: route.id.clone(),
            action: "source.automation.create_task".to_string(),
            reason_code: "matched_source_task_binding".to_string(),
            operator_reason: None,
            idempotency_key: Some(route.automation_key.clone()),
            expected_version: Some(route.binding_revision.clone()),
            fencing_token: None,
            canonical_request: serde_json::json!({
                "route_id": &route.id,
                "source_event_id": &route.source_event_id,
                "binding_name": &route.binding_name,
                "binding_revision": &route.binding_revision,
                "template_name": &route.template_name,
                "template_hash": &route.template_hash,
                "task_id": &route.deterministic_task_id,
            }),
        })
        .await?;
    if audit.record.status == "succeeded" {
        let task_id = audit
            .record
            .result_id
            .context("succeeded automation audit has no task result")?;
        route_repository.complete(&route.id, &task_id).await?;
        source_repository
            .complete_routing(&event.id, "routed", Some(&task_id), None)
            .await?;
        return Ok(());
    }
    if audit.record.status != "reserved" {
        return fail_automation_route(
            route_repository,
            source_repository,
            event,
            &route.id,
            "automation_audit_not_executable",
            None,
        )
        .await;
    }

    let mut initial_vars = rendered
        .action
        .initial_vars
        .clone()
        .into_iter()
        .collect::<HashMap<_, _>>();
    initial_vars.insert("source_event_id".to_string(), route.source_event_id.clone());
    initial_vars.insert("source_provider".to_string(), route.provider.clone());
    initial_vars.insert("source_reaction".to_string(), route.reaction.clone());
    initial_vars.insert("source_message_url".to_string(), source_message_url);
    initial_vars.insert("source_template".to_string(), route.template_name.clone());
    initial_vars.insert("source_binding".to_string(), route.binding_name.clone());
    let creation = agent_orchestrator::task_ops::create_task_impl_with_id_outcome(
        state,
        agent_orchestrator::dto::CreateTaskPayload {
            name: Some(format!("source-{}-{}", route.reaction, route.binding_name)),
            goal: Some(rendered.goal.clone()),
            project_id: Some(route.project_id.clone()),
            workspace_id: Some(rendered.action.workspace.clone()),
            workflow_id: Some(rendered.action.workflow.clone()),
            target_files: None,
            parent_task_id: None,
            spawn_reason: Some("source_automation".to_string()),
            step_filter: None,
            initial_vars: Some(initial_vars),
        },
        Some(&route.deterministic_task_id),
    )?;
    let task_id = creation.task.id.clone();
    if rendered.action.start && creation.task.status == "created" {
        state.task_enqueuer.enqueue_task(state, &task_id).await?;
        state.worker_notify.notify_one();
    }
    source_repository
        .create_binding(CreateSourceBinding {
            project_id: route.project_id.clone(),
            task_id: task_id.clone(),
            provider: route.provider.clone(),
            installation_id: route.installation_id.clone(),
            conversation_id: Some(route.channel_id.clone()),
            thread_id: Some(route.message_ts.clone()),
            binding_type: "automation".to_string(),
            created_by_event_id: route.source_event_id.clone(),
        })
        .await?;
    state.emit_event(
        &task_id,
        None,
        "source_automation_routed",
        serde_json::json!({
            "source_event_id": &route.source_event_id,
            "automation_route_id": &route.id,
            "request_id": &route.request_id,
            "reaction": &route.reaction,
            "binding_name": &route.binding_name,
            "binding_revision": &route.binding_revision,
            "template_name": &route.template_name,
            "template_hash": &route.template_hash,
            "message": format!("Slack badge '{}' routed with template '{}'", route.reaction, route.template_name),
            "status": "routed",
        }),
    );
    audit_repository
        .complete(
            &route.request_id,
            "succeeded",
            None,
            Some("task"),
            Some(&task_id),
        )
        .await?;
    route_repository.complete(&route.id, &task_id).await?;
    source_repository
        .complete_routing(&event.id, "routed", Some(&task_id), None)
        .await?;
    Ok(())
}

async fn fail_automation_route(
    route_repository: &AsyncSourceAutomationRepository,
    source_repository: &AsyncSourceRepository,
    event: &SourceEventRecord,
    route_id: &str,
    error_code: &str,
    retry_after: Option<&str>,
) -> Result<()> {
    route_repository
        .fail(route_id, error_code, retry_after)
        .await?;
    source_repository
        .complete_routing(&event.id, "failed", None, Some(error_code))
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_bound_command(
    state: &Arc<InnerState>,
    repository: &AsyncSourceRepository,
    event: &SourceEventRecord,
    task_id: &str,
    trigger_name: &str,
    trigger: &agent_orchestrator::config::TriggerConfig,
    command: &SourceCommand,
) -> Result<()> {
    let webhook = trigger
        .event
        .as_ref()
        .and_then(|value| value.webhook.as_ref())
        .context("source trigger webhook config missing")?;
    let actor_id = event
        .external_actor_id
        .as_deref()
        .context("source command actor missing")?;
    let role = webhook
        .actor_roles
        .get(actor_id)
        .map(String::as_str)
        .unwrap_or("read_only");
    let actor = format!("{}:{}:{}", event.provider, event.installation_id, actor_id);
    let (target_type, target_id, action) = command_audit_target(command, task_id);
    let idempotency_key = format!("source-command:{}", event.id);
    let canonical_request = serde_json::to_value(command)?;
    let request_hash = short_hash(&serde_json::to_string(&canonical_request)?);
    let request_id = format!("req-source-{}", short_hash(&event.id));
    let common_audit = AsyncActionAuditRepository::new(state.async_database.clone());
    let reservation = common_audit
        .reserve(ActionAuditReservation {
            request_id: request_id.clone(),
            project_id: event.project_id.clone(),
            actor: Some(actor.clone()),
            resolved_role: Some(role.to_string()),
            transport: format!("{}_adapter", event.provider),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            action: format!("source.command.{action}"),
            reason_code: "provider_command".to_string(),
            operator_reason: None,
            idempotency_key: Some(idempotency_key.clone()),
            expected_version: command_expected_version(command),
            fencing_token: None,
            canonical_request,
        })
        .await?;
    if !reservation.should_execute {
        return Ok(());
    }
    let reserved = repository
        .begin_command_action(SourceCommandActionInput {
            request_id: request_id.clone(),
            source_event_id: event.id.clone(),
            actor: actor.clone(),
            resolved_role: role.to_string(),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            action: action.to_string(),
            idempotency_key: idempotency_key.clone(),
            request_hash,
        })
        .await?;
    if !reserved {
        common_audit
            .complete(&request_id, "succeeded", None, Some("task"), Some(task_id))
            .await?;
        return Ok(());
    }
    if !matches!(role, "operator" | "admin") && !matches!(command, SourceCommand::OpenConsole) {
        repository
            .complete_command_action(
                &event.id,
                &idempotency_key,
                "failed",
                None,
                Some("actor_not_authorized"),
            )
            .await?;
        common_audit
            .complete(
                &request_id,
                "denied",
                Some("authorization_denied"),
                None,
                None,
            )
            .await?;
        bail!("source actor is not authorized for privileged command");
    }

    let command_result: Result<()> = async {
        match command {
            SourceCommand::Approve {
                attention_item_id,
                expected_version,
            } => {
                execute_attention_command(
                    state,
                    event,
                    &actor,
                    attention_item_id,
                    *expected_version,
                    "approve_decision",
                )
                .await?;
            }
            SourceCommand::Reject {
                attention_item_id,
                expected_version,
            } => {
                execute_attention_command(
                    state,
                    event,
                    &actor,
                    attention_item_id,
                    *expected_version,
                    "reject_decision",
                )
                .await?;
            }
            SourceCommand::Retry {
                attention_item_id,
                expected_version,
            } => {
                execute_attention_command(
                    state,
                    event,
                    &actor,
                    attention_item_id,
                    *expected_version,
                    "retry_failed_item",
                )
                .await?;
            }
            SourceCommand::AddContext => append_source_context(state, event, task_id),
            SourceCommand::Cancel => {
                orchestrator_scheduler::service::task::pause_task(state.clone(), task_id)
                    .await
                    .map_err(anyhow::Error::from)?;
                state.emit_event(
                    task_id,
                    None,
                    "source_cancelled",
                    serde_json::json!({
                        "source_event_id": event.id,
                        "request_id": request_id,
                        "actor_id": actor,
                        "status": "paused",
                    }),
                );
            }
            SourceCommand::Branch => {
                let child_id = deterministic_task_id(&event.id);
                let child = fire_trigger_canonical_with_context(
                    state,
                    trigger_name,
                    &event.project_id,
                    trigger,
                    None,
                    TriggerFireContext {
                        requested_task_id: Some(child_id),
                        parent_task_id: Some(task_id.to_string()),
                        source_event_id: Some(event.id.clone()),
                        goal: event.normalized.text_summary.clone(),
                        initial_vars: Some(source_initial_vars(event)),
                    },
                )
                .await?;
                if let Some(conversation) = event.normalized.conversation.as_ref() {
                    repository
                        .create_binding(CreateSourceBinding {
                            project_id: event.project_id.clone(),
                            task_id: child.clone(),
                            provider: event.provider.clone(),
                            installation_id: event.installation_id.clone(),
                            conversation_id: Some(conversation.conversation_id.clone()),
                            thread_id: conversation.thread_id.clone(),
                            binding_type: "related".to_string(),
                            created_by_event_id: event.id.clone(),
                        })
                        .await?;
                }
                state.emit_event(
                    task_id,
                    None,
                    "source_branch_created",
                    serde_json::json!({
                        "source_event_id": event.id,
                        "request_id": request_id,
                        "child_task_id": child,
                        "actor_id": actor,
                    }),
                );
            }
            SourceCommand::OpenConsole => {
                state.emit_event(
                    task_id,
                    None,
                    "source_open_console",
                    serde_json::json!({
                        "source_event_id": event.id,
                        "request_id": request_id,
                        "actor_id": actor,
                        "deep_link": format!("orchestrator://tasks/{task_id}"),
                    }),
                );
            }
        }
        Ok(())
    }
    .await;
    match command_result {
        Ok(()) => {
            repository
                .complete_command_action(
                    &event.id,
                    &idempotency_key,
                    "succeeded",
                    Some(&serde_json::json!({"task_id": task_id})),
                    None,
                )
                .await?;
            common_audit
                .complete(&request_id, "succeeded", None, Some("task"), Some(task_id))
                .await?;
            Ok(())
        }
        Err(error) => {
            repository
                .complete_command_action(
                    &event.id,
                    &idempotency_key,
                    "failed",
                    None,
                    Some(stable_error_code(&error)),
                )
                .await?;
            common_audit
                .complete(
                    &request_id,
                    "failed",
                    Some(stable_error_code(&error)),
                    None,
                    None,
                )
                .await?;
            Err(error)
        }
    }
}

fn command_expected_version(command: &SourceCommand) -> Option<String> {
    match command {
        SourceCommand::Approve {
            expected_version, ..
        }
        | SourceCommand::Reject {
            expected_version, ..
        }
        | SourceCommand::Retry {
            expected_version, ..
        } => Some(expected_version.to_string()),
        _ => None,
    }
}

fn command_audit_target<'a>(
    command: &'a SourceCommand,
    task_id: &'a str,
) -> (&'static str, &'a str, &'static str) {
    match command {
        SourceCommand::Approve {
            attention_item_id, ..
        } => ("attention_item", attention_item_id, "approve_decision"),
        SourceCommand::Reject {
            attention_item_id, ..
        } => ("attention_item", attention_item_id, "reject_decision"),
        SourceCommand::Retry {
            attention_item_id, ..
        } => ("attention_item", attention_item_id, "retry_failed_item"),
        SourceCommand::AddContext => ("task", task_id, "add_context"),
        SourceCommand::Cancel => ("task", task_id, "cancel"),
        SourceCommand::Branch => ("task", task_id, "branch"),
        SourceCommand::OpenConsole => ("task", task_id, "open_console"),
    }
}

async fn execute_attention_command(
    state: &Arc<InnerState>,
    event: &SourceEventRecord,
    actor: &str,
    attention_item_id: &str,
    expected_version: i64,
    action_id: &str,
) -> Result<()> {
    orchestrator_scheduler::service::attention::execute_allowlisted_action(
        state,
        attention_item_id,
        expected_version,
        &format!("source:{}:{action_id}", event.id),
        actor,
        action_id,
        &serde_json::json!({}),
    )
    .await?;
    Ok(())
}

fn append_source_context(state: &InnerState, event: &SourceEventRecord, task_id: &str) {
    state.emit_event(
        task_id,
        None,
        "source_context_added",
        serde_json::json!({
            "source_event_id": event.id,
            "provider": event.provider,
            "message": event.normalized.text_summary,
            "status": "accepted",
        }),
    );
}

async fn materialize_ambiguity(
    state: &InnerState,
    event: &SourceEventRecord,
    summary: &str,
) -> Result<()> {
    let digest = short_hash(&format!("source-ambiguity:{}", event.id));
    state
        .attention_repo
        .upsert_external_candidate(AttentionCandidate {
            id: format!("attn-source-{digest}"),
            project_id: event.project_id.clone(),
            task_id: String::new(),
            task_item_id: None,
            step_id: None,
            session_id: None,
            kind: "source_routing_ambiguous".to_string(),
            severity: AttentionSeverity::Intervention,
            title: "External source needs routing".to_string(),
            summary: summary.to_string(),
            requested_decision: Some(serde_json::json!({
                "type": "object",
                "properties": {"task_id": {"type": "string"}},
                "required": ["task_id"],
                "additionalProperties": false
            })),
            actions: vec![AttentionActionDescriptor {
                id: "acknowledge".to_string(),
                label: "Acknowledge".to_string(),
                required_role: "operator".to_string(),
                confirmation: "none".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "additionalProperties": false
                }),
            }],
            dedupe_key: format!("source-routing:{}", event.id),
            source_event_id: event.id.clone(),
            occurred_at: event.received_at.clone(),
            sla_deadline: None,
        })
        .await?;
    Ok(())
}

fn source_initial_vars(event: &SourceEventRecord) -> HashMap<String, String> {
    HashMap::from([
        ("source_event_id".to_string(), event.id.clone()),
        ("source_provider".to_string(), event.provider.clone()),
        (
            "source_installation_id".to_string(),
            event.installation_id.clone(),
        ),
    ])
}

fn short_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn stable_error_code(error: &anyhow::Error) -> &'static str {
    let message = error.to_string();
    if message.contains("not authorized") {
        "actor_not_authorized"
    } else if message.contains("trigger") {
        "trigger_failed"
    } else if message.contains("attention") {
        "attention_action_failed"
    } else {
        "routing_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::config::{
        SecretStoreConfig, SourceTaskBindingConfig, SourceTaskBindingMatchConfig,
        SourceTaskTemplateActionConfig, SourceTaskTemplateConfig, SourceTaskTemplateSkillConfig,
        TriggerActionConfig, TriggerConfig, TriggerEventConfig, TriggerOutboundCredentialRef,
        TriggerWebhookConfig,
    };
    use agent_orchestrator::dto::CreateTaskPayload;
    use agent_orchestrator::source::{
        ConversationRef, ExternalActorRef, ExternalArtifactRef, IngestSourceEvent,
        NormalizedSourceEvent, SourceEventKind, SourceReactionRef,
    };
    use agent_orchestrator::state::update_config_runtime;
    use agent_orchestrator::test_utils::TestState;

    fn source_trigger() -> TriggerConfig {
        TriggerConfig {
            cron: None,
            event: Some(TriggerEventConfig {
                source: "webhook".into(),
                filter: None,
                webhook: Some(TriggerWebhookConfig {
                    secret: None,
                    outbound_credential: None,
                    signature_header: None,
                    crd_ref: None,
                    provider: Some("fixture".into()),
                    installation_id: Some("install-1".into()),
                    actor_roles: HashMap::from([("operator-1".into(), "operator".into())]),
                    reaction_routing: "disabled".into(),
                    timestamp_tolerance_secs: 300,
                }),
                filesystem: None,
            }),
            action: TriggerActionConfig {
                workflow: "basic".into(),
                workspace: "default".into(),
                args: None,
                start: false,
            },
            concurrency_policy: agent_orchestrator::cli_types::ConcurrencyPolicy::Allow,
            suspend: false,
            history_limit: None,
            throttle: None,
        }
    }

    fn event(id: &str, top_level: bool) -> IngestSourceEvent {
        IngestSourceEvent {
            project_id: "default".into(),
            event: NormalizedSourceEvent {
                provider: "fixture".into(),
                installation_id: "install-1".into(),
                external_event_id: id.into(),
                kind: SourceEventKind::Message,
                reaction: None,
                actor: ExternalActorRef {
                    external_id: "actor-1".into(),
                    display_name: None,
                },
                conversation: Some(ConversationRef {
                    conversation_id: "conversation-1".into(),
                    thread_id: Some("thread-1".into()),
                    top_level,
                }),
                text_summary: Some(format!("context {id}")),
                command: None,
                attachments: Vec::new(),
                occurred_at: "2026-07-14T00:00:00Z".into(),
            },
            payload_hash: format!("hash-{id}"),
            raw_payload_ref: None,
        }
    }

    fn command_event(id: &str, actor_id: &str, command: SourceCommand) -> IngestSourceEvent {
        let mut input = event(id, false);
        input.event.kind = SourceEventKind::Command;
        input.event.actor.external_id = actor_id.to_string();
        input.event.command = Some(command);
        input
    }

    fn reaction_event(id: &str, target_kind: &str) -> IngestSourceEvent {
        let mut input = event(id, false);
        input.event.kind = SourceEventKind::ReactionAdded;
        input.event.reaction = Some(SourceReactionRef {
            name: "agent_docs".into(),
            target: ExternalArtifactRef {
                kind: target_kind.into(),
                external_id: "conversation-1:thread-1".into(),
                url: None,
            },
        });
        input.event.text_summary = None;
        input
    }

    fn state_with_trigger() -> (TestState, Arc<InnerState>) {
        let mut fixture = TestState::new();
        let state = fixture.build();
        std::fs::write(
            fixture
                .temp_root()
                .join("workspace/default/docs/qa/QA-source-fixture.md"),
            "# Source fixture\n",
        )
        .expect("write QA fixture");
        update_config_runtime(&state, |current| {
            let mut next = current.clone();
            Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .get_mut("default")
                .expect("default project")
                .triggers
                .insert("source-trigger".into(), source_trigger());
            (next, ())
        });
        (fixture, state)
    }

    #[tokio::test]
    async fn duplicate_top_level_event_creates_one_task_and_binding() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let first = repository
            .ingest(event("event-1", true))
            .await
            .expect("ingest");
        let duplicate = repository
            .ingest(event("event-1", true))
            .await
            .expect("dedupe");
        assert!(first.inserted);
        assert!(!duplicate.inserted);
        let claimed = repository.claim_pending(10).await.expect("claim");
        route_one(&state, &repository, &claimed[0])
            .await
            .expect("route");
        reconcile_source_once(&state).await.expect("empty replay");
        let routed = repository
            .get(&first.event.id)
            .await
            .expect("get")
            .expect("event");
        assert_eq!(routed.routing_state, "routed");
        let task_id = routed.routed_task_id.expect("task");
        assert_eq!(
            repository
                .list_bindings(&task_id)
                .await
                .expect("bindings")
                .len(),
            1
        );
        let count = state
            .async_database
            .reader()
            .call(|conn| {
                Ok(conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))?)
            })
            .await
            .expect("task count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn bound_thread_message_routes_to_existing_task() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let root = repository
            .ingest(event("event-1", true))
            .await
            .expect("root");
        let claimed = repository.claim_pending(10).await.expect("claim root");
        route_one(&state, &repository, &claimed[0])
            .await
            .expect("route root");
        let root_task = repository
            .get(&root.event.id)
            .await
            .expect("get root")
            .expect("root event")
            .routed_task_id
            .expect("root task");
        let reply = repository
            .ingest(event("event-2", false))
            .await
            .expect("reply");
        let claimed = repository.claim_pending(10).await.expect("claim reply");
        route_one(&state, &repository, &claimed[0])
            .await
            .expect("route reply");
        let reply_task = repository
            .get(&reply.event.id)
            .await
            .expect("get reply")
            .expect("reply event")
            .routed_task_id
            .expect("reply task");
        assert_eq!(root_task, reply_task);
    }

    #[tokio::test]
    async fn reaction_is_ignored_without_task_binding_or_duplicate_attempt() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let first = repository
            .ingest(reaction_event("reaction-1", "message"))
            .await
            .expect("reaction");
        let duplicate = repository
            .ingest(reaction_event("reaction-1", "message"))
            .await
            .expect("duplicate");
        assert!(first.inserted);
        assert!(!duplicate.inserted);
        reconcile_source_once(&state).await.expect("route reaction");
        reconcile_source_once(&state).await.expect("empty replay");

        let routed = repository
            .get(&first.event.id)
            .await
            .expect("get")
            .expect("event");
        assert_eq!(routed.routing_state, "ignored");
        assert_eq!(
            routed.last_error_code.as_deref(),
            Some("reaction_routing_not_enabled")
        );
        assert_eq!(routed.routing_attempts, 1);
        assert!(routed.routed_task_id.is_none());
        let (tasks, bindings) = state
            .async_database
            .reader()
            .call(|conn| {
                Ok((
                    conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))?,
                    conn.query_row("SELECT COUNT(*) FROM source_bindings", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                ))
            })
            .await
            .expect("counts");
        assert_eq!((tasks, bindings), (0, 0));
    }

    #[tokio::test]
    async fn matched_reaction_with_missing_template_fails_closed_without_task() {
        let (_fixture, state) = state_with_trigger();
        update_config_runtime(&state, |current| {
            let mut next = current.clone();
            let project = Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .get_mut("default")
                .expect("default project");
            let webhook = project
                .triggers
                .get_mut("source-trigger")
                .and_then(|trigger| trigger.event.as_mut())
                .and_then(|event| event.webhook.as_mut())
                .expect("webhook");
            webhook.reaction_routing = "bindings".into();
            webhook
                .actor_roles
                .insert("actor-1".into(), "operator".into());
            project.source_task_bindings.insert(
                "docs".into(),
                SourceTaskBindingConfig {
                    trigger_ref: "source-trigger".into(),
                    match_rule: SourceTaskBindingMatchConfig {
                        event_kind: "reaction_added".into(),
                        reaction: "agent_docs".into(),
                        target_kind: "message".into(),
                        channels: vec!["conversation-1".into()],
                        all_channels: false,
                    },
                    template_ref: "docs-template".into(),
                    allowed_actor_roles: vec!["operator".into()],
                    suspend: false,
                },
            );
            (next, ())
        });
        let active = read_active_config(&state).expect("active config");
        let simulated = agent_orchestrator::source_task_binding::match_source_task_binding(
            &active.config,
            "default",
            &agent_orchestrator::source_task_binding::SourceTaskBindingMatchInput {
                provider: "fixture".into(),
                installation_id: "install-1".into(),
                event_kind: "reaction_added".into(),
                reaction: "agent_docs".into(),
                target_kind: "message".into(),
                channel_id: "conversation-1".into(),
                external_actor_id: "actor-1".into(),
            },
        )
        .expect("simulate");
        assert_eq!(simulated.status, "matched");

        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let reaction = repository
            .ingest(reaction_event("reaction-enabled", "message"))
            .await
            .expect("reaction");
        reconcile_source_once(&state).await.expect("route reaction");
        let routed = repository
            .get(&reaction.event.id)
            .await
            .expect("get")
            .expect("event");
        assert_eq!(routed.routing_state, "failed");
        assert_eq!(routed.last_error_code.as_deref(), Some("routing_failed"));
        assert!(routed.routed_task_id.is_none());
    }

    #[tokio::test]
    async fn matched_slack_badge_creates_one_canonical_task_for_duplicate_deliveries() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fake Slack bind");
        let address = listener.local_addr().expect("fake Slack address");
        tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route(
                    "/api/chat.getPermalink",
                    axum::routing::get(|axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>| async move {
                        if query.get("channel").map(String::as_str) == Some("conversation-1")
                            && query.get("message_ts").map(String::as_str) == Some("thread-1")
                        {
                            axum::Json(serde_json::json!({
                                "ok": true,
                                "permalink": "https://acme.slack.com/archives/conversation-1/p171234000100"
                            }))
                        } else {
                            axum::Json(serde_json::json!({
                                "ok": false,
                                "error": "message_not_found"
                            }))
                        }
                    }),
                ),
            )
            .await
            .expect("fake Slack server");
        });
        crate::slack_api::set_test_api_base(Some(format!("http://{address}/api/")));

        let (_fixture, state) = state_with_trigger();
        update_config_runtime(&state, |current| {
            let mut next = current.clone();
            let project = Arc::make_mut(&mut next.active_config)
                .config
                .projects
                .get_mut("default")
                .expect("default project");
            let webhook = project
                .triggers
                .get_mut("source-trigger")
                .and_then(|trigger| trigger.event.as_mut())
                .and_then(|event| event.webhook.as_mut())
                .expect("webhook");
            webhook.provider = Some("slack".into());
            webhook.installation_id = Some("T_TEST".into());
            webhook.reaction_routing = "bindings".into();
            webhook.outbound_credential = Some(TriggerOutboundCredentialRef {
                from_ref: "slack-api".into(),
                key: "BOT_TOKEN".into(),
            });
            webhook
                .actor_roles
                .insert("actor-1".into(), "operator".into());
            project.secret_stores.insert(
                "slack-api".into(),
                SecretStoreConfig {
                    data: HashMap::from([("BOT_TOKEN".into(), "xoxb-never-persist".into())]),
                },
            );
            project.source_task_templates.insert(
                "docs-template".into(),
                SourceTaskTemplateConfig {
                    skill: SourceTaskTemplateSkillConfig {
                        name: "docs".into(),
                        invocation: "$docs".into(),
                        args: vec![],
                    },
                    action: SourceTaskTemplateActionConfig {
                        workflow: "basic".into(),
                        workspace: "default".into(),
                        start: false,
                        initial_vars: Default::default(),
                    },
                    goal_template: "{skill_invocation} {source_message_url}".into(),
                    allowed_variables: vec!["skill_invocation".into(), "source_message_url".into()],
                },
            );
            project.source_task_bindings.insert(
                "docs".into(),
                SourceTaskBindingConfig {
                    trigger_ref: "source-trigger".into(),
                    match_rule: SourceTaskBindingMatchConfig {
                        event_kind: "reaction_added".into(),
                        reaction: "agent_docs".into(),
                        target_kind: "message".into(),
                        channels: vec!["conversation-1".into()],
                        all_channels: false,
                    },
                    template_ref: "docs-template".into(),
                    allowed_actor_roles: vec!["operator".into()],
                    suspend: false,
                },
            );
            (next, ())
        });
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let mut first_input = reaction_event("reaction-delivery-1", "message");
        first_input.event.provider = "slack".into();
        first_input.event.installation_id = "T_TEST".into();
        let mut second_input = first_input.clone();
        second_input.event.external_event_id = "reaction-delivery-2".into();
        second_input.payload_hash = "hash-reaction-delivery-2".into();
        let first = repository
            .ingest(first_input)
            .await
            .expect("first reaction");
        let second = repository
            .ingest(second_input)
            .await
            .expect("second reaction");

        reconcile_source_once(&state)
            .await
            .expect("route reactions");
        crate::slack_api::set_test_api_base(None);

        let first = repository
            .get(&first.event.id)
            .await
            .expect("first read")
            .expect("first event");
        let second = repository
            .get(&second.event.id)
            .await
            .expect("second read")
            .expect("second event");
        assert_eq!(first.routing_state, "routed");
        assert_eq!(first.routed_task_id, second.routed_task_id);
        let task_id = first.routed_task_id.expect("automation task");
        let (tasks, routes, automation_bindings, audits, goal, leaked) = state
            .async_database
            .reader()
            .call(move |conn| {
                Ok((
                    conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0))?,
                    conn.query_row("SELECT COUNT(*) FROM source_automation_routes", [], |row| row.get::<_, i64>(0))?,
                    conn.query_row("SELECT COUNT(*) FROM source_bindings WHERE binding_type='automation'", [], |row| row.get::<_, i64>(0))?,
                    conn.query_row("SELECT COUNT(*) FROM control_action_audit WHERE action='source.automation.create_task' AND status='succeeded'", [], |row| row.get::<_, i64>(0))?,
                    conn.query_row("SELECT goal FROM tasks WHERE id=?1", [&task_id], |row| row.get::<_, String>(0))?,
                    conn.query_row("SELECT COUNT(*) FROM source_automation_routes WHERE binding_snapshot_json LIKE '%xoxb-never-persist%' OR template_snapshot_json LIKE '%xoxb-never-persist%' OR permalink LIKE '%xoxb-never-persist%'", [], |row| row.get::<_, i64>(0))?,
                ))
            })
            .await
            .expect("automation evidence");
        assert_eq!(
            (tasks, routes, automation_bindings, audits, leaked),
            (1, 1, 1, 1, 0)
        );
        assert!(goal.contains("$docs"));
        assert!(goal.contains("https://acme.slack.com/archives/conversation-1/"));
    }

    #[tokio::test]
    async fn reaction_does_not_append_to_matching_bound_thread() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        repository
            .ingest(event("event-root", true))
            .await
            .expect("root");
        reconcile_source_once(&state).await.expect("route root");
        let before = state
            .async_database
            .reader()
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE event_type='source_context_added'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await
            .expect("context count");

        let reaction = repository
            .ingest(reaction_event("reaction-bound", "message"))
            .await
            .expect("reaction");
        reconcile_source_once(&state)
            .await
            .expect("ignore reaction");
        let after = state
            .async_database
            .reader()
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT COUNT(*) FROM events WHERE event_type='source_context_added'",
                    [],
                    |row| row.get::<_, i64>(0),
                )?)
            })
            .await
            .expect("context count");
        let routed = repository
            .get(&reaction.event.id)
            .await
            .expect("get")
            .expect("event");
        assert_eq!(before, after);
        assert_eq!(routed.routing_state, "ignored");
        assert!(routed.routed_task_id.is_none());
    }

    #[tokio::test]
    async fn non_message_reaction_is_ignored_with_target_reason() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let reaction = repository
            .ingest(reaction_event("reaction-file", "file"))
            .await
            .expect("reaction");
        reconcile_source_once(&state)
            .await
            .expect("ignore reaction");
        let routed = repository
            .get(&reaction.event.id)
            .await
            .expect("get")
            .expect("event");
        assert_eq!(routed.routing_state, "ignored");
        assert_eq!(
            routed.last_error_code.as_deref(),
            Some("unsupported_reaction_target")
        );
        assert!(routed.routed_task_id.is_none());
    }

    #[tokio::test]
    async fn ambiguous_binding_materializes_attention_without_guessing() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        let root = repository
            .ingest(event("event-root", true))
            .await
            .expect("root");
        reconcile_source_once(&state).await.expect("route root");
        let root_task = repository
            .get(&root.event.id)
            .await
            .expect("get root")
            .expect("root event")
            .routed_task_id
            .expect("root task");
        let secondary = agent_orchestrator::task_ops::create_task_impl_with_id(
            &state,
            CreateTaskPayload {
                name: Some("secondary".into()),
                goal: Some("secondary".into()),
                project_id: Some("default".into()),
                workspace_id: Some("default".into()),
                workflow_id: Some("basic".into()),
                ..Default::default()
            },
            Some("source-secondary"),
        )
        .expect("secondary task");
        repository
            .create_binding(CreateSourceBinding {
                project_id: "default".into(),
                task_id: secondary.id,
                provider: "fixture".into(),
                installation_id: "install-1".into(),
                conversation_id: Some("conversation-1".into()),
                thread_id: Some("thread-1".into()),
                binding_type: "related".into(),
                created_by_event_id: root.event.id,
            })
            .await
            .expect("ambiguous binding");
        let reply = repository
            .ingest(event("event-ambiguous", false))
            .await
            .expect("reply");
        reconcile_source_once(&state)
            .await
            .expect("route ambiguity");
        let routed = repository
            .get(&reply.event.id)
            .await
            .expect("get reply")
            .expect("reply event");
        assert_eq!(routed.routing_state, "needs_attention");
        assert_eq!(
            routed.last_error_code.as_deref(),
            Some("correlation_ambiguous")
        );
        assert!(routed.routed_task_id.is_none());
        let attention = state
            .attention_repo
            .list(
                agent_orchestrator::attention::AttentionFilter {
                    kind: Some("source_routing_ambiguous".into()),
                    limit: 10,
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("attention");
        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].source_event_id, reply.event.id);
        assert_ne!(root_task, "source-secondary");
    }

    #[tokio::test]
    async fn unknown_actor_privileged_command_fails_closed_and_is_audited() {
        let (_fixture, state) = state_with_trigger();
        let repository = AsyncSourceRepository::new(state.async_database.clone());
        repository
            .ingest(event("event-root", true))
            .await
            .expect("root");
        reconcile_source_once(&state).await.expect("route root");
        let command = repository
            .ingest(command_event(
                "event-command",
                "unknown-actor",
                SourceCommand::Cancel,
            ))
            .await
            .expect("command");
        reconcile_source_once(&state).await.expect("route command");
        let routed = repository
            .get(&command.event.id)
            .await
            .expect("get command")
            .expect("command event");
        assert_eq!(routed.routing_state, "failed");
        assert_eq!(
            routed.last_error_code.as_deref(),
            Some("actor_not_authorized")
        );
        let audit = state
            .async_database
            .reader()
            .call(move |conn| {
                Ok(conn.query_row(
                    "SELECT resolved_role,action,status,error_code FROM source_command_actions",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )?)
            })
            .await
            .expect("audit row");
        assert_eq!(
            audit,
            (
                "read_only".to_string(),
                "cancel".to_string(),
                "failed".to_string(),
                "actor_not_authorized".to_string()
            )
        );
    }
}
