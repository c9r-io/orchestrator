use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, map_core_error};

pub(crate) async fn apply(
    server: &OrchestratorServer,
    mut request: Request<ApplyRequest>,
) -> Result<Response<ApplyResponse>, Status> {
    super::authorize(server, &request, "Apply").map_err(Status::from)?;

    // Elevate to Admin when the manifest contains CRDs with plugins or hooks.
    // This prevents Operator-role callers (including agent subprocesses via UDS)
    // from injecting arbitrary shell commands into the plugin execution pipeline.
    if manifests_contain_executable_commands(&request.get_ref().content) {
        super::authorize(server, &request, "ApplyPluginCrd").map_err(Status::from)?;
    }

    let binding_mutation = !request.get_ref().dry_run
        && manifests_contain_source_task_bindings(&request.get_ref().content);
    let project_id = request
        .get_ref()
        .project
        .as_deref()
        .unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID)
        .to_string();
    let content_hash = agent_orchestrator::action_audit::canonical_request_hash(
        &serde_json::json!({"content": request.get_ref().content}),
    )
    .map_err(|error| Status::internal(error.to_string()))?;
    let context = request.get_ref().audit.clone();
    let dry_run = request.get_ref().dry_run;
    let prune = request.get_ref().prune;
    let attempt = if binding_mutation {
        Some(
            super::action_audit::begin(
                server,
                &mut request,
                "Apply",
                context.as_ref(),
                super::action_audit::ActionDescriptor {
                    project_id: &project_id,
                    target_type: "source_task_binding",
                    target_id: &format!("manifest:{}", &content_hash[..16]),
                    action: "source.binding.apply",
                    expected_version: None,
                    fencing_token: None,
                    canonical_request: serde_json::json!({
                        "content_hash": content_hash,
                        "project_id": project_id,
                        "dry_run": dry_run,
                        "prune": prune,
                    }),
                    fallback_reason_code: "legacy_client",
                    fallback_operator_reason: None,
                    fallback_idempotency_key: None,
                    renewable_exemption: false,
                },
            )
            .await?,
        )
    } else {
        None
    };
    if let Some(replayed) = attempt.as_ref().filter(|attempt| !attempt.should_execute) {
        return Err(replayed.status(Status::already_exists(
            "matching SourceTaskBinding apply already audited",
        )));
    }
    let req = request.into_inner();
    let result = agent_orchestrator::service::resource::apply_manifests(
        &server.state,
        &req.content,
        req.dry_run,
        req.project.as_deref(),
        req.prune,
    )
    .map_err(map_core_error);
    let result = match result {
        Ok(result) => result,
        Err(status) => {
            return Err(match attempt {
                Some(attempt) => attempt.failed(server, status).await,
                None => status,
            });
        }
    };
    if !result.errors.is_empty()
        && let Some(attempt) = attempt
    {
        return Err(attempt
            .failed(
                server,
                Status::failed_precondition(result.errors.join("; ")),
            )
            .await);
    }

    if let Some(attempt) = attempt {
        attempt
            .succeeded(
                server,
                Some("config_version"),
                result
                    .config_version
                    .map(|value| value.to_string())
                    .as_deref(),
            )
            .await?;
        Ok(attempt.response(result))
    } else {
        Ok(Response::new(result))
    }
}

fn manifests_contain_source_task_bindings(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "kind: SourceTaskBinding" || trimmed == "kind: 'SourceTaskBinding'"
    })
}

/// Check whether raw YAML content contains CRD manifests with non-empty
/// `plugins` or lifecycle `hooks` sections — i.e., executable commands.
fn manifests_contain_executable_commands(content: &str) -> bool {
    // Quick substring pre-filter to avoid full YAML parsing in the common case.
    let has_plugins = content.contains("plugins:");
    let has_hooks = content.contains("on_create:")
        || content.contains("on_update:")
        || content.contains("on_delete:");
    if !has_plugins && !has_hooks {
        return false;
    }

    // Parse YAML docs to confirm the presence is inside a CRD (kind: CustomResourceDefinition).
    for doc in content.split("\n---") {
        if doc.contains("kind: CustomResourceDefinition")
            && ((has_plugins && doc.contains("plugins:"))
                || (has_hooks
                    && (doc.contains("on_create:")
                        || doc.contains("on_update:")
                        || doc.contains("on_delete:"))))
        {
            return true;
        }
    }
    false
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<GetRequest>,
) -> Result<Response<GetResponse>, Status> {
    super::authorize(server, &request, "Get").map_err(Status::from)?;
    let req = request.into_inner();
    let content = agent_orchestrator::service::resource::get_resource(
        &server.state,
        &req.resource,
        req.selector.as_deref(),
        &req.output_format,
        req.project.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(GetResponse {
        content,
        format: req.output_format,
    }))
}

pub(crate) async fn describe(
    server: &OrchestratorServer,
    request: Request<DescribeRequest>,
) -> Result<Response<DescribeResponse>, Status> {
    super::authorize(server, &request, "Describe").map_err(Status::from)?;
    let req = request.into_inner();
    let content = agent_orchestrator::service::resource::describe_resource(
        &server.state,
        &req.resource,
        &req.output_format,
        req.project.as_deref(),
    )
    .map_err(map_core_error)?;

    Ok(Response::new(DescribeResponse {
        content,
        format: req.output_format,
    }))
}

pub(crate) async fn delete(
    server: &OrchestratorServer,
    mut request: Request<DeleteRequest>,
) -> Result<Response<DeleteResponse>, Status> {
    super::authorize(server, &request, "Delete").map_err(Status::from)?;
    if request.get_ref().force_references && !request.get_ref().force {
        return Err(Status::invalid_argument(
            "force_references requires force confirmation",
        ));
    }
    if request.get_ref().force_references && request.get_ref().audit.is_none() {
        return Err(Status::invalid_argument(
            "force_references requires ActionAuditContext",
        ));
    }
    let project_id = request
        .get_ref()
        .project
        .as_deref()
        .unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID)
        .to_string();
    let target_id = request.get_ref().resource.clone();
    let resource_kind = target_id.split('/').next().unwrap_or_default();
    let is_source_task_binding = matches!(
        resource_kind,
        "sourcetaskbinding" | "source-task-binding" | "source_task_binding" | "stb"
    );
    let reference_target_type = if matches!(resource_kind, "trigger" | "tg") {
        "trigger"
    } else {
        "source_task_template"
    };
    let dry_run = request.get_ref().dry_run;
    let context = request.get_ref().audit.clone();
    let force_references = request.get_ref().force_references;
    let attempt = if force_references || is_source_task_binding {
        Some(
            super::action_audit::begin(
                server,
                &mut request,
                if force_references {
                    "DeleteReferences"
                } else {
                    "Delete"
                },
                context.as_ref(),
                super::action_audit::ActionDescriptor {
                    project_id: &project_id,
                    target_type: if is_source_task_binding {
                        "source_task_binding"
                    } else {
                        reference_target_type
                    },
                    target_id: &target_id,
                    action: if is_source_task_binding {
                        "source.binding.delete"
                    } else {
                        "delete_references"
                    },
                    expected_version: None,
                    fencing_token: None,
                    canonical_request: serde_json::json!({
                        "resource": target_id.clone(),
                        "project_id": project_id.clone(),
                        "force": true,
                        "force_references": true,
                        "dry_run": dry_run,
                    }),
                    fallback_reason_code: if force_references {
                        "operator_force_reference_cleanup"
                    } else {
                        "legacy_client"
                    },
                    fallback_operator_reason: None,
                    fallback_idempotency_key: None,
                    renewable_exemption: false,
                },
            )
            .await?,
        )
    } else {
        None
    };
    let req = request.into_inner();
    if let Some(replayed) = attempt.as_ref().filter(|attempt| !attempt.should_execute) {
        return Ok(replayed.response(DeleteResponse {
            message: format!("{} reference cleanup already completed", req.resource),
        }));
    }
    if let Err(error) = agent_orchestrator::service::resource::delete_resource_with_references(
        &server.state,
        &req.resource,
        req.force,
        req.project.as_deref(),
        req.dry_run,
        req.force_references,
    ) {
        let status = map_core_error(error);
        return Err(match &attempt {
            Some(attempt) => attempt.failed(server, status).await,
            None => status,
        });
    }
    let scope = req
        .project
        .map(|p| format!(" (project: {})", p))
        .unwrap_or_default();
    let verb = if req.dry_run {
        "would be deleted (dry run)"
    } else {
        "deleted"
    };
    let response = DeleteResponse {
        message: format!("{} {}{}", req.resource, verb, scope),
    };
    if let Some(attempt) = attempt {
        let result_type = if is_source_task_binding {
            "source_task_binding"
        } else {
            reference_target_type
        };
        attempt
            .succeeded(server, Some(result_type), Some(&req.resource))
            .await?;
        Ok(attempt.response(response))
    } else {
        Ok(Response::new(response))
    }
}

pub(crate) async fn manifest_export(
    server: &OrchestratorServer,
    request: Request<ManifestExportRequest>,
) -> Result<Response<ManifestExportResponse>, Status> {
    super::authorize(server, &request, "ManifestExport").map_err(Status::from)?;
    let req = request.into_inner();
    let content =
        agent_orchestrator::service::resource::export_manifests(&server.state, &req.output_format)
            .map_err(map_core_error)?;
    Ok(Response::new(ManifestExportResponse {
        content,
        format: req.output_format,
    }))
}
