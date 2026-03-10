use crate::config_load::{
    enforce_deletion_guards_for_removals, load_config, persist_config_and_reload,
    persist_config_for_delete, read_active_config, ResourceRemoval,
};
use crate::crd::{self, ParsedManifest};
use crate::resource::{
    apply_to_project, dispatch_resource, kind_as_str, parse_manifests_from_yaml, ApplyResult,
    Resource,
};
use crate::state::InnerState;
use anyhow::{Context, Result};
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
    let manifests =
        parse_manifests_from_yaml(content).map_err(|e| anyhow::anyhow!("parse error: {}", e))?;

    let current_config = load_config(db_path)?
        .map(|(cfg, _, _)| cfg)
        .unwrap_or_default();
    let mut merged_config = current_config.clone();

    let mut results = Vec::new();
    let mut errors = Vec::new();
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
                let result = apply_to_project(&registered, &mut merged_config, cli_project)?;
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
                match crd::apply_crd(&mut merged_config, crd_manifest) {
                    Ok(result) => {
                        let action = apply_action_label(result);
                        results.push(orchestrator_proto::ApplyResultEntry {
                            kind: format!("crd({})", crd_kind),
                            name: crd_name,
                            action: action.to_string(),
                            project_scope: None,
                        });
                    }
                    Err(error) => {
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
        let conn = crate::db::open_conn(db_path)?;
        enforce_deletion_guards_for_removals(&conn, &deleted_resources)?;
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
        let yaml = serde_yml::to_string(&merged_config)
            .context("failed to serialize config after apply")?;
        let overview = persist_config_and_reload(
            state,
            merged_config,
            yaml,
            "daemon-apply",
            Some(cli_project),
            &deleted_resources,
        )?;
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
    project: Option<&str>,
) -> Result<String> {
    let active = read_active_config(state)?;
    let config = &active.config;
    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let proj_cfg = config
        .projects
        .get(project_id)
        .context(format!("project not found: {}", project_id))?;

    if resource.contains('/') {
        if selector.is_some() {
            anyhow::bail!("label selector (-l) cannot be used with a named resource; use it with list queries only");
        }
        let parts: Vec<&str> = resource.splitn(2, '/').collect();
        let (kind, name) = (parts[0], parts[1]);
        get_single_resource(proj_cfg, kind, name, output_format)
    } else {
        get_list_resource(
            proj_cfg,
            resource,
            selector,
            output_format,
            &config.resource_store,
        )
    }
}

fn get_single_resource(
    project: &crate::config::ProjectConfig,
    kind: &str,
    name: &str,
    output_format: &str,
) -> Result<String> {
    match kind {
        "ws" | "workspace" => {
            let ws = project
                .workspaces
                .get(name)
                .context(format!("workspace not found: {}", name))?;
            format_output(ws, output_format)
        }
        "wf" | "workflow" => {
            let wf = project
                .workflows
                .get(name)
                .context(format!("workflow not found: {}", name))?;
            format_output(wf, output_format)
        }
        "agent" => {
            let agent = project
                .agents
                .get(name)
                .context(format!("agent not found: {}", name))?;
            format_output(agent, output_format)
        }
        _ => anyhow::bail!("unknown resource type: {}", kind),
    }
}

fn get_list_resource(
    project: &crate::config::ProjectConfig,
    resource_type: &str,
    selector: Option<&str>,
    output_format: &str,
    resource_store: &crate::crd::store::ResourceStore,
) -> Result<String> {
    let (names, crd_kind): (Vec<&String>, &str) = match resource_type {
        "ws" | "workspace" | "workspaces" => (project.workspaces.keys().collect(), "Workspace"),
        "agent" | "agents" => (project.agents.keys().collect(), "Agent"),
        "wf" | "workflow" | "workflows" => (project.workflows.keys().collect(), "Workflow"),
        _ => anyhow::bail!("unknown list resource type: {}", resource_type),
    };

    let filtered: Vec<&String> = if let Some(sel) = selector {
        let conditions = parse_label_selector(sel)?;
        names
            .into_iter()
            .filter(|name| {
                let labels = resource_store
                    .get(crd_kind, name)
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
fn parse_label_selector(selector: &str) -> Result<Vec<(String, String)>> {
    let mut conditions = Vec::new();
    for part in selector.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let kv: Vec<&str> = part.splitn(2, '=').collect();
        if kv.len() != 2 {
            anyhow::bail!("invalid label selector: '{}' (expected key=value)", part);
        }
        conditions.push((kv[0].to_string(), kv[1].to_string()));
    }
    Ok(conditions)
}

/// Check if a resource's labels match all selector conditions (AND logic).
fn match_labels(
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

/// Delete a resource by kind/name.
pub fn delete_resource(
    state: &InnerState,
    resource: &str,
    force: bool,
    project: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let parts: Vec<&str> = resource.split('/').collect();
    if parts.len() != 2 {
        anyhow::bail!("invalid resource format: {} (use kind/name)", resource);
    }
    let (kind, name) = (parts[0], parts[1]);

    if !force {
        anyhow::bail!("use --force to confirm deletion of {}/{}", kind, name);
    }

    let config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    if dry_run {
        if kind == "project" {
            if config.projects.contains_key(name) {
                return Ok(());
            } else {
                anyhow::bail!("project '{}' not found", name);
            }
        }
        let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
        let proj_cfg = config
            .projects
            .get(project_id)
            .context(format!("project not found: {}", project_id))?;
        let exists = match kind {
            "ws" | "workspace" => proj_cfg.workspaces.contains_key(name),
            "agent" => proj_cfg.agents.contains_key(name),
            "wf" | "workflow" => proj_cfg.workflows.contains_key(name),
            "steptemplate" | "step-template" | "step_template" => {
                proj_cfg.step_templates.contains_key(name)
            }
            "envstore" | "env-store" | "env_store" | "secretstore" | "secret-store"
            | "secret_store" => proj_cfg.env_stores.contains_key(name),
            _ => false,
        };
        if !exists {
            anyhow::bail!("{}/{} not found in project '{}'", kind, name, project_id);
        }
        return Ok(());
    }

    let mut config = config;

    if kind == "project" {
        // 1. Clear task data (tasks, items, runs, events)
        let _stats = crate::db::reset_project_data(state, name)?;

        // 2. Clean auto-ticket files from project workspaces
        let mut _tickets_cleaned: u64 = 0;
        if let Some(project_cfg) = config.projects.get(name) {
            for ws_config in project_cfg.workspaces.values() {
                let ticket_path = state
                    .app_root
                    .join(&ws_config.root_path)
                    .join(&ws_config.ticket_dir);
                if ticket_path.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&ticket_path) {
                        for entry in entries.flatten() {
                            let fname = entry.file_name();
                            let fname_str = fname.to_string_lossy();
                            if fname_str.starts_with("auto_")
                                && fname_str.ends_with(".md")
                                && std::fs::remove_file(entry.path()).is_ok()
                            {
                                _tickets_cleaned += 1;
                            }
                        }
                    }
                }
            }
        }

        // 3. Remove project config entry
        config.projects.remove(name);

        // 4. Also remove resource DB rows for this project
        {
            let conn = crate::db::open_conn(&state.db_path)?;
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM resources WHERE project = ?1",
                rusqlite::params![name],
            )?;
            tx.commit()?;
        }

        // 5. Persist (using delete-safe path)
        persist_config_for_delete(state, config, "project-delete", &[])?;
        return Ok(());
    }

    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let proj_cfg = config
        .projects
        .get_mut(project_id)
        .context(format!("project not found: {}", project_id))?;
    let canonical_kind = canonical_project_kind(kind)?;
    let deleted = delete_resource_from_project(proj_cfg, kind, name)?;
    if !deleted {
        anyhow::bail!("{}/{} not found in project '{}'", kind, name, project_id);
    }
    let deleted_resources = vec![ResourceRemoval {
        kind: canonical_kind.to_string(),
        project_id: project_id.to_string(),
        name: name.to_string(),
    }];
    persist_config_for_delete(state, config, "daemon-delete", &deleted_resources)?;
    Ok(())
}

fn delete_resource_from_project(
    proj: &mut crate::config::ProjectConfig,
    kind: &str,
    name: &str,
) -> Result<bool> {
    match kind {
        "ws" | "workspace" => Ok(proj.workspaces.remove(name).is_some()),
        "agent" => Ok(proj.agents.remove(name).is_some()),
        "wf" | "workflow" => Ok(proj.workflows.remove(name).is_some()),
        "steptemplate" | "step-template" | "step_template" => {
            Ok(proj.step_templates.remove(name).is_some())
        }
        "executionprofile" | "execution-profile" | "execution_profile" => {
            Ok(proj.execution_profiles.remove(name).is_some())
        }
        "envstore" | "env-store" | "env_store" | "secretstore" | "secret-store"
        | "secret_store" => Ok(proj.env_stores.remove(name).is_some()),
        _ => anyhow::bail!("unknown resource type for project delete: {}", kind),
    }
}

fn canonical_project_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "ws" | "workspace" => Ok("Workspace"),
        "agent" => Ok("Agent"),
        "wf" | "workflow" => Ok("Workflow"),
        "steptemplate" | "step-template" | "step_template" => Ok("StepTemplate"),
        "executionprofile" | "execution-profile" | "execution_profile" => Ok("ExecutionProfile"),
        "envstore" | "env-store" | "env_store" => Ok("EnvStore"),
        "secretstore" | "secret-store" | "secret_store" => Ok("SecretStore"),
        _ => anyhow::bail!("unknown resource type for project delete: {}", kind),
    }
}

fn prunable_resource_kind(resource: &crate::resource::RegisteredResource) -> Option<&'static str> {
    match resource.kind() {
        crate::cli_types::ResourceKind::Workspace => Some("Workspace"),
        crate::cli_types::ResourceKind::Agent => Some("Agent"),
        crate::cli_types::ResourceKind::Workflow => Some("Workflow"),
        crate::cli_types::ResourceKind::StepTemplate => Some("StepTemplate"),
        crate::cli_types::ResourceKind::ExecutionProfile => Some("ExecutionProfile"),
        crate::cli_types::ResourceKind::EnvStore => Some("EnvStore"),
        crate::cli_types::ResourceKind::SecretStore => Some("SecretStore"),
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
            "EnvStore" | "SecretStore" => {
                let expected_sensitivity = *kind == "SecretStore";
                let existing_names: Vec<String> = previous_project
                    .env_stores
                    .iter()
                    .filter_map(|(name, store)| {
                        if store.sensitive == expected_sensitivity && !declared_names.contains(name)
                        {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                for name in existing_names {
                    candidate_project.env_stores.remove(&name);
                    deletions.push(ResourceRemoval {
                        kind: (*kind).to_string(),
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
                parts.push(serde_yml::to_string(doc)?);
            }
            for doc in &crd_docs {
                parts.push(serde_yml::to_string(doc)?);
            }
            Ok(parts.join("---\n"))
        }
    }
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
            execution_profiles: Default::default(),
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::read_active_config;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    fn workflow_manifest(name: &str, command: &str) -> String {
        format!(
            "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"{command}\"\n  loop:\n    mode: once\n"
        )
    }

    fn project_bundle_manifest(delete_workflow_name: &str) -> String {
        format!(
            "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"workspace/default\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {delete_workflow_name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo delete\"\n  loop:\n    mode: once\n"
        )
    }

    fn project_subset_manifest() -> String {
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"workspace/default\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}'\"\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n".to_string()
    }

    #[test]
    fn apply_without_prune_keeps_existing_resources_not_in_manifest() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let first_manifest = format!(
            "{}---\n{}",
            workflow_manifest("keep-me", "echo keep"),
            workflow_manifest("update-me", "echo old")
        );
        apply_manifests(
            &state,
            &first_manifest,
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("seed workflows");

        let second_manifest = workflow_manifest("update-me", "echo new");
        apply_manifests(
            &state,
            &second_manifest,
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("apply without prune");

        let active = read_active_config(&state).expect("read active config");
        let project = active
            .config
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        assert!(project.workflows.contains_key("keep-me"));
        assert!(project.workflows.contains_key("update-me"));
    }

    #[test]
    fn apply_prune_dry_run_reports_deleted_without_persisting() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let seed_manifest = format!(
            "{}---\n{}",
            workflow_manifest("keep-me", "echo keep"),
            workflow_manifest("delete-me", "echo delete")
        );
        apply_manifests(
            &state,
            &seed_manifest,
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("seed workflows");

        let dry_run = apply_manifests(
            &state,
            &workflow_manifest("keep-me", "echo keep"),
            true,
            Some(crate::config::DEFAULT_PROJECT_ID),
            true,
        )
        .expect("dry-run prune");

        assert!(dry_run
            .results
            .iter()
            .any(|entry| entry.name == "delete-me" && entry.action == "deleted"));

        let active = read_active_config(&state).expect("read active config");
        let project = active
            .config
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        assert!(project.workflows.contains_key("delete-me"));
    }

    #[test]
    fn apply_prune_blocks_non_terminal_referenced_workflow() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/prune-block.md");
        std::fs::write(&qa_file, "# prune block\n").expect("seed qa file");

        let seed_manifest = format!(
            "{}---\n{}",
            workflow_manifest("keep-me", "echo keep"),
            workflow_manifest("delete-me", "echo delete")
        );
        apply_manifests(
            &state,
            &seed_manifest,
            false,
            Some(crate::config::DEFAULT_PROJECT_ID),
            false,
        )
        .expect("seed workflows");

        create_task_impl(
            &state,
            CreateTaskPayload {
                workflow_id: Some("delete-me".to_string()),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create referencing task");

        let error = apply_manifests(
            &state,
            &workflow_manifest("keep-me", "echo keep"),
            true,
            Some(crate::config::DEFAULT_PROJECT_ID),
            true,
        )
        .expect_err("prune should be blocked");
        let message = error.to_string();
        assert!(message.contains("workflow/delete-me"));
        assert!(message.contains("blocking tasks:"));
        assert!(message.contains("rerun without --prune"));

        let active = read_active_config(&state).expect("read active config after blocked prune");
        let project = active
            .config
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        assert!(project.workflows.contains_key("delete-me"));
        assert!(project.workflows.contains_key("keep-me"));
    }

    #[test]
    fn apply_without_prune_preserves_same_named_resources_across_projects() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let bundle = project_bundle_manifest("delete-me");
        apply_manifests(&state, &bundle, false, Some("alpha"), false).expect("seed alpha");
        apply_manifests(&state, &bundle, false, Some("beta"), false).expect("seed beta");

        apply_manifests(
            &state,
            &workflow_manifest("keep-me", "echo updated"),
            false,
            Some("alpha"),
            false,
        )
        .expect("apply workflow-only manifest without prune");

        let active = read_active_config(&state).expect("read active config");
        let alpha = active.config.projects.get("alpha").expect("alpha project");
        let beta = active.config.projects.get("beta").expect("beta project");
        assert!(alpha.workspaces.contains_key("shared-ws"));
        assert!(alpha.workflows.contains_key("delete-me"));
        assert!(beta.workspaces.contains_key("shared-ws"));
        assert!(beta.workflows.contains_key("delete-me"));
    }

    #[test]
    fn apply_prune_isolated_to_target_project_with_same_named_resources() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/cross-project.md");
        std::fs::write(&qa_file, "# cross project\n").expect("seed qa file");

        let bundle = project_bundle_manifest("delete-me");
        apply_manifests(&state, &bundle, false, Some("alpha"), false).expect("seed alpha");
        apply_manifests(&state, &bundle, false, Some("beta"), false).expect("seed beta");

        create_task_impl(
            &state,
            CreateTaskPayload {
                project_id: Some("alpha".to_string()),
                workspace_id: Some("shared-ws".to_string()),
                workflow_id: Some("delete-me".to_string()),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create alpha blocker");

        apply_manifests(
            &state,
            &project_subset_manifest(),
            false,
            Some("beta"),
            true,
        )
        .expect("beta prune should ignore alpha blocker");

        let active = read_active_config(&state).expect("read active config");
        let alpha = active.config.projects.get("alpha").expect("alpha project");
        let beta = active.config.projects.get("beta").expect("beta project");
        assert!(alpha.workflows.contains_key("delete-me"));
        assert!(!beta.workflows.contains_key("delete-me"));
        assert!(beta.workflows.contains_key("keep-me"));
    }
}
