use crate::config_load::read_active_config;
use crate::error::{Result, classify_resource_error};
use crate::state::InnerState;

use super::format_output;

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
        "trigger" | "tg" => "Trigger",
        _ => {
            // CRD-defined custom resource fallback (skip kinds with dedicated ProjectConfig
            // projections — those are handled by the match arms above)
            if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(config, kind) {
                if !crate::crd::resolve::is_builtin_kind(&crd.kind) {
                    let storage_key = format!("{}/{}", crd.kind, name);
                    if let Some(cr) = config.custom_resources.get(&storage_key) {
                        return format_output(cr, output_format);
                    }
                    return Err(classify_resource_error(
                        "resource.get",
                        anyhow::anyhow!("{} not found: {}", crd.kind, name),
                    ));
                }
            }
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!("unknown resource type: {}", kind),
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
                    anyhow::anyhow!("workspace not found: {}", name),
                )
            })?;
            format_output(ws, output_format)
        }
        "wf" | "workflow" => {
            let wf = project.workflows.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("workflow not found: {}", name),
                )
            })?;
            format_output(wf, output_format)
        }
        "agent" => {
            let agent = project.agents.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("agent not found: {}", name),
                )
            })?;
            format_output(agent, output_format)
        }
        "trigger" | "tg" => {
            let trigger = project.triggers.get(name).ok_or_else(|| {
                classify_resource_error(
                    "resource.get",
                    anyhow::anyhow!("trigger not found: {}", name),
                )
            })?;
            format_output(trigger, output_format)
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
        "trigger" | "triggers" | "tg" => (project.triggers.keys().collect(), "Trigger"),
        _ => {
            // CRD-defined custom resource list fallback (skip kinds with dedicated ProjectConfig
            // projections — those are handled by the match arms above)
            if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(config, resource_type)
            {
                if !crate::crd::resolve::is_builtin_kind(&crd.kind) {
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
                                let storage_key = format!("{}{}", prefix, name);
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
            }
            return Err(classify_resource_error(
                "resource.get",
                anyhow::anyhow!("unknown list resource type: {}", resource_type),
            ));
        }
    };

    let filtered: Vec<&String> = if let Some(sel) = selector {
        let conditions = parse_label_selector(sel)?;
        names
            .into_iter()
            .filter(|name| {
                let labels = resource_store
                    .get_namespaced(crd_kind, project_id, name)
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
                anyhow::anyhow!("invalid label selector: '{}' (expected key=value)", part),
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
    get_resource(state, resource, None, output_format, project)
}
