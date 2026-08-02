use orchestrator_proto::*;
use serde::Deserialize as _;
use tonic::{Request, Response, Status};

use super::{OrchestratorServer, map_core_error};

#[derive(Debug, Clone)]
struct ApplyResourceDescriptor {
    kind: agent_orchestrator::cli_types::ResourceKind,
    kind_name: String,
    name: String,
}

pub(crate) async fn apply(
    server: &OrchestratorServer,
    mut request: Request<ApplyRequest>,
) -> Result<Response<ApplyResponse>, Status> {
    super::authorize(server, &request, "Apply").map_err(Status::from)?;
    let _mutation_guard = server.config_mutation_lock.lock().await;

    // Elevate to Admin when the manifest contains CRDs with plugins or hooks.
    // This prevents Operator-role callers (including agent subprocesses via UDS)
    // from injecting arbitrary shell commands into the plugin execution pipeline.
    let contains_driver_raw_args = manifests_contain_driver_raw_args(&request.get_ref().content);
    if manifests_contain_executable_commands(&request.get_ref().content) || contains_driver_raw_args
    {
        super::authorize(server, &request, "ApplyPluginCrd").map_err(Status::from)?;
    }

    let resource_descriptor = single_builtin_apply_descriptor(&request.get_ref().content).ok();
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
    let expected_revision = request.get_ref().expected_revision.clone();
    let require_absent = request.get_ref().require_absent;
    let dry_run = request.get_ref().dry_run;
    let prune = request.get_ref().prune;
    let audited_mutation = !dry_run
        && (context.is_some()
            || contains_driver_raw_args
            || resource_descriptor.as_ref().is_some_and(|descriptor| {
                matches!(
                    descriptor.kind,
                    agent_orchestrator::cli_types::ResourceKind::SourceTaskTemplate
                        | agent_orchestrator::cli_types::ResourceKind::SourceTaskBinding
                )
            }));
    let target_type = resource_descriptor
        .as_ref()
        .map(|descriptor| match descriptor.kind {
            agent_orchestrator::cli_types::ResourceKind::SourceTaskTemplate => {
                "source_task_template"
            }
            agent_orchestrator::cli_types::ResourceKind::SourceTaskBinding => "source_task_binding",
            agent_orchestrator::cli_types::ResourceKind::Agent if contains_driver_raw_args => {
                "agent_driver"
            }
            _ => "resource",
        })
        .unwrap_or("resource_manifest");
    let action = resource_descriptor
        .as_ref()
        .map(|descriptor| match descriptor.kind {
            agent_orchestrator::cli_types::ResourceKind::SourceTaskTemplate => {
                "source.template.apply"
            }
            agent_orchestrator::cli_types::ResourceKind::SourceTaskBinding => {
                "source.binding.apply"
            }
            agent_orchestrator::cli_types::ResourceKind::Agent if contains_driver_raw_args => {
                "agent.driver.raw_args.apply"
            }
            _ => "resource.apply",
        })
        .unwrap_or("resource.apply");
    let target_id = resource_descriptor
        .as_ref()
        .map(|descriptor| format!("{}/{}", descriptor.kind_name, descriptor.name))
        .unwrap_or_else(|| format!("manifest:{}", &content_hash[..16]));
    let attempt = if audited_mutation {
        Some(
            super::action_audit::begin(
                server,
                &mut request,
                "Apply",
                context.as_ref(),
                super::action_audit::ActionDescriptor {
                    project_id: &project_id,
                    target_type,
                    target_id: &target_id,
                    action,
                    expected_version: expected_revision.clone(),
                    fencing_token: None,
                    canonical_request: serde_json::json!({
                        "content_hash": content_hash,
                        "project_id": project_id,
                        "dry_run": dry_run,
                        "prune": prune,
                        "expected_revision": expected_revision,
                        "require_absent": require_absent,
                    }),
                    fallback_reason_code: super::action_audit::FALLBACK_REASON_LEGACY_CLIENT,
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
            "matching audited resource apply already exists",
        )));
    }
    if expected_revision.is_some() || require_absent {
        let revision_result = validate_resource_revision(
            server,
            resource_descriptor.as_ref(),
            &project_id,
            expected_revision.as_deref(),
            require_absent,
        );
        if let Err(status) = revision_result {
            return Err(match attempt {
                Some(attempt) => attempt.failed(server, status).await,
                None => status,
            });
        }
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

fn manifests_contain_driver_raw_args(content: &str) -> bool {
    if !content.contains("rawArgs") {
        return false;
    }
    serde_yaml::Deserializer::from_str(content).any(|document| {
        let Ok(value) = serde_yaml::Value::deserialize(document) else {
            return false;
        };
        value.get("kind").and_then(serde_yaml::Value::as_str) == Some("Agent")
            && value
                .get("spec")
                .and_then(|spec| spec.get("driver"))
                .and_then(|driver| driver.get("rawArgs"))
                .and_then(serde_yaml::Value::as_sequence)
                .is_some_and(|args| !args.is_empty())
    })
}

fn validate_resource_revision(
    server: &OrchestratorServer,
    descriptor: Option<&ApplyResourceDescriptor>,
    project_id: &str,
    expected_revision: Option<&str>,
    require_absent: bool,
) -> Result<(), Status> {
    let descriptor = descriptor.ok_or_else(|| {
        Status::invalid_argument("optimistic resource apply requires exactly one builtin manifest")
    })?;
    let current = agent_orchestrator::service::resource::current_resource_revision(
        &server.state,
        descriptor.kind,
        &descriptor.name,
        Some(project_id),
    )
    .map_err(map_core_error)?;
    if require_absent && current.is_some() {
        return Err(Status::aborted(format!(
            "{}/{} was created after the editor loaded; refresh before saving",
            descriptor.kind_name, descriptor.name
        )));
    }
    if let Some(expected) = expected_revision {
        match current {
            Some(actual) if actual == expected => {}
            Some(_) => {
                return Err(Status::aborted(format!(
                    "{}/{} changed after the editor loaded; refresh before saving",
                    descriptor.kind_name, descriptor.name
                )));
            }
            None => {
                return Err(Status::aborted(format!(
                    "{}/{} no longer exists; refresh before saving",
                    descriptor.kind_name, descriptor.name
                )));
            }
        }
    }
    Ok(())
}

fn single_builtin_apply_descriptor(content: &str) -> Result<ApplyResourceDescriptor, Status> {
    use agent_orchestrator::resource::Resource;

    let manifests = agent_orchestrator::resource::parse_manifests_from_yaml(content)
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    if manifests.len() != 1 {
        return Err(Status::invalid_argument(
            "reviewed resource apply requires exactly one manifest",
        ));
    }
    let manifest = manifests
        .into_iter()
        .next()
        .ok_or_else(|| Status::invalid_argument("resource apply requires one manifest"))?;
    let agent_orchestrator::crd::ParsedManifest::Builtin(resource) = manifest else {
        return Err(Status::invalid_argument(
            "reviewed resource apply requires one builtin manifest",
        ));
    };
    let registered = agent_orchestrator::resource::dispatch_resource(*resource)
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(ApplyResourceDescriptor {
        kind: registered.kind(),
        kind_name: format!("{:?}", registered.kind()),
        name: registered.name().to_string(),
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

pub(crate) async fn catalog_list(
    server: &OrchestratorServer,
    request: Request<ResourceCatalogListRequest>,
) -> Result<Response<ResourceCatalogListResponse>, Status> {
    super::authorize(server, &request, "ResourceCatalogList").map_err(Status::from)?;
    let req = request.into_inner();
    let page = agent_orchestrator::service::resource::list_resource_summaries(
        &server.state,
        &req.resource_type,
        req.project.as_deref(),
        req.cursor.as_deref(),
        if req.limit == 0 {
            100
        } else {
            req.limit as usize
        },
    )
    .map_err(map_core_error)?;
    Ok(Response::new(ResourceCatalogListResponse {
        resources: page
            .resources
            .into_iter()
            .map(|resource| ResourceSummary {
                kind: resource.kind,
                name: resource.name,
                project_id: resource.project_id,
                revision: resource.revision,
                source: Some(resource.source),
            })
            .collect(),
        next_cursor: page.next_cursor,
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
    let summary = describe_summary(
        &req.resource,
        req.project
            .as_deref()
            .unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID),
        &content,
    )
    .map_err(map_core_error)?;

    Ok(Response::new(DescribeResponse {
        content,
        format: req.output_format,
        resource: summary,
    }))
}

fn describe_summary(
    resource: &str,
    project_id: &str,
    content: &str,
) -> agent_orchestrator::error::Result<Option<ResourceSummary>> {
    let Some((kind, name)) = resource.split_once('/') else {
        return Ok(None);
    };
    let canonical_kind = match kind {
        "ws" | "workspace" => "Workspace",
        "wf" | "workflow" => "Workflow",
        "agent" => "Agent",
        "steptemplate" | "step-template" | "step_template" => "StepTemplate",
        "executionprofile" | "execution-profile" | "execution_profile" => "ExecutionProfile",
        "trigger" | "tg" => "Trigger",
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt" => {
            "SourceTaskTemplate"
        }
        "sourcetaskbinding" | "source-task-binding" | "source_task_binding" | "stb" => {
            "SourceTaskBinding"
        }
        _ => return Ok(None),
    };
    Ok(Some(ResourceSummary {
        kind: canonical_kind.to_string(),
        name: name.to_string(),
        project_id: project_id.to_string(),
        revision: agent_orchestrator::service::resource::resource_content_revision(content)?,
        source: Some("describe_snapshot".to_string()),
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
                        super::action_audit::FALLBACK_REASON_LEGACY_CLIENT
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
        .map(|p| format!(" (project: {p})"))
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

#[cfg(test)]
mod driver_tests {
    use super::manifests_contain_driver_raw_args;

    #[test]
    fn raw_driver_args_are_detected_only_on_agent_documents() {
        let agent = r#"
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: codex
spec:
  driver:
    provider: codex
    rawArgs: [--experimental]
    unsafeRawArgs: true
"#;
        assert!(manifests_contain_driver_raw_args(agent));

        let workflow = agent.replace("kind: Agent", "kind: Workflow");
        assert!(!manifests_contain_driver_raw_args(&workflow));
        assert!(!manifests_contain_driver_raw_args(
            "kind: Agent\nspec:\n  driver:\n    provider: codex\n"
        ));
    }
}
