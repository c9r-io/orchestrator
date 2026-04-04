mod delete;
mod query;
mod trigger_ops;

#[cfg(test)]
mod tests;

// Re-export public API (preserves agent_orchestrator::service::resource::* paths)
pub use delete::delete_resource;
pub use query::{describe_resource, get_resource};
pub use trigger_ops::{fire_trigger, resume_trigger, suspend_trigger};

// Re-import private helpers for test visibility via `use super::*`
#[cfg(test)]
use delete::{canonical_project_kind, delete_resource_from_project};
#[cfg(test)]
use query::{match_labels, parse_label_selector};

use crate::config_load::{
    ResourceRemoval, enforce_deletion_guards_for_removals, load_config, persist_config_and_reload,
    read_active_config,
};
use crate::crd::{self, ParsedManifest};
use crate::error::{Result, classify_resource_error};
use crate::resource::{
    ApplyResult, Resource, apply_to_project, dispatch_resource, kind_as_str,
    parse_manifests_from_yaml,
};
use crate::state::InnerState;
use anyhow::Context;
use std::collections::{HashMap, HashSet};

/// Apply manifest content. Returns an ApplyResponse proto.
pub fn apply_manifests(
    state: &InnerState,
    content: &str,
    dry_run: bool,
    project: Option<&str>,
    prune: bool,
) -> Result<orchestrator_proto::ApplyResponse> {
    let db_path = &state.db_path;
    let manifests = parse_manifests_from_yaml(content).map_err(|e| {
        classify_resource_error("resource.apply", anyhow::anyhow!("parse error: {}", e))
    })?;

    let current_config = load_config(db_path)
        .map_err(|err| classify_resource_error("resource.apply", err))?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();
    let mut merged_config = current_config.clone();

    let mut results = Vec::new();
    let mut errors = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut prunable_manifest_names: HashMap<&'static str, HashSet<String>> = HashMap::new();

    let cli_project = project
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    for (index, manifest) in manifests.into_iter().enumerate() {
        match manifest {
            ParsedManifest::Builtin(resource) => {
                if let Err(error) = resource.validate_version() {
                    errors.push(format!("document {}: {}", index + 1, error));
                    continue;
                }
                let registered = match dispatch_resource(*resource) {
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
                // Collect warnings for workflow resources (unknown fields, uncaptured vars)
                if let crate::resource::RegisteredResource::Workflow(ref wf) = registered {
                    warnings.extend(wf.collect_warnings());
                }
                if let Some(meta_project) = registered.metadata_project() {
                    if meta_project != cli_project {
                        errors.push(format!(
                            "{} / {} project mismatch: --project={} but metadata.project={}",
                            kind_as_str(registered.kind()),
                            registered.name(),
                            cli_project,
                            meta_project
                        ));
                        continue;
                    }
                }
                let result = apply_to_project(&registered, &mut merged_config, cli_project)
                    .map_err(|err| classify_resource_error("resource.apply", err))?;
                if let Some(kind) = prunable_resource_kind(&registered) {
                    prunable_manifest_names
                        .entry(kind)
                        .or_default()
                        .insert(registered.name().to_string());
                }
                let action = apply_action_label(result);
                results.push(orchestrator_proto::ApplyResultEntry {
                    kind: kind_as_str(registered.kind()).to_string(),
                    name: registered.name().to_string(),
                    action: action.to_string(),
                    project_scope: Some(cli_project.to_string()),
                });
            }
            ParsedManifest::Crd(crd_manifest) => {
                let crd_name = crd_manifest.metadata.name.clone();
                let crd_kind = crd_manifest.spec.kind.clone();
                let plugins_snapshot: Vec<_> = crd_manifest
                    .spec
                    .plugins
                    .iter()
                    .map(|p| (p.name.clone(), p.plugin_type.clone(), p.command.clone()))
                    .collect();
                let hooks_snapshot: Vec<_> = [
                    ("on_create", &crd_manifest.spec.hooks.on_create),
                    ("on_update", &crd_manifest.spec.hooks.on_update),
                    ("on_delete", &crd_manifest.spec.hooks.on_delete),
                ]
                .iter()
                .filter_map(|(label, cmd)| cmd.as_ref().map(|c| (label.to_string(), c.clone())))
                .collect();

                let policy_mode = format!("{:?}", state.plugin_policy.mode).to_lowercase();
                match crd::apply_crd(&mut merged_config, crd_manifest, &state.plugin_policy) {
                    Ok(result) => {
                        // Audit: log allowed plugins
                        for (pname, ptype, pcmd) in &plugins_snapshot {
                            let _ = crate::db::insert_plugin_audit(
                                &state.db_path,
                                &crate::db::PluginAuditRecord {
                                    action: "crd_apply".into(),
                                    crd_kind: crd_kind.clone(),
                                    plugin_name: Some(pname.clone()),
                                    plugin_type: Some(ptype.clone()),
                                    command: pcmd.clone(),
                                    applied_by: None,
                                    transport: None,
                                    peer_pid: None,
                                    result: "allowed".into(),
                                    policy_mode: Some(policy_mode.clone()),
                                },
                            );
                        }
                        for (label, cmd) in &hooks_snapshot {
                            let _ = crate::db::insert_plugin_audit(
                                &state.db_path,
                                &crate::db::PluginAuditRecord {
                                    action: "crd_apply".into(),
                                    crd_kind: crd_kind.clone(),
                                    plugin_name: Some(label.clone()),
                                    plugin_type: Some("hook".into()),
                                    command: cmd.clone(),
                                    applied_by: None,
                                    transport: None,
                                    peer_pid: None,
                                    result: "allowed".into(),
                                    policy_mode: Some(policy_mode.clone()),
                                },
                            );
                        }
                        let action = apply_action_label(result);
                        results.push(orchestrator_proto::ApplyResultEntry {
                            kind: format!("crd({})", crd_kind),
                            name: crd_name,
                            action: action.to_string(),
                            project_scope: None,
                        });
                    }
                    Err(error) => {
                        // Audit: log denied plugins
                        for (pname, ptype, pcmd) in &plugins_snapshot {
                            let _ = crate::db::insert_plugin_audit(
                                &state.db_path,
                                &crate::db::PluginAuditRecord {
                                    action: "crd_apply".into(),
                                    crd_kind: crd_kind.clone(),
                                    plugin_name: Some(pname.clone()),
                                    plugin_type: Some(ptype.clone()),
                                    command: pcmd.clone(),
                                    applied_by: None,
                                    transport: None,
                                    peer_pid: None,
                                    result: "denied".into(),
                                    policy_mode: Some(policy_mode.clone()),
                                },
                            );
                        }
                        errors.push(format!(
                            "document {} (CRD {}): {}",
                            index + 1,
                            crd_name,
                            error
                        ));
                    }
                }
            }
            ParsedManifest::Custom(cr_manifest) => {
                let cr_kind = cr_manifest.kind.clone();
                let cr_name = cr_manifest.metadata.name.clone();
                match crd::apply_custom_resource(&mut merged_config, cr_manifest) {
                    Ok(result) => {
                        let action = apply_action_label(result);
                        let display_kind = crd::crd_kind_display(&cr_kind);
                        results.push(orchestrator_proto::ApplyResultEntry {
                            kind: display_kind,
                            name: cr_name,
                            action: action.to_string(),
                            project_scope: None,
                        });
                    }
                    Err(error) => {
                        errors.push(format!(
                            "document {} ({}/{}): {}",
                            index + 1,
                            cr_kind,
                            cr_name,
                            error
                        ));
                    }
                }
            }
        }
    }

    let deleted_resources = if errors.is_empty() && prune {
        plan_prune_for_project(
            &current_config,
            &mut merged_config,
            cli_project,
            &prunable_manifest_names,
        )?
    } else {
        Vec::new()
    };

    if errors.is_empty() && !deleted_resources.is_empty() {
        let conn = crate::db::open_conn(db_path)
            .map_err(|err| classify_resource_error("resource.apply", err))?;
        enforce_deletion_guards_for_removals(&conn, &deleted_resources)
            .map_err(|err| classify_resource_error("resource.apply", err))?;
    }

    for deletion in &deleted_resources {
        results.push(orchestrator_proto::ApplyResultEntry {
            kind: deletion.kind.to_lowercase(),
            name: deletion.name.clone(),
            action: "deleted".to_string(),
            project_scope: Some(deletion.project_id.clone()),
        });
    }

    let config_version = if !dry_run && !results.is_empty() && errors.is_empty() {
        autofill_defaults_for_manifest_mode(&mut merged_config);
        let yaml = serde_yaml::to_string(&merged_config)
            .context("failed to serialize config after apply")
            .map_err(|err| classify_resource_error("resource.apply", err))?;
        let overview = persist_config_and_reload(
            state,
            merged_config,
            yaml,
            "daemon-apply",
            Some(cli_project),
            &deleted_resources,
        )
        .map_err(|err| classify_resource_error("resource.apply", err))?;
        // Notify trigger engine to pick up any trigger config changes.
        crate::trigger_engine::notify_trigger_reload(state);
        Some(overview.version)
    } else {
        None
    };

    Ok(orchestrator_proto::ApplyResponse {
        results,
        config_version,
        errors,
        warnings,
    })
}

// ── Helpers used by apply_manifests and tests ────────────────────────

fn prunable_resource_kind(resource: &crate::resource::RegisteredResource) -> Option<&'static str> {
    match resource.kind() {
        crate::cli_types::ResourceKind::Workspace => Some("Workspace"),
        crate::cli_types::ResourceKind::Agent => Some("Agent"),
        crate::cli_types::ResourceKind::Workflow => Some("Workflow"),
        crate::cli_types::ResourceKind::StepTemplate => Some("StepTemplate"),
        crate::cli_types::ResourceKind::ExecutionProfile => Some("ExecutionProfile"),
        crate::cli_types::ResourceKind::EnvStore => Some("EnvStore"),
        crate::cli_types::ResourceKind::SecretStore => Some("SecretStore"),
        crate::cli_types::ResourceKind::Trigger => Some("Trigger"),
        crate::cli_types::ResourceKind::Project | crate::cli_types::ResourceKind::RuntimePolicy => {
            None
        }
    }
}

fn apply_action_label(result: ApplyResult) -> &'static str {
    match result {
        ApplyResult::Created => "created",
        ApplyResult::Configured => "updated",
        ApplyResult::Unchanged => "unchanged",
    }
}

fn plan_prune_for_project(
    previous: &crate::config::OrchestratorConfig,
    candidate: &mut crate::config::OrchestratorConfig,
    project_id: &str,
    manifest_names: &HashMap<&'static str, HashSet<String>>,
) -> Result<Vec<ResourceRemoval>> {
    let Some(previous_project) = previous.projects.get(project_id) else {
        return Ok(Vec::new());
    };
    let Some(candidate_project) = candidate.projects.get_mut(project_id) else {
        return Ok(Vec::new());
    };

    let mut deletions: Vec<ResourceRemoval> = Vec::new();
    for (kind, declared_names) in manifest_names {
        match *kind {
            "Agent" => prune_map_entries(
                &mut candidate_project.agents,
                declared_names,
                kind,
                project_id,
                &mut deletions,
            ),
            "Workflow" => prune_map_entries(
                &mut candidate_project.workflows,
                declared_names,
                kind,
                project_id,
                &mut deletions,
            ),
            "Workspace" => prune_map_entries(
                &mut candidate_project.workspaces,
                declared_names,
                kind,
                project_id,
                &mut deletions,
            ),
            "StepTemplate" => prune_map_entries(
                &mut candidate_project.step_templates,
                declared_names,
                kind,
                project_id,
                &mut deletions,
            ),
            "ExecutionProfile" => prune_map_entries(
                &mut candidate_project.execution_profiles,
                declared_names,
                kind,
                project_id,
                &mut deletions,
            ),
            "EnvStore" => {
                let existing_names: Vec<String> = previous_project
                    .env_stores
                    .keys()
                    .filter(|name| !declared_names.contains(*name))
                    .cloned()
                    .collect();
                for name in existing_names {
                    candidate_project.env_stores.remove(&name);
                    deletions.push(ResourceRemoval {
                        kind: "EnvStore".to_string(),
                        project_id: project_id.to_string(),
                        name,
                    });
                }
            }
            "SecretStore" => {
                let existing_names: Vec<String> = previous_project
                    .secret_stores
                    .keys()
                    .filter(|name| !declared_names.contains(*name))
                    .cloned()
                    .collect();
                for name in existing_names {
                    candidate_project.secret_stores.remove(&name);
                    deletions.push(ResourceRemoval {
                        kind: "SecretStore".to_string(),
                        project_id: project_id.to_string(),
                        name,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(deletions)
}

fn prune_map_entries<T>(
    map: &mut HashMap<String, T>,
    declared_names: &HashSet<String>,
    kind: &str,
    project_id: &str,
    deletions: &mut Vec<ResourceRemoval>,
) {
    let existing_names: Vec<String> = map
        .keys()
        .filter(|name| !declared_names.contains(*name))
        .cloned()
        .collect();
    for name in existing_names {
        map.remove(&name);
        deletions.push(ResourceRemoval {
            kind: kind.to_string(),
            project_id: project_id.to_string(),
            name,
        });
    }
}

/// Export all resources as manifest documents in JSON or YAML format.
pub fn export_manifests(state: &InnerState, output_format: &str) -> Result<String> {
    let active = read_active_config(state)?;
    let config = &active.config;

    let builtin_docs = crate::resource::export_manifest_documents(config);
    let crd_docs = crate::resource::export_crd_documents(config);

    match output_format {
        "json" => {
            let mut all = serde_json::to_value(&builtin_docs)?;
            if let serde_json::Value::Array(ref mut arr) = all {
                for doc in crd_docs {
                    if let Ok(json_val) = serde_json::to_value(&doc) {
                        arr.push(json_val);
                    }
                }
            }
            Ok(serde_json::to_string_pretty(&all)?)
        }
        _ => {
            let mut parts = Vec::new();
            for doc in &builtin_docs {
                parts.push(serde_yaml::to_string(doc)?);
            }
            for doc in &crd_docs {
                parts.push(serde_yaml::to_string(doc)?);
            }
            Ok(parts.join("---\n"))
        }
    }
}

pub(super) fn format_output<T: serde::Serialize>(value: &T, format: &str) -> Result<String> {
    match format {
        "json" => Ok(serde_json::to_string_pretty(value)?),
        "yaml" => Ok(serde_yaml::to_string(value)?),
        "table" => Ok(serde_json::to_string_pretty(value)?), // fallback
        _ => Ok(serde_json::to_string_pretty(value)?),
    }
}

fn autofill_defaults_for_manifest_mode(config: &mut crate::config::OrchestratorConfig) {
    config
        .projects
        .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
        .or_insert_with(|| crate::config::ProjectConfig {
            description: Some("Built-in default project".to_string()),
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
            step_templates: Default::default(),
            env_stores: Default::default(),
            secret_stores: Default::default(),
            execution_profiles: Default::default(),
            triggers: Default::default(),
        });
}
