use anyhow::{Context, Result, bail};
use orchestrator_proto::{
    ActionAuditContext, OrchestratorServiceClient, SourceAutomationGetRequest,
    SourceAutomationListRequest, SourceAutomationMutationRequest, SourceAutomationRouteGetRequest,
    SourceAutomationSimulateRequest, SourceAutomationStatusRequest, SourceAutomationWatchRequest,
    SourceBindRequest, SourceBinding, SourceBindingListRequest, SourceConnectionCatalogRequest,
    SourceConnectionConnectRequest, SourceConnectionDedicatedGetRequest,
    SourceConnectionDedicatedMutationRequest, SourceConnectionDedicatedPreviewRequest,
    SourceConnectionDedicatedProvisioningResponse, SourceConnectionGetRequest,
    SourceConnectionIntentGetRequest, SourceConnectionIntentMutationRequest,
    SourceConnectionListRequest, SourceConnectionMutationRequest, SourceConnectionTransferRequest,
    SourceConnectionWatchRequest, SourceEvent, SourceEventGetRequest, SourceEventIngestRequest,
    SourceEventListRequest, SourceReplayRequest, SourceTaskBindingMutationRequest,
    SourceTaskBindingSimulateRequest, SourceTaskTemplatePreviewRequest,
};
use sha2::{Digest, Sha256};
use std::io::Read;
use tonic::transport::Channel;

use crate::{
    OutputFormat, SourceAutomationCommands, SourceBindingCommands, SourceCommands,
    SourceConnectionCommands, SourceTemplateCommands,
};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: SourceCommands,
) -> Result<()> {
    match command {
        SourceCommands::Connection { command } => dispatch_connection(client, command).await?,
        SourceCommands::Template { command } => match command {
            SourceTemplateCommands::Preview {
                name,
                project,
                provider,
                installation,
                message_url,
                event_id,
                reaction,
                target_id,
                output,
            } => {
                let preview = client
                    .source_task_template_preview(SourceTaskTemplatePreviewRequest {
                        name,
                        project_id: project,
                        provider,
                        installation_id: installation,
                        message_url,
                        event_id,
                        reaction,
                        target_id,
                        draft_content: None,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "name": preview.name,
                        "project_id": preview.project_id,
                        "skill": {
                            "name": preview.skill_name,
                            "invocation": preview.skill_invocation,
                            "args": preview.skill_args,
                        },
                        "goal": preview.goal,
                        "action": {
                            "workflow": preview.workflow,
                            "workspace": preview.workspace,
                            "start": preview.start,
                            "initial_vars": preview.initial_vars,
                        },
                        "content_hash": preview.content_hash,
                        "revision": preview.revision,
                        "warnings": preview.warnings,
                    }),
                    output,
                )?;
            }
        },
        SourceCommands::Binding { command } => match command {
            SourceBindingCommands::Simulate {
                project,
                provider,
                installation,
                event_kind,
                reaction,
                target_kind,
                channel,
                actor,
                output,
            } => {
                let result = client
                    .source_task_binding_simulate(SourceTaskBindingSimulateRequest {
                        project_id: project,
                        provider,
                        installation_id: installation,
                        event_kind,
                        reaction,
                        target_kind,
                        channel_id: channel,
                        external_actor_id: actor,
                        draft_content: None,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "status": result.status,
                        "reason": result.reason,
                        "trigger_name": result.trigger_name,
                        "resolved_role": result.resolved_role,
                        "binding_id": result.binding_id,
                        "template_ref": result.template_ref,
                        "binding_revision": result.binding_revision,
                        "candidates": result.candidates.into_iter().map(|candidate| serde_json::json!({
                            "binding_id": candidate.binding_id,
                            "reason": candidate.reason,
                            "revision": candidate.revision,
                        })).collect::<Vec<_>>(),
                    }),
                    output,
                )?;
            }
            SourceBindingCommands::Suspend { name, project } => {
                let result = client
                    .source_task_binding_suspend(SourceTaskBindingMutationRequest {
                        name,
                        project_id: project,
                        audit: Some(audit_context(
                            "operator_source_binding_suspend",
                            "binding-suspend",
                        )),
                        expected_revision: None,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "name": result.name,
                        "suspend": result.suspend,
                        "revision": result.revision,
                        "message": result.message,
                    }),
                    OutputFormat::Yaml,
                )?;
            }
            SourceBindingCommands::Resume { name, project } => {
                let result = client
                    .source_task_binding_resume(SourceTaskBindingMutationRequest {
                        name,
                        project_id: project,
                        audit: Some(audit_context(
                            "operator_source_binding_resume",
                            "binding-resume",
                        )),
                        expected_revision: None,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "name": result.name,
                        "suspend": result.suspend,
                        "revision": result.revision,
                        "message": result.message,
                    }),
                    OutputFormat::Yaml,
                )?;
            }
        },
        SourceCommands::Automation { command } => match command {
            SourceAutomationCommands::List {
                project,
                state,
                provider,
                binding,
                task,
                page_size,
                page_token,
                output,
            } => {
                let response = client
                    .source_automation_list(SourceAutomationListRequest {
                        project_id: project,
                        state,
                        provider,
                        binding_name: binding,
                        task_id: task,
                        page_size,
                        page_token,
                    })
                    .await?
                    .into_inner();
                if output == OutputFormat::Table {
                    println!("ID\tSTATE\tPROVIDER\tBINDING\tATTEMPTS\tTASK\tUPDATED");
                    for route in &response.routes {
                        println!(
                            "{}\t{}\t{}\t{}\t{}/{}\t{}\t{}",
                            route.id,
                            route.status,
                            route.provider,
                            route.binding_name,
                            route.attempt_count,
                            route.max_attempts,
                            route.task_id.as_deref().unwrap_or("-"),
                            route.updated_at,
                        );
                    }
                    if let Some(token) = response.next_page_token {
                        eprintln!("next_page_token={token}");
                    }
                } else {
                    print_value(
                        serde_json::json!({
                            "routes": response.routes.iter().map(automation_route_value).collect::<Vec<_>>(),
                            "next_page_token": response.next_page_token,
                        }),
                        output,
                    )?;
                }
            }
            SourceAutomationCommands::Get {
                route_id,
                attempt_limit,
                output,
            } => {
                let detail = client
                    .source_automation_get(SourceAutomationGetRequest {
                        route_id,
                        attempt_limit,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "route": detail.route.as_ref().map(automation_route_value),
                        "attempts": detail.attempts.into_iter().map(|attempt| serde_json::json!({
                            "id": attempt.id,
                            "route_id": attempt.route_id,
                            "generation": attempt.generation,
                            "attempt_no": attempt.attempt_no,
                            "started_at": attempt.started_at,
                            "completed_at": attempt.completed_at,
                            "result_state": attempt.result_state,
                            "error_code": attempt.error_code,
                            "error_category": attempt.error_category,
                            "retry_after_seconds": attempt.retry_after_seconds,
                        })).collect::<Vec<_>>(),
                    }),
                    output,
                )?;
            }
            SourceAutomationCommands::Watch {
                project,
                after,
                output,
            } => {
                let mut stream = client
                    .source_automation_watch(SourceAutomationWatchRequest {
                        project_id: project,
                        after_cursor: after,
                        interval_millis: 500,
                    })
                    .await?
                    .into_inner();
                while let Some(delta) = stream.message().await? {
                    print_value(
                        serde_json::json!({
                            "cursor": delta.cursor,
                            "route_version": delta.route_version,
                            "state": delta.state,
                            "error_code": delta.error_code,
                            "changed_at": delta.changed_at,
                            "route": delta.route.as_ref().map(automation_route_value),
                        }),
                        output,
                    )?;
                }
            }
            SourceAutomationCommands::Simulate {
                project,
                provider,
                installation,
                reaction,
                channel,
                actor,
                message_url,
                target_id,
                event_id,
                output,
            } => {
                let simulation = client
                    .source_automation_simulate(SourceAutomationSimulateRequest {
                        project_id: project,
                        provider,
                        installation_id: installation,
                        event_kind: "reaction_added".to_string(),
                        reaction,
                        target_kind: "message".to_string(),
                        channel_id: channel,
                        external_actor_id: actor,
                        message_url,
                        event_id,
                        target_id,
                        draft_binding_content: None,
                    })
                    .await?
                    .into_inner();
                let matched = simulation.match_result;
                let rendered = simulation.rendered;
                print_value(
                    serde_json::json!({
                        "match": matched.map(|value| serde_json::json!({
                            "status": value.status,
                            "reason": value.reason,
                            "trigger_name": value.trigger_name,
                            "resolved_role": value.resolved_role,
                            "binding_id": value.binding_id,
                            "template_ref": value.template_ref,
                            "binding_revision": value.binding_revision,
                            "candidates": value.candidates.into_iter().map(|candidate| serde_json::json!({
                                "binding_id": candidate.binding_id,
                                "reason": candidate.reason,
                                "revision": candidate.revision,
                            })).collect::<Vec<_>>(),
                        })),
                        "rendered": rendered.map(|value| serde_json::json!({
                            "skill_name": value.skill_name,
                            "skill_invocation": value.skill_invocation,
                            "skill_args": value.skill_args,
                            "goal": value.goal,
                            "workflow": value.workflow,
                            "workspace": value.workspace,
                            "start": value.start,
                            "initial_vars": value.initial_vars,
                            "content_hash": value.content_hash,
                            "revision": value.revision,
                            "warnings": value.warnings,
                        })),
                        "mutation_performed": simulation.mutation_performed,
                        "network_performed": simulation.network_performed,
                    }),
                    output,
                )?;
            }
            SourceAutomationCommands::Replay {
                route_id,
                expected_version,
                reason,
                idempotency_key,
                adopt_current_config,
                output,
            } => {
                let route = client
                    .source_automation_replay(SourceAutomationMutationRequest {
                        route_id,
                        expected_version,
                        reason,
                        idempotency_key,
                        adopt_current_config,
                    })
                    .await?
                    .into_inner();
                print_value(automation_route_value(&route), output)?;
            }
            SourceAutomationCommands::Ignore {
                route_id,
                expected_version,
                reason,
                idempotency_key,
                output,
            } => {
                let route = client
                    .source_automation_ignore(SourceAutomationMutationRequest {
                        route_id,
                        expected_version,
                        reason,
                        idempotency_key,
                        adopt_current_config: false,
                    })
                    .await?
                    .into_inner();
                print_value(automation_route_value(&route), output)?;
            }
            SourceAutomationCommands::Status { project, output } => {
                let status = client
                    .source_automation_status_get(SourceAutomationStatusRequest {
                        project_id: project,
                    })
                    .await?
                    .into_inner();
                print_value(
                    serde_json::json!({
                        "project_id": status.project_id,
                        "backlog_count": status.backlog_count,
                        "oldest_age_seconds": status.oldest_age_seconds,
                        "active_leases": status.active_leases,
                        "retrying_count": status.retrying_count,
                        "needs_attention_count": status.needs_attention_count,
                        "failure_categories": status.failure_categories.into_iter().map(|value| serde_json::json!({
                            "category": value.category,
                            "count": value.count,
                        })).collect::<Vec<_>>(),
                    }),
                    output,
                )?;
            }
        },
        SourceCommands::List {
            project,
            task,
            state,
            limit,
            output,
        } => {
            let response = client
                .source_event_list(SourceEventListRequest {
                    project_id: project,
                    task_id: task,
                    routing_state: state,
                    limit,
                })
                .await?
                .into_inner();
            print_events(&response.events, output)?;
        }
        SourceCommands::Get { id, output } => {
            let event = client
                .source_event_get(SourceEventGetRequest { id })
                .await?
                .into_inner();
            print_value(event_value(&event), output)?;
        }
        SourceCommands::Route {
            source_event_id,
            output,
        } => {
            let route = client
                .source_automation_route_get(SourceAutomationRouteGetRequest { source_event_id })
                .await?
                .into_inner();
            print_value(
                serde_json::json!({
                    "id": route.id,
                    "project_id": route.project_id,
                    "source_event_id": route.source_event_id,
                    "provider": route.provider,
                    "reaction": route.reaction,
                    "binding_name": route.binding_name,
                    "binding_revision": route.binding_revision,
                    "template_name": route.template_name,
                    "template_hash": route.template_hash,
                    "status": route.status,
                    "error_code": route.error_code,
                    "task_id": route.task_id,
                    "permalink": route.permalink,
                    "request_id": route.request_id,
                    "created_at": route.created_at,
                    "completed_at": route.completed_at,
                }),
                output,
            )?;
        }
        SourceCommands::Ingest {
            project,
            file,
            payload_hash,
        } => {
            let normalized_json = super::common::read_input_or_file(&file)?;
            let payload_hash = payload_hash.unwrap_or_else(|| {
                Sha256::digest(normalized_json.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            });
            let response = client
                .source_event_ingest(SourceEventIngestRequest {
                    project_id: project,
                    normalized_json,
                    payload_hash,
                    audit: Some(audit_context("operator_source_ingest", "ingest")),
                })
                .await?
                .into_inner();
            let event = response
                .event
                .as_ref()
                .map(event_value)
                .unwrap_or(serde_json::Value::Null);
            print_value(
                serde_json::json!({"inserted": response.inserted, "event": event}),
                OutputFormat::Yaml,
            )?;
        }
        SourceCommands::Bindings { task_id, output } => {
            let response = client
                .source_binding_list(SourceBindingListRequest { task_id })
                .await?
                .into_inner();
            print_bindings(&response.bindings, output)?;
        }
        SourceCommands::Bind {
            project,
            task,
            provider,
            installation,
            conversation,
            thread,
            binding_type,
            source_event,
        } => {
            let binding = client
                .source_bind(SourceBindRequest {
                    project_id: project,
                    task_id: task,
                    provider,
                    installation_id: installation,
                    conversation_id: conversation,
                    thread_id: thread,
                    binding_type,
                    created_by_event_id: source_event,
                    audit: Some(audit_context("operator_source_bind", "bind")),
                })
                .await?
                .into_inner();
            print_value(binding_value(&binding), OutputFormat::Yaml)?;
        }
        SourceCommands::Replay { id } => {
            let response = client
                .source_replay(SourceReplayRequest {
                    id,
                    audit: Some(audit_context("operator_source_replay", "replay")),
                })
                .await?
                .into_inner();
            println!("{}\t{}", response.id, response.status);
        }
    }
    Ok(())
}

async fn dispatch_connection(
    client: &mut OrchestratorServiceClient<Channel>,
    command: SourceConnectionCommands,
) -> Result<()> {
    match command {
        SourceConnectionCommands::List {
            project,
            provider,
            include_disconnected,
            output,
        } => {
            let response = client
                .source_connection_list(SourceConnectionListRequest {
                    project_id: project,
                    provider,
                    include_disconnected,
                    limit: 200,
                })
                .await?
                .into_inner();
            if output == OutputFormat::Table {
                println!("ID\tPROVIDER\tMODE\tSTATE\tGEN\tTRIGGER\tUPDATED");
                for connection in response.connections {
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        connection.id,
                        connection.provider,
                        connection.provisioning_mode,
                        connection.state,
                        connection.generation,
                        connection.trigger_name.as_deref().unwrap_or("-"),
                        connection.updated_at,
                    );
                }
            } else {
                print_value(
                    serde_json::Value::Array(
                        response.connections.iter().map(connection_value).collect(),
                    ),
                    output,
                )?;
            }
        }
        SourceConnectionCommands::Get {
            id,
            project,
            output,
        } => {
            let connection = client
                .source_connection_get(SourceConnectionGetRequest {
                    project_id: project,
                    id,
                })
                .await?
                .into_inner();
            print_value(connection_value(&connection), output)?;
        }
        SourceConnectionCommands::Watch { project, after } => {
            let mut stream = client
                .source_connection_watch(SourceConnectionWatchRequest {
                    project_id: project,
                    after_cursor: after,
                    interval_millis: 500,
                })
                .await?
                .into_inner();
            while let Some(delta) = stream.message().await? {
                print_value(
                    serde_json::json!({
                        "cursor": delta.cursor,
                        "connection_version": delta.connection_version,
                        "state": delta.state,
                        "error_code": delta.error_code,
                        "request_id": delta.request_id,
                        "changed_at": delta.changed_at,
                        "connection": delta.connection.as_ref().map(connection_value),
                    }),
                    OutputFormat::Json,
                )?;
            }
        }
        SourceConnectionCommands::Catalog { output } => {
            let catalog = client
                .source_connection_catalog_get(SourceConnectionCatalogRequest {})
                .await?
                .into_inner();
            print_value(
                serde_json::json!({
                    "protocol_version": catalog.protocol_version,
                    "gateway_configured": catalog.gateway_configured,
                    "permalink_proxy": catalog.permalink_proxy,
                    "modes": catalog.modes.into_iter().map(|mode| serde_json::json!({
                        "mode": mode.mode,
                        "available": mode.available,
                        "unavailable_reason": mode.unavailable_reason,
                    })).collect::<Vec<_>>(),
                }),
                output,
            )?;
        }
        SourceConnectionCommands::Connect {
            project,
            label,
            reason,
            idempotency_key,
            no_open,
        } => {
            let intent = client
                .source_connection_connect(SourceConnectionConnectRequest {
                    project_id: project,
                    provider: "slack".into(),
                    provisioning_mode: "managed_shared".into(),
                    display_label: label,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            maybe_open_oauth(intent.authorize_url.as_deref(), no_open);
            print_value(intent_value(&intent), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::ProvisionDedicated {
            project,
            label,
            config_token_stdin,
            approve,
            reason,
            idempotency_key,
            no_open,
        } => {
            if !config_token_stdin {
                bail!(
                    "--config-token-stdin is required; Configuration Tokens are never accepted in argv or environment"
                );
            }
            let mut config_token = String::new();
            std::io::stdin()
                .take(8193)
                .read_to_string(&mut config_token)
                .context("failed to read Configuration Token from stdin")?;
            let config_token = config_token.trim().to_string();
            if config_token.is_empty() || config_token.len() > 8192 {
                bail!("Configuration Token stdin must contain 1-8192 characters");
            }
            let preview = client
                .source_connection_dedicated_preview(SourceConnectionDedicatedPreviewRequest {
                    project_id: project.clone(),
                    display_label: label,
                    config_token,
                    idempotency_key: idempotency_key.clone(),
                    reason: reason.clone(),
                })
                .await?
                .into_inner();
            if !approve {
                print_value(dedicated_value(&preview), OutputFormat::Yaml)?;
                eprintln!(
                    "Review the manifest diff, then run `orchestrator source connection dedicated-resume {} --project {} ...` to approve before expiry.",
                    preview.id, project
                );
            } else {
                let approved = client
                    .source_connection_dedicated_approve(SourceConnectionDedicatedMutationRequest {
                        project_id: project,
                        provisioning_id: preview.id,
                        idempotency_key: format!("{idempotency_key}-approve"),
                        reason,
                    })
                    .await?
                    .into_inner();
                maybe_open_oauth(approved.authorize_url.as_deref(), no_open);
                print_value(dedicated_value(&approved), OutputFormat::Yaml)?;
            }
        }
        SourceConnectionCommands::DedicatedStatus {
            provisioning_id,
            project,
            output,
        } => {
            let value = client
                .source_connection_dedicated_get(SourceConnectionDedicatedGetRequest {
                    project_id: project,
                    provisioning_id,
                })
                .await?
                .into_inner();
            print_value(dedicated_value(&value), output)?;
        }
        SourceConnectionCommands::DedicatedResume {
            provisioning_id,
            project,
            reason,
            idempotency_key,
            no_open,
        } => {
            let value = client
                .source_connection_dedicated_approve(SourceConnectionDedicatedMutationRequest {
                    project_id: project,
                    provisioning_id,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            maybe_open_oauth(value.authorize_url.as_deref(), no_open);
            print_value(dedicated_value(&value), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::DedicatedAbandon {
            provisioning_id,
            project,
            reason,
            idempotency_key,
        } => {
            let value = client
                .source_connection_dedicated_abandon(SourceConnectionDedicatedMutationRequest {
                    project_id: project,
                    provisioning_id,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            print_value(dedicated_value(&value), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::Status {
            intent_id,
            project,
            output,
        } => {
            let intent = client
                .source_connection_intent_get(SourceConnectionIntentGetRequest {
                    project_id: project,
                    intent_id,
                })
                .await?
                .into_inner();
            print_value(intent_value(&intent), output)?;
        }
        SourceConnectionCommands::Cancel {
            intent_id,
            project,
            reason,
            idempotency_key,
        } => {
            let intent = client
                .source_connection_cancel(SourceConnectionIntentMutationRequest {
                    project_id: project,
                    intent_id,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            print_value(intent_value(&intent), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::Reauthorize {
            id,
            project,
            expected_version,
            reason,
            idempotency_key,
            no_open,
        } => {
            let intent = client
                .source_connection_reauthorize(SourceConnectionMutationRequest {
                    project_id: project,
                    id,
                    expected_version,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            maybe_open_oauth(intent.authorize_url.as_deref(), no_open);
            print_value(intent_value(&intent), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::Disconnect {
            id,
            project,
            expected_version,
            reason,
            idempotency_key,
        } => {
            let connection = client
                .source_connection_disconnect(SourceConnectionMutationRequest {
                    project_id: project,
                    id,
                    expected_version,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            print_value(connection_value(&connection), OutputFormat::Yaml)?;
        }
        SourceConnectionCommands::Transfer {
            id,
            project,
            expected_version,
            target_daemon_id,
            reason,
            idempotency_key,
        } => {
            let connection = client
                .source_connection_transfer(SourceConnectionTransferRequest {
                    project_id: project,
                    id,
                    expected_version,
                    target_daemon_id,
                    idempotency_key,
                    reason,
                })
                .await?
                .into_inner();
            print_value(connection_value(&connection), OutputFormat::Yaml)?;
        }
    }
    Ok(())
}

fn maybe_open_oauth(url: Option<&str>, no_open: bool) {
    let Some(url) = url else { return };
    if no_open || !oauth_authorize_url_allowed(url) {
        return;
    }
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).status();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).status();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let result: std::io::Result<std::process::ExitStatus> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "browser launch unsupported",
    ));
    if !matches!(result, Ok(status) if status.success()) {
        eprintln!("OAuth URL was not opened automatically; use the authorize_url shown below.");
    }
}

fn oauth_authorize_url_allowed(value: &str) -> bool {
    let Ok(value) = url::Url::parse(value) else {
        return false;
    };
    value.scheme() == "https"
        && value.host_str() == Some("slack.com")
        && matches!(value.port(), None | Some(443))
        && value.path() == "/oauth/v2/authorize"
        && value.query().is_some()
        && value.username().is_empty()
        && value.password().is_none()
}

fn connection_value(value: &orchestrator_proto::SourceConnection) -> serde_json::Value {
    serde_json::json!({
        "id": value.id,
        "project_id": value.project_id,
        "provider": value.provider,
        "display_label": value.display_label,
        "provisioning_mode": value.provisioning_mode,
        "app_ownership": value.app_ownership,
        "app_id_digest": value.app_id_digest,
        "manifest_version": value.manifest_version,
        "provision_state": value.provision_state,
        "provision_error_code": value.provision_error_code,
        "installation_id": value.installation_id,
        "installation_id_digest": value.installation_id_digest,
        "enterprise_id_digest": value.enterprise_id_digest,
        "owner_daemon_id": value.owner_daemon_id,
        "generation": value.generation,
        "version": value.version,
        "state": value.state,
        "capabilities": value.capabilities,
        "scopes": value.scopes,
        "trigger_name": value.trigger_name,
        "last_delivery_at": value.last_delivery_at,
        "last_acked_cursor": value.last_acked_cursor,
        "delivery_lag": value.delivery_lag,
        "last_error_code": value.last_error_code,
        "created_at": value.created_at,
        "updated_at": value.updated_at,
        "reauthorized_at": value.reauthorized_at,
        "disconnected_at": value.disconnected_at,
    })
}

fn dedicated_value(value: &SourceConnectionDedicatedProvisioningResponse) -> serde_json::Value {
    serde_json::json!({
        "id": value.id,
        "project_id": value.project_id,
        "status": value.status,
        "manifest_version": value.manifest_version,
        "manifest_digest": value.manifest_digest,
        "diff": value.diff.iter().map(|entry| serde_json::json!({
            "field": entry.field,
            "change": entry.change,
            "before": entry.before,
            "after": entry.after,
            "permission_expansion": entry.permission_expansion,
        })).collect::<Vec<_>>(),
        "app_id_digest": value.app_id_digest,
        "oauth_intent_id": value.oauth_intent_id,
        "authorize_url": value.authorize_url,
        "error_code": value.error_code,
        "expires_at": value.expires_at,
    })
}

fn intent_value(value: &orchestrator_proto::SourceConnectionIntentResponse) -> serde_json::Value {
    serde_json::json!({
        "id": value.id,
        "project_id": value.project_id,
        "provider": value.provider,
        "provisioning_mode": value.provisioning_mode,
        "status": value.status,
        "connection_id": value.connection_id,
        "error_code": value.error_code,
        "expires_at": value.expires_at,
        "authorize_url": value.authorize_url,
        "connection": value.connection.as_ref().map(connection_value),
    })
}

fn audit_context(reason_code: &str, prefix: &str) -> ActionAuditContext {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    ActionAuditContext {
        reason_code: reason_code.to_string(),
        operator_reason: None,
        idempotency_key: Some(format!("cli-source-{prefix}-{nonce}")),
    }
}

fn print_events(events: &[SourceEvent], output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        println!("ID\tPROVIDER\tTYPE\tINSTALLATION\tSTATE\tTASK\tRECEIVED");
        for event in events {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                event.id,
                event.provider,
                event.event_type,
                event.installation_id,
                event.routing_state,
                event.routed_task_id.as_deref().unwrap_or("-"),
                event.received_at
            );
        }
        return Ok(());
    }
    print_value(
        serde_json::Value::Array(events.iter().map(event_value).collect()),
        output,
    )
}

fn print_bindings(bindings: &[SourceBinding], output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        println!("ID\tPROVIDER\tINSTALLATION\tCONVERSATION\tTHREAD\tTYPE\tTASK");
        for binding in bindings {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                binding.id,
                binding.provider,
                binding.installation_id,
                binding.conversation_id.as_deref().unwrap_or("-"),
                binding.thread_id.as_deref().unwrap_or("-"),
                binding.binding_type,
                binding.task_id
            );
        }
        return Ok(());
    }
    print_value(
        serde_json::Value::Array(bindings.iter().map(binding_value).collect()),
        output,
    )
}

fn print_value(value: serde_json::Value, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
        OutputFormat::Yaml | OutputFormat::Table => print!("{}", serde_yaml::to_string(&value)?),
    }
    Ok(())
}

fn event_value(event: &SourceEvent) -> serde_json::Value {
    serde_json::json!({
        "id": event.id,
        "project_id": event.project_id,
        "provider": event.provider,
        "installation_id": event.installation_id,
        "external_event_id": event.external_event_id,
        "event_type": event.event_type,
        "external_actor_id": event.external_actor_id,
        "conversation_id": event.conversation_id,
        "thread_id": event.thread_id,
        "occurred_at": event.occurred_at,
        "received_at": event.received_at,
        "normalized": serde_json::from_str::<serde_json::Value>(&event.normalized_json)
            .unwrap_or(serde_json::Value::Null),
        "payload_hash": event.payload_hash,
        "routing_state": event.routing_state,
        "routing_attempts": event.routing_attempts,
        "routed_task_id": event.routed_task_id,
        "last_error_code": event.last_error_code,
        "automation_route_id": event.automation_route_id,
        "automation_status": event.automation_status,
        "automation_binding_name": event.automation_binding_name,
        "automation_template_name": event.automation_template_name,
        "automation_template_hash": event.automation_template_hash,
    })
}

fn automation_route_value(route: &orchestrator_proto::SourceAutomationRoute) -> serde_json::Value {
    serde_json::json!({
        "id": route.id,
        "project_id": route.project_id,
        "source_event_id": route.source_event_id,
        "provider": route.provider,
        "reaction": route.reaction,
        "binding_name": route.binding_name,
        "binding_revision": route.binding_revision,
        "template_name": route.template_name,
        "template_hash": route.template_hash,
        "status": route.status,
        "error_code": route.error_code,
        "error_category": route.error_category,
        "task_id": route.task_id,
        "permalink": route.permalink,
        "request_id": route.request_id,
        "generation": route.generation,
        "version": route.version,
        "attempt_count": route.attempt_count,
        "max_attempts": route.max_attempts,
        "next_attempt_at": route.next_attempt_at,
        "lease_expires_at": route.lease_expires_at,
        "suspended_scope": route.suspended_scope,
        "last_attempt_at": route.last_attempt_at,
        "created_at": route.created_at,
        "updated_at": route.updated_at,
        "completed_at": route.completed_at,
    })
}

fn binding_value(binding: &SourceBinding) -> serde_json::Value {
    serde_json::json!({
        "id": binding.id,
        "project_id": binding.project_id,
        "task_id": binding.task_id,
        "provider": binding.provider,
        "installation_id": binding.installation_id,
        "conversation_id": binding.conversation_id,
        "thread_id": binding.thread_id,
        "binding_type": binding.binding_type,
        "created_by_event_id": binding.created_by_event_id,
        "created_at": binding.created_at,
    })
}

#[cfg(test)]
mod managed_connection_tests {
    use super::oauth_authorize_url_allowed;

    #[test]
    fn oauth_browser_allowlist_is_exact() {
        assert!(oauth_authorize_url_allowed(
            "https://slack.com/oauth/v2/authorize?state=opaque"
        ));
        assert!(!oauth_authorize_url_allowed(
            "https://slack.com.evil.example/oauth/v2/authorize?state=opaque"
        ));
        assert!(!oauth_authorize_url_allowed(
            "https://slack.com/oauth/v2/authorize"
        ));
    }
}
