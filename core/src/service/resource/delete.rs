use crate::config_load::{ResourceRemoval, persist_config_for_delete, read_active_config};
use crate::error::{Result, classify_resource_error};
use crate::state::InnerState;

/// Delete a resource by kind/name.
pub fn delete_resource(
    state: &InnerState,
    resource: &str,
    force: bool,
    project: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    delete_resource_with_references(state, resource, force, project, dry_run, false)
}

/// Deletes a resource and, when explicitly authorized by the caller, its binding references.
pub fn delete_resource_with_references(
    state: &InnerState,
    resource: &str,
    force: bool,
    project: Option<&str>,
    dry_run: bool,
    force_references: bool,
) -> Result<()> {
    let parts: Vec<&str> = resource.split('/').collect();
    if parts.len() != 2 {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("invalid resource format: {} (use kind/name)", resource),
        ));
    }
    let (kind, name) = (parts[0], parts[1]);

    if !force {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("use --force to confirm deletion of {}/{}", kind, name),
        ));
    }

    let config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let is_source_template = matches!(
        kind,
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt"
    );
    if force_references && !is_source_template {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("--force-references is only valid for SourceTaskTemplate deletion"),
        ));
    }
    let binding_references = if is_source_template {
        source_task_binding_references(&config, project_id, name)
    } else {
        Vec::new()
    };
    if !binding_references.is_empty() && !force_references {
        return Err(crate::error::OrchestratorError::invalid_state(
            "resource.delete",
            anyhow::anyhow!(
                "SourceTaskTemplate '{}/{}' is referenced by SourceTaskBinding(s): {}; use --force-references with Admin authorization to remove them atomically",
                project_id,
                name,
                binding_references.join(", ")
            ),
        ));
    }

    if dry_run {
        if kind == "project" {
            if config.projects.contains_key(name) {
                return Ok(());
            } else {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("project '{}' not found", name),
                ));
            }
        }
        // CRD dry-run check
        if kind == "crd" || kind == "customresourcedefinition" {
            if config.custom_resource_definitions.contains_key(name) {
                return Ok(());
            } else {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("CRD '{}' not found", name),
                ));
            }
        }
        // Custom resource dry-run check (skip kinds with dedicated ProjectConfig projections)
        if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(&config, kind) {
            if !crate::crd::resolve::is_builtin_kind(&crd.kind) {
                let storage_key = format!("{}/{}", crd.kind, name);
                if config.custom_resources.contains_key(&storage_key) {
                    return Ok(());
                } else {
                    return Err(classify_resource_error(
                        "resource.delete",
                        anyhow::anyhow!("{}/{} not found", crd.kind, name),
                    ));
                }
            }
        }
        let proj_cfg = match config.projects.get(project_id) {
            Some(p) => p,
            None => {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("{}/{} not found in project '{}'", kind, name, project_id),
                ));
            }
        };
        let exists = match kind {
            "ws" | "workspace" => proj_cfg.workspaces.contains_key(name),
            "agent" => proj_cfg.agents.contains_key(name),
            "wf" | "workflow" => proj_cfg.workflows.contains_key(name),
            "steptemplate" | "step-template" | "step_template" => {
                proj_cfg.step_templates.contains_key(name)
            }
            "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt" => {
                proj_cfg.source_task_templates.contains_key(name)
            }
            "envstore" | "env-store" | "env_store" | "secretstore" | "secret-store"
            | "secret_store" => proj_cfg.env_stores.contains_key(name),
            "trigger" | "tg" => proj_cfg.triggers.contains_key(name),
            _ => false,
        };
        if !exists {
            return Err(classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("{}/{} not found in project '{}'", kind, name, project_id),
            ));
        }
        return Ok(());
    }

    let mut config = config;
    if force_references {
        remove_source_task_binding_references(&mut config, project_id, name);
    }

    // Handle CRD and custom resource deletion (not project-scoped)
    if kind == "crd" || kind == "customresourcedefinition" {
        let deleted = crate::crd::delete_crd(&mut config, name)?;
        if !deleted {
            return Err(classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("CRD '{}' not found", name),
            ));
        }
        persist_config_for_delete(state, config, "daemon-delete", &[])?;
        crate::trigger_engine::notify_trigger_reload(state);
        return Ok(());
    }

    if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(&config, kind) {
        if !crate::crd::resolve::is_builtin_kind(&crd.kind) {
            let crd_kind = crd.kind.clone();
            let deleted = crate::crd::delete_custom_resource(&mut config, &crd_kind, name)?;
            if !deleted {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("{}/{} not found", crd_kind, name),
                ));
            }
            persist_config_for_delete(state, config, "daemon-delete", &[])?;
            crate::trigger_engine::notify_trigger_reload(state);
            return Ok(());
        }
    }

    if kind == "project" {
        // 1. Clear task data (tasks, items, runs, events)
        let _stats = crate::db::reset_project_data(state, name)?;

        // 2. Clean auto-ticket files from project workspaces
        let mut _tickets_cleaned: u64 = 0;
        if let Some(project_cfg) = config.projects.get(name) {
            for ws_config in project_cfg.workspaces.values() {
                let ticket_path = state
                    .data_dir
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

        // 3. Remove project config entry and resource_store entries
        config.projects.remove(name);
        config.resource_store.remove_all_for_project(name);

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
        crate::trigger_engine::notify_trigger_reload(state);
        return Ok(());
    }

    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let proj_cfg = config.projects.get_mut(project_id).ok_or_else(|| {
        classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("project not found: {}", project_id),
        )
    })?;
    let canonical_kind = canonical_project_kind(kind)?;
    let deleted = delete_resource_from_project(proj_cfg, kind, name)?;
    if !deleted {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("{}/{} not found in project '{}'", kind, name, project_id),
        ));
    }
    let mut deleted_resources = vec![ResourceRemoval {
        kind: canonical_kind.to_string(),
        project_id: project_id.to_string(),
        name: name.to_string(),
    }];
    if force_references {
        deleted_resources.extend(binding_references.into_iter().map(|binding_name| {
            ResourceRemoval {
                kind: "SourceTaskBinding".to_string(),
                project_id: project_id.to_string(),
                name: binding_name,
            }
        }));
    }
    persist_config_for_delete(state, config, "daemon-delete", &deleted_resources)?;
    crate::trigger_engine::notify_trigger_reload(state);
    Ok(())
}

fn source_task_binding_references(
    config: &crate::config::OrchestratorConfig,
    project_id: &str,
    template_name: &str,
) -> Vec<String> {
    let mut names: Vec<String> = config
        .resource_store
        .list_by_kind_for_project("SourceTaskBinding", project_id)
        .into_iter()
        .filter(|resource| {
            resource
                .spec
                .get("templateRef")
                .or_else(|| resource.spec.get("template_ref"))
                .and_then(serde_json::Value::as_str)
                == Some(template_name)
        })
        .map(|resource| resource.metadata.name.clone())
        .collect();
    for resource in config.custom_resources.values() {
        if resource.kind == "SourceTaskBinding"
            && resource.metadata.project.as_deref() == Some(project_id)
            && resource
                .spec
                .get("templateRef")
                .or_else(|| resource.spec.get("template_ref"))
                .and_then(serde_json::Value::as_str)
                == Some(template_name)
            && !names.contains(&resource.metadata.name)
        {
            names.push(resource.metadata.name.clone());
        }
    }
    names.sort();
    names
}

fn remove_source_task_binding_references(
    config: &mut crate::config::OrchestratorConfig,
    project_id: &str,
    template_name: &str,
) {
    let names = source_task_binding_references(config, project_id, template_name);
    for name in &names {
        config
            .resource_store
            .remove_namespaced("SourceTaskBinding", project_id, name);
    }
    config.custom_resources.retain(|_, resource| {
        !(resource.kind == "SourceTaskBinding"
            && resource.metadata.project.as_deref() == Some(project_id)
            && names.contains(&resource.metadata.name))
    });
}

pub(super) fn delete_resource_from_project(
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
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt" => {
            Ok(proj.source_task_templates.remove(name).is_some())
        }
        "executionprofile" | "execution-profile" | "execution_profile" => {
            Ok(proj.execution_profiles.remove(name).is_some())
        }
        "envstore" | "env-store" | "env_store" | "secretstore" | "secret-store"
        | "secret_store" => Ok(proj.env_stores.remove(name).is_some()),
        "trigger" | "tg" => Ok(proj.triggers.remove(name).is_some()),
        _ => Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("unknown resource type for project delete: {}", kind),
        )),
    }
}

pub(super) fn canonical_project_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "ws" | "workspace" => Ok("Workspace"),
        "agent" => Ok("Agent"),
        "wf" | "workflow" => Ok("Workflow"),
        "steptemplate" | "step-template" | "step_template" => Ok("StepTemplate"),
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt" => {
            Ok("SourceTaskTemplate")
        }
        "executionprofile" | "execution-profile" | "execution_profile" => Ok("ExecutionProfile"),
        "envstore" | "env-store" | "env_store" => Ok("EnvStore"),
        "secretstore" | "secret-store" | "secret_store" => Ok("SecretStore"),
        "trigger" | "tg" => Ok("Trigger"),
        _ => Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("unknown resource type for project delete: {}", kind),
        )),
    }
}

#[cfg(test)]
mod source_template_reference_tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::CustomResource;

    #[test]
    fn finds_and_removes_future_shaped_binding_references() {
        let mut config = crate::config::OrchestratorConfig::default();
        config.resource_store.put(CustomResource {
            kind: "SourceTaskBinding".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: "slack-docs".to_string(),
                project: Some("alpha".to_string()),
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({
                "templateRef": "docs",
                "suspend": false,
            }),
            generation: 1,
            created_at: "2026-07-17T00:00:00Z".to_string(),
            updated_at: "2026-07-17T00:00:00Z".to_string(),
        });

        assert_eq!(
            source_task_binding_references(&config, "alpha", "docs"),
            vec!["slack-docs"]
        );
        remove_source_task_binding_references(&mut config, "alpha", "docs");
        assert!(source_task_binding_references(&config, "alpha", "docs").is_empty());
    }
}
