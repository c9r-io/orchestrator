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
    // Every non-dry-run apply is audited. The condition this replaced also
    // required an envelope, raw args, or one of two Source kinds, and its first
    // disjunct was `context.is_some()` — so an envelope-less apply never reached
    // `action_audit::begin`, and the `enforced` rejection inside `resolve_context`
    // was unreachable precisely when it should fire. DD-111 makes the envelope the
    // durable record of *every* process-console mutation; this restores that.
    let audited_mutation = !dry_run;
    let agent_raw_args_override = |descriptor: &ApplyResourceDescriptor| {
        contains_driver_raw_args
            && matches!(
                descriptor.kind,
                agent_orchestrator::cli_types::ResourceKind::Agent
            )
    };
    let target_type = resource_descriptor
        .as_ref()
        .map(|descriptor| {
            if agent_raw_args_override(descriptor) {
                "agent_driver"
            } else {
                resource_target_type(descriptor.kind)
            }
        })
        .unwrap_or("resource_manifest");
    let action = resource_descriptor
        .as_ref()
        .map(|descriptor| {
            if agent_raw_args_override(descriptor) {
                "agent.driver.raw_args.apply"
            } else {
                apply_action(descriptor.kind)
            }
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

/// Canonical audit action for a builtin apply, one per `ResourceKind`.
///
/// The match is deliberately exhaustive with no `_` arm: a thirteenth variant
/// must fail to compile here rather than silently inherit the generic
/// `resource.apply` name, which is how eleven of these went unnamed. That
/// compile-time obligation — not any runtime list — is what makes the covered
/// set derived from the enum.
///
/// `source.template.apply` and `source.binding.apply` keep their shipped names.
/// They already appear in DD-111, QA 157 and recorded audit rows, and renaming
/// them for regularity would falsify those records. `Agent` maps to
/// `resource.agent.apply` here; the caller substitutes
/// `agent.driver.raw_args.apply` when raw args are present, since that is a
/// property of the payload rather than of the kind.
fn apply_action(kind: agent_orchestrator::cli_types::ResourceKind) -> &'static str {
    use agent_orchestrator::cli_types::ResourceKind;
    match kind {
        ResourceKind::Workspace => "resource.workspace.apply",
        ResourceKind::Agent => "resource.agent.apply",
        ResourceKind::Workflow => "resource.workflow.apply",
        ResourceKind::Project => "resource.project.apply",
        ResourceKind::RuntimePolicy => "resource.runtime_policy.apply",
        ResourceKind::StepTemplate => "resource.step_template.apply",
        ResourceKind::SourceTaskTemplate => "source.template.apply",
        ResourceKind::SourceTaskBinding => "source.binding.apply",
        ResourceKind::ExecutionProfile => "resource.execution_profile.apply",
        ResourceKind::EnvStore => "resource.env_store.apply",
        ResourceKind::SecretStore => "resource.secret_store.apply",
        ResourceKind::Trigger => "resource.trigger.apply",
    }
}

/// Canonical audit action for a builtin delete, one per `ResourceKind`.
///
/// Exhaustive and wildcard-free for the same reason as [`apply_action`]: a
/// thirteenth variant must fail to compile rather than silently inherit the
/// generic `resource.delete`.
///
/// Two names break the `resource.<snake_kind>.delete` rule, and for one reason
/// each. `source.binding.delete` is already recorded in `control_action_audit`
/// and named by `scripts/qa/test-source-task-binding.sh`; renaming it would
/// falsify stored rows. `source.template.delete` is new but is spelled to match
/// its own apply, `source.template.apply` — the vocabulary is one family per
/// kind, so that an auditor asking "everything about this source template" needs
/// one prefix rather than two. `delete_action_naming::apply_and_delete_share_a_family`
/// asserts that property rather than leaving it to care.
fn delete_action(kind: agent_orchestrator::cli_types::ResourceKind) -> &'static str {
    use agent_orchestrator::cli_types::ResourceKind;
    match kind {
        ResourceKind::Workspace => "resource.workspace.delete",
        ResourceKind::Agent => "resource.agent.delete",
        ResourceKind::Workflow => "resource.workflow.delete",
        ResourceKind::Project => "resource.project.delete",
        ResourceKind::RuntimePolicy => "resource.runtime_policy.delete",
        ResourceKind::StepTemplate => "resource.step_template.delete",
        ResourceKind::SourceTaskTemplate => "source.template.delete",
        ResourceKind::SourceTaskBinding => "source.binding.delete",
        ResourceKind::ExecutionProfile => "resource.execution_profile.delete",
        ResourceKind::EnvStore => "resource.env_store.delete",
        ResourceKind::SecretStore => "resource.secret_store.delete",
        ResourceKind::Trigger => "resource.trigger.delete",
    }
}

/// Canonical audit `target_type` for a builtin resource, one per `ResourceKind`.
///
/// Exhaustive for the same reason as [`apply_action`]. The two Source kinds keep
/// the `source_task_*` spellings already present in stored rows.
///
/// Shared by apply and delete. `target_type` names the object, not the verb, and
/// the values the delete path already stored for `source.binding.delete` and
/// `delete_references` agree with this table — so reusing it moves nothing, while
/// a second table would be free to drift from this one.
fn resource_target_type(kind: agent_orchestrator::cli_types::ResourceKind) -> &'static str {
    use agent_orchestrator::cli_types::ResourceKind;
    match kind {
        ResourceKind::Workspace => "workspace",
        ResourceKind::Agent => "agent",
        ResourceKind::Workflow => "workflow",
        ResourceKind::Project => "project",
        ResourceKind::RuntimePolicy => "runtime_policy",
        ResourceKind::StepTemplate => "step_template",
        ResourceKind::SourceTaskTemplate => "source_task_template",
        ResourceKind::SourceTaskBinding => "source_task_binding",
        ResourceKind::ExecutionProfile => "execution_profile",
        ResourceKind::EnvStore => "env_store",
        ResourceKind::SecretStore => "secret_store",
        ResourceKind::Trigger => "trigger",
    }
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
    let resolved_kind = agent_orchestrator::resource::resource_kind_from_alias(resource_kind);
    // `--force-references` is valid only for SourceTaskTemplate and Trigger; the
    // service layer refuses every other kind before anything is removed.
    let reference_target_type = match resolved_kind {
        Some(agent_orchestrator::cli_types::ResourceKind::Trigger) => "trigger",
        _ => "source_task_template",
    };
    let dry_run = request.get_ref().dry_run;
    let force = request.get_ref().force;
    let context = request.get_ref().audit.clone();
    let force_references = request.get_ref().force_references;
    // `None` is a real answer, not a parse failure: `crd`,
    // `customresourcedefinition` and every CRD-defined custom kind are deletable
    // and have no `ResourceKind`. They record the generic name, mirroring an
    // apply that resolves to no single builtin manifest.
    let (delete_target_type, delete_action_name) = match resolved_kind {
        Some(kind) => (resource_target_type(kind), delete_action(kind)),
        None => ("resource_manifest", "resource.delete"),
    };
    // Every non-dry-run delete is audited. The condition this replaced fired
    // only for `--force-references` and SourceTaskBinding, so eleven of twelve
    // kinds left no `control_action_audit` row at all — and unlike apply's old
    // condition it had no `context.is_some()` disjunct, so an envelope was
    // accepted and then discarded. The CLI sends one on every delete, which made
    // the default path the dropped one. With `begin` unreachable, the `enforced`
    // rejection inside `resolve_context` could not fire either: the mode neither
    // audited a delete nor refused it. DD-111 makes the envelope the durable
    // record of *every* process-console mutation, and a delete is irreversible.
    let attempt = if !dry_run {
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
                    target_type: if force_references {
                        reference_target_type
                    } else {
                        delete_target_type
                    },
                    target_id: &target_id,
                    // A `--force-references` cleanup keeps its own name. It is a
                    // cross-resource action — it removes bindings the caller did
                    // not name — so it is not the per-kind delete of its target
                    // and does not join that naming surface.
                    action: if force_references {
                        "delete_references"
                    } else {
                        delete_action_name
                    },
                    expected_version: None,
                    fencing_token: None,
                    canonical_request: serde_json::json!({
                        "resource": target_id.clone(),
                        "project_id": project_id.clone(),
                        "force": force,
                        "force_references": force_references,
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
            message: if force_references {
                format!("{} reference cleanup already completed", req.resource)
            } else {
                format!("{} already deleted", req.resource)
            },
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
        let result_type = if force_references {
            reference_target_type
        } else {
            delete_target_type
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
mod apply_action_naming {
    use super::{apply_action, resource_target_type};
    use agent_orchestrator::cli_types::ResourceKind;

    /// Every `ResourceKind`, listed once.
    ///
    /// Two independent gates keep this honest, and neither is this array on its
    /// own. A thirteenth variant fails to compile in `apply_action` and
    /// `resource_target_type`, which have no `_` arm — that is the real derivation
    /// from the enum. `covers_every_variant` below then fails until the variant
    /// is added here too, so the array cannot silently fall behind the enum it
    /// claims to enumerate.
    const ALL_KINDS: [ResourceKind; 12] = [
        ResourceKind::Workspace,
        ResourceKind::Agent,
        ResourceKind::Workflow,
        ResourceKind::Project,
        ResourceKind::RuntimePolicy,
        ResourceKind::StepTemplate,
        ResourceKind::SourceTaskTemplate,
        ResourceKind::SourceTaskBinding,
        ResourceKind::ExecutionProfile,
        ResourceKind::EnvStore,
        ResourceKind::SecretStore,
        ResourceKind::Trigger,
    ];

    /// Fails if a variant is added to `ResourceKind` without being added to
    /// `ALL_KINDS`. The match is exhaustive and wildcard-free on purpose: it is
    /// the compiler, not the assertion, that notices the new variant.
    #[test]
    fn covers_every_variant() {
        fn discriminant_index(kind: ResourceKind) -> usize {
            match kind {
                ResourceKind::Workspace => 0,
                ResourceKind::Agent => 1,
                ResourceKind::Workflow => 2,
                ResourceKind::Project => 3,
                ResourceKind::RuntimePolicy => 4,
                ResourceKind::StepTemplate => 5,
                ResourceKind::SourceTaskTemplate => 6,
                ResourceKind::SourceTaskBinding => 7,
                ResourceKind::ExecutionProfile => 8,
                ResourceKind::EnvStore => 9,
                ResourceKind::SecretStore => 10,
                ResourceKind::Trigger => 11,
            }
        }
        let mut seen = [false; 12];
        for kind in ALL_KINDS {
            seen[discriminant_index(kind)] = true;
        }
        assert!(
            seen.iter().all(|entry| *entry),
            "ALL_KINDS is missing a ResourceKind variant"
        );
    }

    /// The defect this FR closes: eleven kinds shared the generic
    /// `resource.apply`, so an audit reader could not tell a SecretStore write
    /// from a Workspace edit. Distinctness is the property that failed, so it is
    /// the property asserted — a test that only checked "non-empty" would have
    /// passed against the broken code.
    #[test]
    fn every_kind_has_a_distinct_named_action() {
        let mut actions: Vec<&'static str> = ALL_KINDS.iter().copied().map(apply_action).collect();
        actions.sort_unstable();
        let total = actions.len();
        actions.dedup();
        assert_eq!(total, actions.len(), "two kinds share an audit action name");
        for kind in ALL_KINDS {
            let action = apply_action(kind);
            assert_ne!(
                action, "resource.apply",
                "{kind:?} still falls back to the generic action name"
            );
            assert!(
                action.ends_with(".apply"),
                "{kind:?} action {action} breaks the <domain>.<kind>.apply convention"
            );
        }
    }

    #[test]
    fn every_kind_has_a_distinct_target_type() {
        let mut types: Vec<&'static str> = ALL_KINDS
            .iter()
            .copied()
            .map(resource_target_type)
            .collect();
        types.sort_unstable();
        let total = types.len();
        types.dedup();
        assert_eq!(total, types.len(), "two kinds share an audit target_type");
        for kind in ALL_KINDS {
            assert_ne!(
                resource_target_type(kind),
                "resource",
                "{kind:?} still falls back to the generic target_type"
            );
        }
    }

    /// These two names predate the FR and appear in DD-111, QA 157 and stored
    /// audit rows. Renaming them for regularity would falsify those records, so
    /// the exception is pinned rather than left to judgement.
    #[test]
    fn shipped_source_action_names_are_unchanged() {
        assert_eq!(
            apply_action(ResourceKind::SourceTaskTemplate),
            "source.template.apply"
        );
        assert_eq!(
            apply_action(ResourceKind::SourceTaskBinding),
            "source.binding.apply"
        );
        assert_eq!(
            resource_target_type(ResourceKind::SourceTaskTemplate),
            "source_task_template"
        );
        assert_eq!(
            resource_target_type(ResourceKind::SourceTaskBinding),
            "source_task_binding"
        );
    }
}

#[cfg(test)]
mod delete_action_naming {
    use super::{apply_action, delete_action, resource_target_type};
    use agent_orchestrator::cli_types::ResourceKind;
    use agent_orchestrator::resource::ALL_RESOURCE_KINDS;

    /// The defect FR-167 closes: eleven of twelve kinds recorded no row at all,
    /// so there was nothing to tell a SecretStore removal from a Workspace one.
    /// Distinctness is the property that was missing, so it is the property
    /// asserted — "non-empty" would have passed against the broken code.
    #[test]
    fn every_kind_has_a_distinct_named_delete_action() {
        let mut actions: Vec<&'static str> = ALL_RESOURCE_KINDS
            .iter()
            .copied()
            .map(delete_action)
            .collect();
        actions.sort_unstable();
        let total = actions.len();
        actions.dedup();
        assert_eq!(total, actions.len(), "two kinds share a delete action name");
        for kind in ALL_RESOURCE_KINDS {
            let action = delete_action(kind);
            assert_ne!(
                action, "resource.delete",
                "{kind:?} still falls back to the generic delete action name"
            );
            assert!(
                action.ends_with(".delete"),
                "{kind:?} action {action} breaks the <domain>.<kind>.delete convention"
            );
        }
    }

    /// A delete name must not collide with an apply name. They share one column
    /// in `control_action_audit`, so a collision would make `--action` return
    /// both verbs and no filter could separate them again.
    #[test]
    fn delete_and_apply_names_never_collide() {
        let mut names: Vec<&'static str> = ALL_RESOURCE_KINDS
            .iter()
            .copied()
            .flat_map(|kind| [apply_action(kind), delete_action(kind)])
            .collect();
        names.push("resource.apply");
        names.push("resource.delete");
        names.push("delete_references");
        names.push("agent.driver.raw_args.apply");
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(
            total,
            names.len(),
            "an action name is claimed twice across the apply and delete surfaces"
        );
    }

    /// One vocabulary family per kind.
    ///
    /// This is why `SourceTaskTemplate` deletes as `source.template.delete`
    /// rather than the rule-literal `resource.source_task_template.delete`: with
    /// the families split, an auditor asking for everything about one source
    /// template would need two prefixes and would silently get half the story
    /// from either one. Asserted, because it is the kind of consistency that
    /// erodes the first time someone adds a kind in a hurry.
    #[test]
    fn apply_and_delete_share_a_family() {
        for kind in ALL_RESOURCE_KINDS {
            let apply = apply_action(kind);
            let delete = delete_action(kind);
            let family = |action: &'static str| {
                action
                    .rsplit_once('.')
                    .map(|(head, _)| head)
                    .unwrap_or(action)
            };
            assert_eq!(
                family(apply),
                family(delete),
                "{kind:?} applies as {apply} but deletes as {delete}; the two verbs must share a family"
            );
        }
    }

    /// `source.binding.delete` is already stored in `control_action_audit` and
    /// asserted by `scripts/qa/test-source-task-binding.sh`. Renaming it would
    /// falsify recorded history, so the spelling is pinned rather than left to
    /// the convention.
    #[test]
    fn shipped_delete_action_name_is_unchanged() {
        assert_eq!(
            delete_action(ResourceKind::SourceTaskBinding),
            "source.binding.delete"
        );
    }

    /// `target_type` is shared with apply because it names the object, not the
    /// verb. This pins the three values the delete path already stored, so the
    /// shared table cannot quietly move a delete row's target type.
    #[test]
    fn shared_target_types_match_the_values_delete_already_stored() {
        assert_eq!(
            resource_target_type(ResourceKind::SourceTaskBinding),
            "source_task_binding"
        );
        assert_eq!(
            resource_target_type(ResourceKind::SourceTaskTemplate),
            "source_task_template"
        );
        assert_eq!(resource_target_type(ResourceKind::Trigger), "trigger");
    }
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
