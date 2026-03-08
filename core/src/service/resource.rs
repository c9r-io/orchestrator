use crate::config_load::{
    load_raw_config_from_db, persist_config_and_reload, persist_raw_config, read_active_config,
};
use crate::crd::{self, ParsedManifest};
use crate::resource::{
    apply_to_project, delete_resource_by_kind, dispatch_resource, kind_as_str,
    parse_manifests_from_yaml, ApplyResult, Resource,
};
use crate::state::InnerState;
use anyhow::{Context, Result};
use std::collections::BTreeSet;

/// Apply manifest content. Returns an ApplyResponse proto.
pub fn apply_manifests(
    state: &InnerState,
    content: &str,
    dry_run: bool,
    project: Option<&str>,
) -> Result<orchestrator_proto::ApplyResponse> {
    let db_path = &state.db_path;
    let manifests = parse_manifests_from_yaml(content)
        .map_err(|e| anyhow::anyhow!("parse error: {}", e))?;

    let mut merged_config = load_raw_config_from_db(db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut errors = Vec::new();

    for (index, manifest) in manifests.into_iter().enumerate() {
        match manifest {
            ParsedManifest::Builtin(resource) => {
                if let Err(error) = resource.validate_version() {
                    errors.push(format!("document {}: {}", index + 1, error));
                    continue;
                }
                let registered = match dispatch_resource(resource) {
                    Ok(r) => r,
                    Err(error) => {
                        errors.push(format!("document {}: {}", index + 1, error));
                        continue;
                    }
                };
                if let Err(error) = registered.validate() {
                    errors.push(format!(
                        "{} / {} invalid: {}",
                        kind_as_str(registered.kind()),
                        registered.name(),
                        error
                    ));
                    continue;
                }
                let effective_project = project.or_else(|| registered.metadata_project());
                let result = if let Some(proj) = effective_project {
                    apply_to_project(&registered, &mut merged_config, proj)?
                } else {
                    registered.apply(&mut merged_config)?
                };
                let action = match result {
                    ApplyResult::Created => "created",
                    ApplyResult::Configured => "updated",
                    ApplyResult::Unchanged => "unchanged",
                };
                results.push(orchestrator_proto::ApplyResultEntry {
                    kind: kind_as_str(registered.kind()).to_string(),
                    name: registered.name().to_string(),
                    action: action.to_string(),
                    project_scope: effective_project.map(|s| s.to_string()),
                });
            }
            ParsedManifest::Crd(crd_manifest) => {
                let crd_name = crd_manifest.metadata.name.clone();
                let crd_kind = crd_manifest.spec.kind.clone();
                match crd::apply_crd(&mut merged_config, crd_manifest) {
                    Ok(result) => {
                        let action = match result {
                            ApplyResult::Created => "created",
                            ApplyResult::Configured => "updated",
                            ApplyResult::Unchanged => "unchanged",
                        };
                        results.push(orchestrator_proto::ApplyResultEntry {
                            kind: format!("crd({})", crd_kind),
                            name: crd_name,
                            action: action.to_string(),
                            project_scope: None,
                        });
                    }
                    Err(error) => {
                        errors.push(format!("document {} (CRD {}): {}", index + 1, crd_name, error));
                    }
                }
            }
            ParsedManifest::Custom(cr_manifest) => {
                let cr_kind = cr_manifest.kind.clone();
                let cr_name = cr_manifest.metadata.name.clone();
                match crd::apply_custom_resource(&mut merged_config, cr_manifest) {
                    Ok(result) => {
                        let action = match result {
                            ApplyResult::Created => "created",
                            ApplyResult::Configured => "updated",
                            ApplyResult::Unchanged => "unchanged",
                        };
                        let display_kind = crd::crd_kind_display(&cr_kind);
                        results.push(orchestrator_proto::ApplyResultEntry {
                            kind: display_kind,
                            name: cr_name,
                            action: action.to_string(),
                            project_scope: None,
                        });
                    }
                    Err(error) => {
                        errors.push(format!("document {} ({}/{}): {}", index + 1, cr_kind, cr_name, error));
                    }
                }
            }
        }
    }

    let config_version = if !dry_run && !results.is_empty() && errors.is_empty() {
        autofill_defaults_for_manifest_mode(&mut merged_config);
        let overview = persist_raw_config(db_path, merged_config, "daemon-apply")?;
        Some(overview.version)
    } else {
        None
    };

    Ok(orchestrator_proto::ApplyResponse {
        results,
        config_version,
        errors,
    })
}

/// Get a resource by selector string. Returns serialized content.
pub fn get_resource(
    state: &InnerState,
    resource: &str,
    selector: Option<&str>,
    output_format: &str,
) -> Result<String> {
    let active = read_active_config(state)?;
    // Delegate to a JSON serialization of the resource data
    // For simplicity, serialize to the requested format
    let config = &active.config;

    if resource.contains('/') {
        // Single resource get
        let parts: Vec<&str> = resource.splitn(2, '/').collect();
        let (kind, name) = (parts[0], parts[1]);
        get_single_resource(config, kind, name, output_format)
    } else {
        get_list_resource(config, resource, selector, output_format)
    }
}

fn get_single_resource(
    config: &crate::config::OrchestratorConfig,
    kind: &str,
    name: &str,
    output_format: &str,
) -> Result<String> {
    match kind {
        "ws" | "workspace" => {
            let ws = config
                .workspaces
                .get(name)
                .context(format!("workspace not found: {}", name))?;
            format_output(ws, output_format)
        }
        "wf" | "workflow" => {
            let wf = config
                .workflows
                .get(name)
                .context(format!("workflow not found: {}", name))?;
            format_output(wf, output_format)
        }
        "agent" => {
            let agent = config
                .agents
                .get(name)
                .context(format!("agent not found: {}", name))?;
            format_output(agent, output_format)
        }
        _ => anyhow::bail!("unknown resource type: {}", kind),
    }
}

fn get_list_resource(
    config: &crate::config::OrchestratorConfig,
    resource_type: &str,
    _selector: Option<&str>,
    output_format: &str,
) -> Result<String> {
    match resource_type {
        "ws" | "workspace" | "workspaces" => {
            let names: Vec<&String> = config.workspaces.keys().collect();
            format_output(&names, output_format)
        }
        "agent" | "agents" => {
            let names: Vec<&String> = config.agents.keys().collect();
            format_output(&names, output_format)
        }
        "wf" | "workflow" | "workflows" => {
            let names: Vec<&String> = config.workflows.keys().collect();
            format_output(&names, output_format)
        }
        _ => anyhow::bail!("unknown list resource type: {}", resource_type),
    }
}

/// Describe a resource (detailed view).
pub fn describe_resource(
    state: &InnerState,
    resource: &str,
    output_format: &str,
) -> Result<String> {
    // For now, describe is identical to get with yaml default
    get_resource(state, resource, None, output_format)
}

/// Delete a resource by kind/name.
pub fn delete_resource(state: &InnerState, resource: &str, force: bool) -> Result<()> {
    let parts: Vec<&str> = resource.split('/').collect();
    if parts.len() != 2 {
        anyhow::bail!("invalid resource format: {} (use kind/name)", resource);
    }
    let (kind, name) = (parts[0], parts[1]);

    if !force {
        anyhow::bail!("use --force to confirm deletion of {}/{}", kind, name);
    }

    let mut config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    if (kind == "ws" || kind == "workspace") && config.defaults.workspace == name {
        anyhow::bail!(
            "cannot delete workspace '{}': it is the current default workspace",
            name
        );
    }
    if (kind == "wf" || kind == "workflow") && config.defaults.workflow == name {
        anyhow::bail!(
            "cannot delete workflow '{}': it is the current default workflow",
            name
        );
    }

    let deleted = delete_resource_by_kind(&mut config, kind, name)?;
    if !deleted {
        anyhow::bail!("{}/{} not found", kind, name);
    }

    let yaml =
        serde_yml::to_string(&config).context("failed to serialize configuration after delete")?;
    persist_config_and_reload(state, config, yaml, "daemon")?;
    Ok(())
}

fn format_output<T: serde::Serialize>(value: &T, format: &str) -> Result<String> {
    match format {
        "json" => Ok(serde_json::to_string_pretty(value)?),
        "yaml" => Ok(serde_yml::to_string(value)?),
        "table" => Ok(serde_json::to_string_pretty(value)?), // fallback
        _ => Ok(serde_json::to_string_pretty(value)?),
    }
}

fn autofill_defaults_for_manifest_mode(config: &mut crate::config::OrchestratorConfig) {
    if config.defaults.project.trim().is_empty() {
        config.defaults.project = "default".to_string();
    }
    if config.defaults.workspace.trim().is_empty() {
        if config.workspaces.contains_key("default") {
            config.defaults.workspace = "default".to_string();
        } else {
            let workspaces: BTreeSet<_> = config.workspaces.keys().cloned().collect();
            if let Some(first) = workspaces.into_iter().next() {
                config.defaults.workspace = first;
            }
        }
    }
    if config.defaults.workflow.trim().is_empty() {
        if config.workflows.contains_key("qa_only") {
            config.defaults.workflow = "qa_only".to_string();
        } else {
            let workflows: BTreeSet<_> = config.workflows.keys().cloned().collect();
            if let Some(first) = workflows.into_iter().next() {
                config.defaults.workflow = first;
            }
        }
    }
}
