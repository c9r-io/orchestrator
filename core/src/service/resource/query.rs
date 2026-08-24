use crate::config_load::read_active_config;
use crate::error::{Result, classify_resource_error};
use crate::resource::{
    AgentResource, EnvStoreResource, ExecutionProfileResource, ProjectResource, RegisteredResource,
    Resource, RuntimePolicyResource, SecretStoreResource, SourceTaskBindingResource,
    SourceTaskTemplateResource, StepTemplateResource, TriggerResource, WorkflowResource,
    WorkspaceResource,
};
use crate::state::InnerState;
use serde::{Deserialize, Serialize};

use super::format_output;

/// Stable daemon-owned metadata used by resource catalog consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSummary {
    /// Canonical manifest kind, for example `Workspace`.
    pub kind: String,
    /// Resource name within the project.
    pub name: String,
    /// Authoritative project scope.
    pub project_id: String,
    /// SHA-256 over the normalized current resource representation.
    pub revision: String,
    /// Projection that supplied the current resource.
    pub source: String,
}

/// One bounded, deterministically ordered resource catalog page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSummaryPage {
    /// Resource summaries in ascending name order.
    pub resources: Vec<ResourceSummary>,
    /// Last returned name when another page exists.
    pub next_cursor: Option<String>,
}

/// Get a resource by selector string. Returns serialized content.
pub fn get_resource(
    state: &InnerState,
    resource: &str,
    selector: Option<&str>,
    output_format: &str,
    project: Option<&str>,
) -> Result<String> {
    let active =
        read_active_config(state).map_err(|err| classify_resource_error("resource.get", err))?;
    let config = &active.config;
    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let empty_project = crate::config::ProjectConfig::default();
    let proj_cfg = config.projects.get(project_id).unwrap_or(&empty_project);

    if resource.contains('/') {
        if selector.is_some() {
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!(
                    "label selector (-l) cannot be used with a named resource; use it with list queries only"
                ),
            ));
        }
        let parts: Vec<&str> = resource.splitn(2, '/').collect();
        let (kind, name) = (parts[0], parts[1]);
        get_single_resource(
            proj_cfg,
            kind,
            name,
            output_format,
            project_id,
            &config.resource_store,
            config,
        )
    } else {
        get_list_resource(
            proj_cfg,
            resource,
            selector,
            output_format,
            project_id,
            &config.resource_store,
            config,
        )
    }
}

fn get_single_resource(
    project: &crate::config::ProjectConfig,
    kind: &str,
    name: &str,
    output_format: &str,
    project_id: &str,
    resource_store: &crate::crd::store::ResourceStore,
    config: &crate::config::OrchestratorConfig,
) -> Result<String> {
    let crd_kind = match kind {
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
        _ => {
            // CRD-defined custom resource fallback (skip kinds with dedicated ProjectConfig
            // projections — those are handled by the match arms above)
            if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(config, kind)
                && !crate::crd::resolve::is_builtin_kind(&crd.kind)
            {
                let storage_key = format!("{}/{}", crd.kind, name);
                if let Some(cr) = config.custom_resources.get(&storage_key) {
                    return format_output(cr, output_format);
                }
                return Err(classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("{} not found: {}", crd.kind, name),
                ));
            }
            // Project, RuntimePolicy, EnvStore and SecretStore have a typed
            // reader but no store-based rendering above. Serving them from the
            // same function `describe` calls is what makes `get kind/name` and
            // `describe kind/name` agree byte for byte — a property this arm
            // gets for free and a duplicated kind list here would not.
            //
            // The kind has to be resolved *separately* from the lookup, because
            // `describe_builtin_resource` returns `Ok(None)` both for a kind it
            // does not know and for a known kind whose instance is absent.
            // Asking it alone reports a missing instance as an unknown type,
            // which is the diagnostic conflation FR-171 requirement 3 is about.
            // The builtin CRD registry already carries every alias, so the
            // singular set is derived from it rather than repeated here.
            //
            // Plural forms are excluded deliberately: `get workspaces/foo` is a
            // malformed query, and answering it with `Workspace not found: foo`
            // would assert something about `foo` that was never looked up.
            if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(config, kind) {
                let singular = crd.kind.eq_ignore_ascii_case(kind)
                    || crd
                        .short_names
                        .iter()
                        .any(|short| short.eq_ignore_ascii_case(kind));
                if singular {
                    if let Some(content) = describe_builtin_resource(
                        config,
                        kind,
                        name,
                        output_format,
                        Some(project_id),
                    )? {
                        return Ok(content);
                    }
                    return Err(classify_resource_error(
                        "resource.get",
                        anyhow::anyhow!("{} not found: {}", crd.kind, name),
                    ));
                }
            }
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!("unknown resource type: {kind}"),
            ));
        }
    };

    // Try to serve from the resource_store first (includes metadata with labels).
    if let Some(cr) = resource_store.get_namespaced(crd_kind, project_id, name) {
        return format_output(&cr, output_format);
    }

    // Fallback: serve from the in-memory config (without labels/annotations).
    match kind {
        "ws" | "workspace" => {
            let ws = project.workspaces.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("workspace not found: {name}"),
                )
            })?;
            format_output(ws, output_format)
        }
        "wf" | "workflow" => {
            let wf = project.workflows.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("workflow not found: {name}"),
                )
            })?;
            format_output(wf, output_format)
        }
        "agent" => {
            let agent = project.agents.get(name).ok_or_else(|| {
                classify_resource_error("resource.get", anyhow::anyhow!("agent not found: {name}"))
            })?;
            format_output(agent, output_format)
        }
        "steptemplate" | "step-template" | "step_template" => {
            let template = project.step_templates.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("step template not found: {name}"),
                )
            })?;
            format_output(template, output_format)
        }
        "executionprofile" | "execution-profile" | "execution_profile" => {
            let profile = project.execution_profiles.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("execution profile not found: {name}"),
                )
            })?;
            format_output(profile, output_format)
        }
        "trigger" | "tg" => {
            let trigger = project.triggers.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("trigger not found: {name}"),
                )
            })?;
            format_output(trigger, output_format)
        }
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt" => {
            let template = project.source_task_templates.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("source task template not found: {name}"),
                )
            })?;
            format_output(template, output_format)
        }
        "sourcetaskbinding" | "source-task-binding" | "source_task_binding" | "stb" => {
            let binding = project.source_task_bindings.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("source task binding not found: {name}"),
                )
            })?;
            format_output(binding, output_format)
        }
        _ => unreachable!(),
    }
}

fn get_list_resource(
    project: &crate::config::ProjectConfig,
    resource_type: &str,
    selector: Option<&str>,
    output_format: &str,
    project_id: &str,
    resource_store: &crate::crd::store::ResourceStore,
    config: &crate::config::OrchestratorConfig,
) -> Result<String> {
    let (names, crd_kind): (Vec<&String>, &str) = match resource_type {
        "ws" | "workspace" | "workspaces" => (project.workspaces.keys().collect(), "Workspace"),
        "agent" | "agents" => (project.agents.keys().collect(), "Agent"),
        "wf" | "workflow" | "workflows" => (project.workflows.keys().collect(), "Workflow"),
        "steptemplate" | "step-template" | "step_template" | "steptemplates" => {
            (project.step_templates.keys().collect(), "StepTemplate")
        }
        "executionprofile" | "execution-profile" | "execution_profile" | "executionprofiles" => (
            project.execution_profiles.keys().collect(),
            "ExecutionProfile",
        ),
        "trigger" | "triggers" | "tg" => (project.triggers.keys().collect(), "Trigger"),
        "sourcetasktemplate"
        | "source-task-template"
        | "source_task_template"
        | "sourcetasktemplates"
        | "stt" => (
            project.source_task_templates.keys().collect(),
            "SourceTaskTemplate",
        ),
        "sourcetaskbinding"
        | "source-task-binding"
        | "source_task_binding"
        | "sourcetaskbindings"
        | "stb" => (
            project.source_task_bindings.keys().collect(),
            "SourceTaskBinding",
        ),
        "envstore" | "env-store" | "env_store" | "envstores" => {
            (project.env_stores.keys().collect(), "EnvStore")
        }
        // Listing a SecretStore yields names only — this function formats
        // `filtered`, which is a Vec of names, and never touches a spec. The
        // values live in `get_single_resource`, which redacts them.
        "secretstore" | "secret-store" | "secret_store" | "secretstores" => {
            (project.secret_stores.keys().collect(), "SecretStore")
        }
        // Project is the one kind that is not project-scoped, so it is read
        // from the whole config rather than from `project`, and `--project`
        // does not narrow it. The empty name is skipped for the same reason
        // `export_manifest_resources` skips it: a blank project id is a
        // structural artefact, not a project someone created.
        "project" | "projects" => (
            config
                .projects
                .keys()
                .filter(|name| !name.is_empty())
                .collect(),
            "Project",
        ),
        _ => {
            // CRD-defined custom resource list fallback (skip kinds with dedicated ProjectConfig
            // projections — those are handled by the match arms above)
            if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(config, resource_type)
                && !crate::crd::resolve::is_builtin_kind(&crd.kind)
            {
                let prefix = format!("{}/", crd.kind);
                let cr_names: Vec<String> = config
                    .custom_resources
                    .keys()
                    .filter(|key| key.starts_with(&prefix))
                    .map(|key| key[prefix.len()..].to_string())
                    .collect();

                let filtered: Vec<&String> = if let Some(sel) = selector {
                    let conditions = parse_label_selector(sel)?;
                    cr_names
                        .iter()
                        .filter(|name| {
                            let storage_key = format!("{prefix}{name}");
                            let labels = config
                                .custom_resources
                                .get(&storage_key)
                                .and_then(|cr| cr.metadata.labels.as_ref());
                            match_labels(labels, &conditions)
                        })
                        .collect()
                } else {
                    cr_names.iter().collect()
                };

                return format_output(&filtered, output_format);
            }
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!("unknown list resource type: {resource_type}"),
            ));
        }
    };

    // `get_namespaced` is a raw `{kind}/{project}/{name}` lookup and does not
    // resolve scope itself. Every kind above except Project is project-scoped;
    // Project is stored under `_system`, so looking it up under `project_id`
    // would find nothing and silently drop every row from a label query.
    let store_project = if crate::crd::store::is_project_scoped(crd_kind) {
        project_id
    } else {
        crate::crd::store::SYSTEM_PROJECT
    };

    let filtered: Vec<&String> = if let Some(sel) = selector {
        let conditions = parse_label_selector(sel)?;
        names
            .into_iter()
            .filter(|name| {
                let labels = resource_store
                    .get_namespaced(crd_kind, store_project, name)
                    .and_then(|cr| cr.metadata.labels.as_ref());
                match_labels(labels, &conditions)
            })
            .collect()
    } else {
        names
    };

    format_output(&filtered, output_format)
}

/// Parse a label selector string like "env=dev,tier=qa" into key-value pairs.
pub(super) fn parse_label_selector(selector: &str) -> Result<Vec<(String, String)>> {
    let mut conditions = Vec::new();
    for part in selector.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let kv: Vec<&str> = part.splitn(2, '=').collect();
        if kv.len() != 2 {
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!("invalid label selector: '{part}' (expected key=value)"),
            ));
        }
        conditions.push((kv[0].to_string(), kv[1].to_string()));
    }
    Ok(conditions)
}

/// Check if a resource's labels match all selector conditions (AND logic).
pub(super) fn match_labels(
    labels: Option<&std::collections::HashMap<String, String>>,
    conditions: &[(String, String)],
) -> bool {
    let Some(labels) = labels else {
        return conditions.is_empty();
    };
    conditions
        .iter()
        .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
}

/// Describe a resource (detailed view).
pub fn describe_resource(
    state: &InnerState,
    resource: &str,
    output_format: &str,
    project: Option<&str>,
) -> Result<String> {
    if let Some((kind, name)) = resource.split_once('/') {
        let active = read_active_config(state)
            .map_err(|err| classify_resource_error("resource.describe", err))?;
        if let Some(content) =
            describe_builtin_resource(&active.config, kind, name, output_format, project)?
        {
            return Ok(content);
        }
    }
    get_resource(state, resource, None, output_format, project)
}

fn describe_builtin_resource(
    config: &crate::config::OrchestratorConfig,
    kind: &str,
    name: &str,
    output_format: &str,
    project: Option<&str>,
) -> Result<Option<String>> {
    let resource = match kind {
        "ws" | "workspace" => WorkspaceResource::get_from_project(config, name, project)
            .map(RegisteredResource::Workspace),
        "agent" => AgentResource::get_from_project(config, name, project)
            .map(Box::new)
            .map(RegisteredResource::Agent),
        "wf" | "workflow" => WorkflowResource::get_from_project(config, name, project)
            .map(RegisteredResource::Workflow),
        "steptemplate" | "step-template" | "step_template" => {
            StepTemplateResource::get_from_project(config, name, project)
                .map(RegisteredResource::StepTemplate)
        }
        "executionprofile" | "execution-profile" | "execution_profile" => {
            ExecutionProfileResource::get_from_project(config, name, project)
                .map(RegisteredResource::ExecutionProfile)
        }
        "envstore" | "env-store" | "env_store" => {
            EnvStoreResource::get_from_project(config, name, project)
                .map(RegisteredResource::EnvStore)
        }
        // The in-memory config holds decrypted SecretStore values — the load
        // path runs `decrypt_resource_spec_json` over every resource — so this
        // is the point where they must not escape. Reads render the same
        // placeholder `RedactedConfig` writes into a snapshot and into an
        // export; the values are reachable only through env injection, never
        // through a read command.
        "secretstore" | "secret-store" | "secret_store" => {
            SecretStoreResource::get_from_project(config, name, project).map(|mut store| {
                for value in store.spec.data.values_mut() {
                    *value = crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER.to_string();
                }
                RegisteredResource::SecretStore(store)
            })
        }
        "project" => ProjectResource::get_from_project(config, name, project)
            .map(RegisteredResource::Project),
        // RuntimePolicy is a resolved singleton rather than a stored row:
        // `get_from_project` returns the effective policy for any name, walking
        // project -> `_system` -> defaults. That is why it answers a single
        // read and is deliberately absent from `get_list_resource`.
        "runtimepolicy" | "runtime-policy" => {
            RuntimePolicyResource::get_from_project(config, name, project)
                .map(RegisteredResource::RuntimePolicy)
        }
        _ => return Ok(None),
    };
    resource
        .map(|resource| {
            let yaml = resource
                .to_yaml()
                .map_err(|err| classify_resource_error("resource.describe", err))?;
            match output_format {
                "yaml" => Ok(yaml),
                "json" | "table" => {
                    let value: serde_yaml::Value = serde_yaml::from_str(&yaml)
                        .map_err(|err| classify_resource_error("resource.describe", err))?;
                    serde_json::to_string_pretty(&value)
                        .map_err(|err| classify_resource_error("resource.describe", err))
                }
                _ => Ok(yaml),
            }
        })
        .transpose()
}

/// Return a bounded catalog page without asking callers to parse serialized output.
pub fn list_resource_summaries(
    state: &InnerState,
    resource_type: &str,
    project: Option<&str>,
    cursor: Option<&str>,
    limit: usize,
) -> Result<ResourceSummaryPage> {
    let active =
        read_active_config(state).map_err(|err| classify_resource_error("resource.list", err))?;
    let config = &active.config;
    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let empty_project = crate::config::ProjectConfig::default();
    let project = config.projects.get(project_id).unwrap_or(&empty_project);
    let (kind, query_kind, mut names): (&str, &str, Vec<String>) = match resource_type {
        "ws" | "workspace" | "workspaces" => (
            "Workspace",
            "workspace",
            project.workspaces.keys().cloned().collect(),
        ),
        "wf" | "workflow" | "workflows" => (
            "Workflow",
            "workflow",
            project.workflows.keys().cloned().collect(),
        ),
        "agent" | "agents" => ("Agent", "agent", project.agents.keys().cloned().collect()),
        "steptemplate" | "step-template" | "step_template" | "steptemplates" => (
            "StepTemplate",
            "steptemplate",
            project.step_templates.keys().cloned().collect(),
        ),
        "executionprofile" | "execution-profile" | "execution_profile" | "executionprofiles" => (
            "ExecutionProfile",
            "executionprofile",
            project.execution_profiles.keys().cloned().collect(),
        ),
        "envstore" | "env-store" | "env_store" | "envstores" => (
            "EnvStore",
            "envstore",
            project.env_stores.keys().cloned().collect(),
        ),
        // Names only, like every other row here: the page carries name, revision
        // and source, never a spec. A single read of one of these redacts.
        "secretstore" | "secret-store" | "secret_store" | "secretstores" => (
            "SecretStore",
            "secretstore",
            project.secret_stores.keys().cloned().collect(),
        ),
        // Project is not project-scoped, so its names come from the whole config
        // and `--project` does not narrow the page.
        "project" | "projects" => (
            "Project",
            "project",
            config
                .projects
                .keys()
                .filter(|name| !name.is_empty())
                .cloned()
                .collect(),
        ),
        // This catalog covers the kinds with a typed renderer, because it renders
        // every row through `describe_builtin_resource` and treats `None` as a
        // missing resource. That is eight of twelve: Trigger, SourceTaskTemplate
        // and SourceTaskBinding are readable through `get` but have no typed
        // renderer, and adding one changes how `describe` renders them — the
        // rendering convergence FR-171 deliberately left out of scope. RuntimePolicy
        // is absent for the reason it is absent from `get_list_resource`: it is a
        // resolved singleton, not a collection.
        //
        // The message says which of those two reasons applies rather than
        // reporting every unsupported kind as bad input, which is what
        // "unsupported expert resource catalog type" did for all seven.
        other => {
            // Two registries are consulted because neither covers all twelve
            // kinds. `find_crd_by_kind_or_alias` yields the canonical kind name
            // but has no definition for Trigger; `is_builtin_alias` knows every
            // kind's singular and plural but returns only a bool. So a Trigger
            // query is correctly told *why* it is refused and cannot be told the
            // canonical spelling — a user-visible consequence of the asymmetry,
            // recorded rather than papered over.
            let canonical = crate::crd::resolve::find_crd_by_kind_or_alias(config, other)
                .map(|crd| crd.kind.clone());
            let builtin_name = canonical.is_some() || crate::crd::resolve::is_builtin_alias(other);
            return Err(classify_resource_error(
                "resource.list",
                match canonical.as_deref() {
                    Some("RuntimePolicy") => anyhow::anyhow!(
                        "RuntimePolicy is a resolved singleton, not a collection; read it with `get runtimepolicy/<name>`"
                    ),
                    Some(kind) => anyhow::anyhow!(
                        "{kind} is not in the resource catalog: it has no typed renderer, so it is readable through `get {other}/<name>` but not browsable here"
                    ),
                    None if builtin_name => anyhow::anyhow!(
                        "{other} names a builtin resource with no typed renderer, so it is readable through `get` but not browsable here"
                    ),
                    None => anyhow::anyhow!("unknown resource catalog type: {other}"),
                },
            ));
        }
    };
    names.sort();
    if let Some(cursor) = cursor {
        names.retain(|name| name.as_str() > cursor);
    }

    let page_limit = limit.clamp(1, 500);
    let has_more = names.len() > page_limit;
    names.truncate(page_limit);
    let mut resources = Vec::with_capacity(names.len());
    for name in names {
        let content =
            describe_builtin_resource(config, query_kind, &name, "yaml", Some(project_id))?
                .ok_or_else(|| {
                    classify_resource_error(
                        "resource.list",
                        anyhow::anyhow!("{kind} not found: {name}"),
                    )
                })?;
        let revision = resource_content_revision(&content)?;
        let source = if config
            .resource_store
            .get_namespaced(kind, project_id, &name)
            .is_some()
        {
            "resource_store"
        } else {
            "active_config"
        };
        resources.push(ResourceSummary {
            kind: kind.to_string(),
            name,
            project_id: project_id.to_string(),
            revision,
            source: source.to_string(),
        });
    }
    let next_cursor = has_more
        .then(|| resources.last().map(|resource| resource.name.clone()))
        .flatten();
    Ok(ResourceSummaryPage {
        resources,
        next_cursor,
    })
}

/// Compute the current stable revision for a builtin resource, if it exists.
pub fn current_resource_revision(
    state: &InnerState,
    kind: crate::cli_types::ResourceKind,
    name: &str,
    project: Option<&str>,
) -> Result<Option<String>> {
    let active = read_active_config(state)
        .map_err(|err| classify_resource_error("resource.revision", err))?;
    let config = &active.config;
    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let empty_project = crate::config::ProjectConfig::default();
    let project_config = config.projects.get(project_id).unwrap_or(&empty_project);
    if kind == crate::cli_types::ResourceKind::SourceTaskTemplate {
        return project_config
            .source_task_templates
            .get(name)
            .map(crate::source_task_template::template_content_hash)
            .transpose()
            .map_err(|err| classify_resource_error("resource.revision", err));
    }
    if kind == crate::cli_types::ResourceKind::SourceTaskBinding {
        return project_config
            .source_task_bindings
            .get(name)
            .map(crate::source_task_binding::binding_content_hash)
            .transpose()
            .map_err(|err| classify_resource_error("resource.revision", err));
    }
    let query_kind = match kind {
        crate::cli_types::ResourceKind::Workspace => Some("workspace"),
        crate::cli_types::ResourceKind::Agent => Some("agent"),
        crate::cli_types::ResourceKind::Workflow => Some("workflow"),
        crate::cli_types::ResourceKind::StepTemplate => Some("steptemplate"),
        crate::cli_types::ResourceKind::ExecutionProfile => Some("executionprofile"),
        crate::cli_types::ResourceKind::Trigger => Some("trigger"),
        crate::cli_types::ResourceKind::SourceTaskTemplate
        | crate::cli_types::ResourceKind::SourceTaskBinding => None,
        _ => None,
    };
    if let Some(query_kind) = query_kind {
        let exists = match kind {
            crate::cli_types::ResourceKind::Workspace => {
                project_config.workspaces.contains_key(name)
            }
            crate::cli_types::ResourceKind::Agent => project_config.agents.contains_key(name),
            crate::cli_types::ResourceKind::Workflow => project_config.workflows.contains_key(name),
            crate::cli_types::ResourceKind::StepTemplate => {
                project_config.step_templates.contains_key(name)
            }
            crate::cli_types::ResourceKind::SourceTaskTemplate => {
                project_config.source_task_templates.contains_key(name)
            }
            crate::cli_types::ResourceKind::SourceTaskBinding => {
                project_config.source_task_bindings.contains_key(name)
            }
            crate::cli_types::ResourceKind::ExecutionProfile => {
                project_config.execution_profiles.contains_key(name)
            }
            crate::cli_types::ResourceKind::Trigger => project_config.triggers.contains_key(name),
            _ => false,
        };
        if !exists {
            return Ok(None);
        }
        let content =
            describe_builtin_resource(config, query_kind, name, "yaml", Some(project_id))?
                .ok_or_else(|| {
                    classify_resource_error(
                        "resource.revision",
                        anyhow::anyhow!("{query_kind} not found: {name}"),
                    )
                })?;
        return resource_content_revision(&content).map(Some);
    }
    let resource = match kind {
        crate::cli_types::ResourceKind::Workspace => {
            WorkspaceResource::get_from_project(config, name, project)
                .map(RegisteredResource::Workspace)
        }
        crate::cli_types::ResourceKind::Agent => {
            AgentResource::get_from_project(config, name, project)
                .map(Box::new)
                .map(RegisteredResource::Agent)
        }
        crate::cli_types::ResourceKind::Workflow => {
            WorkflowResource::get_from_project(config, name, project)
                .map(RegisteredResource::Workflow)
        }
        crate::cli_types::ResourceKind::Project => {
            ProjectResource::get_from_project(config, name, project)
                .map(RegisteredResource::Project)
        }
        crate::cli_types::ResourceKind::RuntimePolicy => {
            RuntimePolicyResource::get_from_project(config, name, project)
                .map(RegisteredResource::RuntimePolicy)
        }
        crate::cli_types::ResourceKind::StepTemplate => {
            StepTemplateResource::get_from_project(config, name, project)
                .map(RegisteredResource::StepTemplate)
        }
        crate::cli_types::ResourceKind::SourceTaskTemplate => {
            SourceTaskTemplateResource::get_from_project(config, name, project)
                .map(RegisteredResource::SourceTaskTemplate)
        }
        crate::cli_types::ResourceKind::SourceTaskBinding => {
            SourceTaskBindingResource::get_from_project(config, name, project)
                .map(RegisteredResource::SourceTaskBinding)
        }
        crate::cli_types::ResourceKind::ExecutionProfile => {
            ExecutionProfileResource::get_from_project(config, name, project)
                .map(RegisteredResource::ExecutionProfile)
        }
        crate::cli_types::ResourceKind::EnvStore => {
            EnvStoreResource::get_from_project(config, name, project)
                .map(RegisteredResource::EnvStore)
        }
        crate::cli_types::ResourceKind::SecretStore => {
            SecretStoreResource::get_from_project(config, name, project)
                .map(RegisteredResource::SecretStore)
        }
        crate::cli_types::ResourceKind::Trigger => {
            TriggerResource::get_from_project(config, name, project)
                .map(RegisteredResource::Trigger)
        }
    };
    resource
        .map(|resource| {
            resource
                .to_yaml()
                .map_err(|err| classify_resource_error("resource.revision", err))
                .and_then(|content| resource_content_revision(&content))
        })
        .transpose()
}

/// Hash a serialized resource after normalizing map key ordering.
pub fn resource_content_revision(content: &str) -> Result<String> {
    let value: serde_yaml::Value = serde_yaml::from_str(content)
        .map_err(|err| classify_resource_error("resource.revision", err))?;
    let value = serde_json::to_value(value)
        .map_err(|err| classify_resource_error("resource.revision", err))?;
    crate::action_audit::canonical_request_hash(&value)
        .map_err(|err| classify_resource_error("resource.revision", err))
}
