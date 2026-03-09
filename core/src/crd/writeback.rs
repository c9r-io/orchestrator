use crate::config::{EnvStoreConfig, OrchestratorConfig};
use crate::crd::projection::{CrdProjectable, RuntimePolicyProjection, SecretStoreProjection};

pub fn reconcile_builtin_kind(config: &mut OrchestratorConfig, kind: &str) {
    match kind {
        "Project" => {
            let projected = config.resource_store.project_map::<crate::config::ProjectConfig>();
            for (name, proj) in projected {
                config
                    .projects
                    .entry(name)
                    .and_modify(|existing| existing.description = proj.description.clone())
                    .or_insert(proj);
            }
        }
        "RuntimePolicy" => {
            if let Some(rp) = config
                .resource_store
                .project_singleton::<RuntimePolicyProjection>()
            {
                config.runner = rp.runner;
                config.resume = rp.resume;
                config.observability = rp.observability;
            }
        }
        _ => {}
    }
}

pub fn seed_store_from_config_snapshot(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
    now: &str,
) {
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::CustomResource;

    let make_cr = |project: Option<String>, spec: serde_json::Value| -> CustomResource {
        CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    };

    // Search all projects for the named resource (project-scoped kinds).
    match kind {
        "Agent" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.agents.get(name) {
                    config.resource_store.put(make_cr(
                        Some(pid.clone()),
                        v.to_cr_spec(),
                    ));
                    return;
                }
            }
        }
        "Workflow" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.workflows.get(name) {
                    config.resource_store.put(make_cr(
                        Some(pid.clone()),
                        v.to_cr_spec(),
                    ));
                    return;
                }
            }
        }
        "Workspace" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.workspaces.get(name) {
                    config.resource_store.put(make_cr(
                        Some(pid.clone()),
                        v.to_cr_spec(),
                    ));
                    return;
                }
            }
        }
        "StepTemplate" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.step_templates.get(name) {
                    config.resource_store.put(make_cr(
                        Some(pid.clone()),
                        v.to_cr_spec(),
                    ));
                    return;
                }
            }
        }
        "EnvStore" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.env_stores.get(name) {
                    if !v.sensitive {
                        config.resource_store.put(make_cr(
                            Some(pid.clone()),
                            v.to_cr_spec(),
                        ));
                        return;
                    }
                }
            }
        }
        "SecretStore" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.env_stores.get(name) {
                    if v.sensitive {
                        let proj = SecretStoreProjection(v.clone());
                        config.resource_store.put(make_cr(
                            Some(pid.clone()),
                            proj.to_cr_spec(),
                        ));
                        return;
                    }
                }
            }
        }
        "Project" => {
            if let Some(v) = config.projects.get(name) {
                config.resource_store.put(make_cr(
                    Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
                    v.to_cr_spec(),
                ));
            }
        }
        "RuntimePolicy" => {
            let rp = RuntimePolicyProjection {
                runner: config.runner.clone(),
                resume: config.resume.clone(),
                observability: config.observability.clone(),
            };
            config.resource_store.put(make_cr(
                Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
                rp.to_cr_spec(),
            ));
        }
        _ => {}
    }
}

pub fn reconcile_single_resource(config: &mut OrchestratorConfig, kind: &str, name: &str) {
    use crate::config::{
        AgentConfig, ProjectConfig, StepTemplateConfig, WorkflowConfig, WorkspaceConfig,
    };

    let cr = config
        .resource_store
        .get_namespaced(kind, crate::crd::store::SYSTEM_PROJECT, name)
        .or_else(|| {
            config
                .resource_store
                .get_namespaced(kind, crate::config::DEFAULT_PROJECT_ID, name)
        })
        .cloned();
    let Some(cr) = cr else {
        return;
    };
    let spec = cr.spec.clone();

    match kind {
        "Agent" => {
            if let Ok(v) = AgentConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .agents
                    .insert(name.to_string(), v);
            }
        }
        "Workflow" => {
            if let Ok(v) = WorkflowConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .workflows
                    .insert(name.to_string(), v);
            }
        }
        "Workspace" => {
            if let Ok(v) = WorkspaceConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .workspaces
                    .insert(name.to_string(), v);
            }
        }
        "Project" => {
            if let Ok(v) = ProjectConfig::from_cr_spec(&spec) {
                config
                    .projects
                    .entry(name.to_string())
                    .and_modify(|existing| existing.description = v.description.clone())
                    .or_insert(v);
            }
        }
        "RuntimePolicy" => {
            if let Ok(rp) = RuntimePolicyProjection::from_cr_spec(&spec) {
                config.runner = rp.runner;
                config.resume = rp.resume;
                config.observability = rp.observability;
            }
        }
        "StepTemplate" => {
            if let Ok(v) = StepTemplateConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .step_templates
                    .insert(name.to_string(), v);
            }
        }
        "EnvStore" => {
            if let Ok(v) = EnvStoreConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .env_stores
                    .insert(name.to_string(), v);
            }
        }
        "SecretStore" => {
            if let Ok(v) = SecretStoreProjection::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .env_stores
                    .insert(name.to_string(), v.0);
            }
        }
        _ => {}
    }
}

pub fn remove_from_config_snapshot(config: &mut OrchestratorConfig, kind: &str, name: &str) {
    match kind {
        "Agent" => {
            for project in config.projects.values_mut() {
                if project.agents.remove(name).is_some() {
                    return;
                }
            }
        }
        "Workflow" => {
            for project in config.projects.values_mut() {
                if project.workflows.remove(name).is_some() {
                    return;
                }
            }
        }
        "Workspace" => {
            for project in config.projects.values_mut() {
                if project.workspaces.remove(name).is_some() {
                    return;
                }
            }
        }
        "Project" => {
            config.projects.remove(name);
        }
        "StepTemplate" => {
            for project in config.projects.values_mut() {
                if project.step_templates.remove(name).is_some() {
                    return;
                }
            }
        }
        "EnvStore" | "SecretStore" => {
            for project in config.projects.values_mut() {
                if project.env_stores.remove(name).is_some() {
                    return;
                }
            }
        }
        _ => {}
    }
}

pub fn reconcile_all_builtins(config: &mut OrchestratorConfig) {
    reconcile_builtin_kind(config, "Project");
    reconcile_builtin_kind(config, "RuntimePolicy");
}

pub fn restore_metadata_from_previous_store(
    config: &mut OrchestratorConfig,
    old_store: &crate::crd::store::ResourceStore,
) {
    for (key, old_cr) in old_store.resources() {
        let has_labels = old_cr
            .metadata
            .labels
            .as_ref()
            .is_some_and(|m| !m.is_empty());
        let has_annotations = old_cr
            .metadata
            .annotations
            .as_ref()
            .is_some_and(|m| !m.is_empty());
        if !has_labels && !has_annotations {
            continue;
        }
        if let Some(new_cr) = config.resource_store.get_mut_by_key(key) {
            if has_labels && new_cr.metadata.labels.is_none() {
                new_cr.metadata.labels = old_cr.metadata.labels.clone();
            }
            if has_annotations && new_cr.metadata.annotations.is_none() {
                new_cr.metadata.annotations = old_cr.metadata.annotations.clone();
            }
        }
    }
}

pub fn sync_config_snapshot_to_store(config: &mut OrchestratorConfig) {
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::CustomResource;

    let now = chrono::Utc::now().to_rfc3339();

    fn make_cr(
        kind: &str,
        name: &str,
        project: Option<String>,
        spec: serde_json::Value,
        now: &str,
    ) -> CustomResource {
        CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    }

    for (project_id, project) in &config.projects {
        for (name, agent) in &project.agents {
            config.resource_store.put(make_cr(
                "Agent",
                name,
                Some(project_id.clone()),
                agent.to_cr_spec(),
                &now,
            ));
        }
        for (name, workflow) in &project.workflows {
            config.resource_store.put(make_cr(
                "Workflow",
                name,
                Some(project_id.clone()),
                workflow.to_cr_spec(),
                &now,
            ));
        }
        for (name, workspace) in &project.workspaces {
            config.resource_store.put(make_cr(
                "Workspace",
                name,
                Some(project_id.clone()),
                workspace.to_cr_spec(),
                &now,
            ));
        }
        for (name, tmpl) in &project.step_templates {
            config.resource_store.put(make_cr(
                "StepTemplate",
                name,
                Some(project_id.clone()),
                tmpl.to_cr_spec(),
                &now,
            ));
        }
        for (name, store) in &project.env_stores {
            let kind = if store.sensitive {
                "SecretStore"
            } else {
                "EnvStore"
            };
            let spec = if store.sensitive {
                SecretStoreProjection(store.clone()).to_cr_spec()
            } else {
                store.to_cr_spec()
            };
            config.resource_store.put(make_cr(
                kind,
                name,
                Some(project_id.clone()),
                spec,
                &now,
            ));
        }
    }

    for (name, project) in &config.projects {
        config.resource_store.put(make_cr(
            "Project",
            name,
            Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
            project.to_cr_spec(),
            &now,
        ));
    }

    let rp = RuntimePolicyProjection {
        runner: config.runner.clone(),
        resume: config.resume.clone(),
        observability: config.observability.clone(),
    };
    config.resource_store.put(make_cr(
        "RuntimePolicy",
        "runtime",
        Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
        rp.to_cr_spec(),
        &now,
    ));
}
