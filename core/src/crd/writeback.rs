use crate::config::{
    EnvStoreConfig, ExecutionProfileConfig, OrchestratorConfig, SecretStoreConfig,
};
use crate::crd::projection::{CrdProjectable, RuntimePolicyProjection};
use crate::crd::store::ResourceStoreExt;

/// Reconciles one builtin kind from the resource store back into legacy config fields.
pub fn reconcile_builtin_kind(config: &mut OrchestratorConfig, kind: &str) {
    match kind {
        "Project" => {
            let projected = config
                .resource_store
                .project_map::<crate::config::ProjectConfig>();
            for (name, proj) in projected {
                config
                    .projects
                    .entry(name)
                    .and_modify(|existing| existing.description = proj.description.clone())
                    .or_insert(proj);
            }
        }
        // RuntimePolicy lives solely in the resource store — no legacy fields to sync.
        "RuntimePolicy" => {}
        _ => {}
    }
}

/// Seeds the resource store from a legacy config snapshot for one builtin resource.
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
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "Workflow" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.workflows.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "Workspace" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.workspaces.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "StepTemplate" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.step_templates.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "ExecutionProfile" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.execution_profiles.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "EnvStore" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.env_stores.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
                }
            }
        }
        "SecretStore" => {
            for (pid, project) in &config.projects {
                if let Some(v) = project.secret_stores.get(name) {
                    config
                        .resource_store
                        .put(make_cr(Some(pid.clone()), v.to_cr_spec()));
                    return;
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
            // Read existing RuntimePolicy from store, or seed defaults if absent
            if config
                .resource_store
                .project_singleton::<RuntimePolicyProjection>()
                .is_none()
            {
                let rp = RuntimePolicyProjection::default();
                config.resource_store.put(make_cr(
                    Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
                    rp.to_cr_spec(),
                ));
            }
        }
        _ => {}
    }
}

/// Reconciles one builtin resource instance from the resource store into config fields.
pub fn reconcile_single_resource(
    config: &mut OrchestratorConfig,
    kind: &str,
    project: Option<&str>,
    name: &str,
) {
    use crate::config::{
        AgentConfig, ProjectConfig, StepTemplateConfig, WorkflowConfig, WorkspaceConfig,
    };
    use crate::crd::store::is_project_scoped;

    let cr = if is_project_scoped(kind) {
        let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
        config
            .resource_store
            .get_namespaced(kind, project_id, name)
            .cloned()
    } else {
        config.resource_store.get(kind, name).cloned()
    };
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
        // RuntimePolicy lives solely in the resource store — no-op.
        "RuntimePolicy" => {}
        "StepTemplate" => {
            if let Ok(v) = StepTemplateConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .step_templates
                    .insert(name.to_string(), v);
            }
        }
        "ExecutionProfile" => {
            if let Ok(v) = ExecutionProfileConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .execution_profiles
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
            if let Ok(v) = SecretStoreConfig::from_cr_spec(&spec) {
                config
                    .ensure_project(cr.metadata.project.as_deref())
                    .secret_stores
                    .insert(name.to_string(), v);
            }
        }
        _ => {}
    }
}

/// Removes one builtin resource from the legacy config snapshot.
pub fn remove_from_config_snapshot(
    config: &mut OrchestratorConfig,
    kind: &str,
    project: Option<&str>,
    name: &str,
) {
    match kind {
        "Agent" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.agents.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.agents.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "Workflow" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.workflows.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.workflows.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "Workspace" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.workspaces.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.workspaces.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "Project" => {
            config.projects.remove(name);
        }
        "StepTemplate" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.step_templates.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.step_templates.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "ExecutionProfile" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.execution_profiles.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.execution_profiles.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "EnvStore" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.env_stores.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.env_stores.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        "SecretStore" => {
            if let Some(project_id) = project {
                if let Some(project) = config.projects.get_mut(project_id) {
                    project.secret_stores.remove(name);
                }
            } else {
                for project in config.projects.values_mut() {
                    if project.secret_stores.remove(name).is_some() {
                        return;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Reconciles every builtin resource kind from the store into legacy config fields.
pub fn reconcile_all_builtins(config: &mut OrchestratorConfig) {
    reconcile_builtin_kind(config, "Project");
    reconcile_builtin_kind(config, "RuntimePolicy");
}

/// Restores metadata from a previous resource store snapshot when available.
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

/// Rebuilds the resource store from the current legacy config snapshot.
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
        for (name, profile) in &project.execution_profiles {
            config.resource_store.put(make_cr(
                "ExecutionProfile",
                name,
                Some(project_id.clone()),
                profile.to_cr_spec(),
                &now,
            ));
        }
        for (name, store) in &project.env_stores {
            config.resource_store.put(make_cr(
                "EnvStore",
                name,
                Some(project_id.clone()),
                store.to_cr_spec(),
                &now,
            ));
        }
        for (name, store) in &project.secret_stores {
            config.resource_store.put(make_cr(
                "SecretStore",
                name,
                Some(project_id.clone()),
                store.to_cr_spec(),
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

    // RuntimePolicy is NOT seeded here — it is preserved from the old store
    // during normalize_config, or loaded from the resources table.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentConfig, DEFAULT_PROJECT_ID, EnvStoreConfig, OrchestratorConfig, ProjectConfig,
        StepTemplateConfig, WorkspaceConfig,
    };
    use crate::crd::projection::CrdProjectable;

    fn make_default_config_with_project() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        config.ensure_project(None);
        config
    }

    fn make_workflow_config() -> crate::config::WorkflowConfig {
        // WorkflowConfig doesn't derive Default; create a minimal one via deserialization
        serde_json::from_value(serde_json::json!({
            "steps": [],
            "loop": { "mode": "once" }
        }))
        .expect("minimal workflow config should deserialize")
    }

    // ── remove_from_config_snapshot ─────────────────────────────

    #[test]
    fn remove_agent_from_snapshot_with_project() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .agents
            .insert("ag1".to_string(), AgentConfig::new());
        remove_from_config_snapshot(&mut config, "Agent", Some(DEFAULT_PROJECT_ID), "ag1");
        assert!(!config.default_project().unwrap().agents.contains_key("ag1"));
    }

    #[test]
    fn remove_agent_from_snapshot_without_project_searches_all() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .agents
            .insert("ag2".to_string(), AgentConfig::new());
        remove_from_config_snapshot(&mut config, "Agent", None, "ag2");
        assert!(!config.default_project().unwrap().agents.contains_key("ag2"));
    }

    #[test]
    fn remove_workflow_from_snapshot() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .workflows
            .insert("wf1".to_string(), make_workflow_config());
        remove_from_config_snapshot(&mut config, "Workflow", None, "wf1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .workflows
                .contains_key("wf1")
        );
    }

    #[test]
    fn remove_workflow_from_snapshot_with_project() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .workflows
            .insert("wf2".to_string(), make_workflow_config());
        remove_from_config_snapshot(&mut config, "Workflow", Some(DEFAULT_PROJECT_ID), "wf2");
        assert!(
            !config
                .default_project()
                .unwrap()
                .workflows
                .contains_key("wf2")
        );
    }

    #[test]
    fn remove_workspace_from_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).workspaces.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: "/ws".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
            },
        );
        remove_from_config_snapshot(&mut config, "Workspace", None, "ws1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .workspaces
                .contains_key("ws1")
        );
    }

    #[test]
    fn remove_workspace_from_snapshot_with_project() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).workspaces.insert(
            "ws2".to_string(),
            WorkspaceConfig {
                root_path: "/ws2".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
            },
        );
        remove_from_config_snapshot(&mut config, "Workspace", Some(DEFAULT_PROJECT_ID), "ws2");
        assert!(
            !config
                .default_project()
                .unwrap()
                .workspaces
                .contains_key("ws2")
        );
    }

    #[test]
    fn remove_project_from_snapshot() {
        let mut config = OrchestratorConfig::default();
        config.ensure_project(Some("my-proj"));
        assert!(config.projects.contains_key("my-proj"));
        remove_from_config_snapshot(&mut config, "Project", None, "my-proj");
        assert!(!config.projects.contains_key("my-proj"));
    }

    #[test]
    fn remove_step_template_from_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).step_templates.insert(
            "tmpl1".to_string(),
            StepTemplateConfig {
                prompt: "test".to_string(),
                description: None,
            },
        );
        remove_from_config_snapshot(&mut config, "StepTemplate", None, "tmpl1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .step_templates
                .contains_key("tmpl1")
        );
    }

    #[test]
    fn remove_step_template_from_snapshot_with_project() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).step_templates.insert(
            "tmpl2".to_string(),
            StepTemplateConfig {
                prompt: "p".to_string(),
                description: None,
            },
        );
        remove_from_config_snapshot(
            &mut config,
            "StepTemplate",
            Some(DEFAULT_PROJECT_ID),
            "tmpl2",
        );
        assert!(
            !config
                .default_project()
                .unwrap()
                .step_templates
                .contains_key("tmpl2")
        );
    }

    #[test]
    fn remove_execution_profile_from_snapshot() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .execution_profiles
            .insert("ep1".to_string(), ExecutionProfileConfig::default());
        remove_from_config_snapshot(&mut config, "ExecutionProfile", None, "ep1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .execution_profiles
                .contains_key("ep1")
        );
    }

    #[test]
    fn remove_execution_profile_from_snapshot_with_project() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .execution_profiles
            .insert("ep2".to_string(), ExecutionProfileConfig::default());
        remove_from_config_snapshot(
            &mut config,
            "ExecutionProfile",
            Some(DEFAULT_PROJECT_ID),
            "ep2",
        );
        assert!(
            !config
                .default_project()
                .unwrap()
                .execution_profiles
                .contains_key("ep2")
        );
    }

    #[test]
    fn remove_env_store_from_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).env_stores.insert(
            "env1".to_string(),
            EnvStoreConfig {
                data: Default::default(),
            },
        );
        remove_from_config_snapshot(&mut config, "EnvStore", None, "env1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .env_stores
                .contains_key("env1")
        );
    }

    #[test]
    fn remove_secret_store_from_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).secret_stores.insert(
            "sec1".to_string(),
            SecretStoreConfig {
                data: Default::default(),
            },
        );
        remove_from_config_snapshot(&mut config, "SecretStore", None, "sec1");
        assert!(
            !config
                .default_project()
                .unwrap()
                .secret_stores
                .contains_key("sec1")
        );
    }

    #[test]
    fn remove_env_store_with_project() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).env_stores.insert(
            "env2".to_string(),
            EnvStoreConfig {
                data: Default::default(),
            },
        );
        remove_from_config_snapshot(&mut config, "EnvStore", Some(DEFAULT_PROJECT_ID), "env2");
        assert!(
            !config
                .default_project()
                .unwrap()
                .env_stores
                .contains_key("env2")
        );
    }

    #[test]
    fn remove_unknown_kind_is_noop() {
        let mut config = make_default_config_with_project();
        // Should not panic
        remove_from_config_snapshot(&mut config, "UnknownKind", None, "whatever");
    }

    #[test]
    fn remove_nonexistent_resource_is_noop() {
        let mut config = make_default_config_with_project();
        // Should not panic even when resource doesn't exist
        remove_from_config_snapshot(&mut config, "Agent", None, "no-such-agent");
    }

    // ── reconcile_builtin_kind ──────────────────────────────────

    #[test]
    fn reconcile_builtin_kind_project_syncs_description() {
        let mut config = OrchestratorConfig::default();
        config.ensure_project(Some("test-proj"));
        // Put a Project CR in the store with a description
        let project_config = ProjectConfig {
            description: Some("updated desc".to_string()),
            ..Default::default()
        };
        let cr = crate::crd::types::CustomResource {
            kind: "Project".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "test-proj".to_string(),
                project: Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
                labels: None,
                annotations: None,
            },
            spec: project_config.to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        config.resource_store.put(cr);
        reconcile_builtin_kind(&mut config, "Project");
        assert_eq!(
            config.projects["test-proj"].description.as_deref(),
            Some("updated desc")
        );
    }

    #[test]
    fn reconcile_builtin_kind_runtime_policy_is_noop() {
        let mut config = OrchestratorConfig::default();
        // Should not panic
        reconcile_builtin_kind(&mut config, "RuntimePolicy");
    }

    #[test]
    fn reconcile_builtin_kind_unknown_is_noop() {
        let mut config = OrchestratorConfig::default();
        reconcile_builtin_kind(&mut config, "SomethingElse");
    }

    // ── reconcile_all_builtins ──────────────────────────────────

    #[test]
    fn reconcile_all_builtins_does_not_panic() {
        let mut config = OrchestratorConfig::default();
        reconcile_all_builtins(&mut config);
    }

    // ── seed_store_from_config_snapshot ──────────────────────────

    #[test]
    fn seed_agent_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .agents
            .insert("my-agent".to_string(), AgentConfig::new());
        seed_store_from_config_snapshot(&mut config, "Agent", "my-agent", "2024-01-01T00:00:00Z");
        assert!(
            config
                .resource_store
                .get_namespaced("Agent", DEFAULT_PROJECT_ID, "my-agent")
                .is_some()
        );
    }

    #[test]
    fn seed_workflow_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .workflows
            .insert("my-wf".to_string(), make_workflow_config());
        seed_store_from_config_snapshot(&mut config, "Workflow", "my-wf", "2024-01-01T00:00:00Z");
        assert!(
            config
                .resource_store
                .get_namespaced("Workflow", DEFAULT_PROJECT_ID, "my-wf")
                .is_some()
        );
    }

    #[test]
    fn seed_workspace_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).workspaces.insert(
            "my-ws".to_string(),
            WorkspaceConfig {
                root_path: "/ws".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
            },
        );
        seed_store_from_config_snapshot(&mut config, "Workspace", "my-ws", "2024-01-01T00:00:00Z");
        assert!(
            config
                .resource_store
                .get_namespaced("Workspace", DEFAULT_PROJECT_ID, "my-ws")
                .is_some()
        );
    }

    #[test]
    fn seed_step_template_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).step_templates.insert(
            "my-tmpl".to_string(),
            StepTemplateConfig {
                prompt: "p".to_string(),
                description: None,
            },
        );
        seed_store_from_config_snapshot(
            &mut config,
            "StepTemplate",
            "my-tmpl",
            "2024-01-01T00:00:00Z",
        );
        assert!(
            config
                .resource_store
                .get_namespaced("StepTemplate", DEFAULT_PROJECT_ID, "my-tmpl")
                .is_some()
        );
    }

    #[test]
    fn seed_execution_profile_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .execution_profiles
            .insert("my-ep".to_string(), ExecutionProfileConfig::default());
        seed_store_from_config_snapshot(
            &mut config,
            "ExecutionProfile",
            "my-ep",
            "2024-01-01T00:00:00Z",
        );
        assert!(
            config
                .resource_store
                .get_namespaced("ExecutionProfile", DEFAULT_PROJECT_ID, "my-ep")
                .is_some()
        );
    }

    #[test]
    fn seed_env_store_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).env_stores.insert(
            "my-env".to_string(),
            EnvStoreConfig {
                data: Default::default(),
            },
        );
        seed_store_from_config_snapshot(&mut config, "EnvStore", "my-env", "2024-01-01T00:00:00Z");
        assert!(
            config
                .resource_store
                .get_namespaced("EnvStore", DEFAULT_PROJECT_ID, "my-env")
                .is_some()
        );
    }

    #[test]
    fn seed_env_store_not_found_in_secret_stores() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).secret_stores.insert(
            "secret-env".to_string(),
            SecretStoreConfig {
                data: Default::default(),
            },
        );
        seed_store_from_config_snapshot(
            &mut config,
            "EnvStore",
            "secret-env",
            "2024-01-01T00:00:00Z",
        );
        // Should NOT seed because the name only exists in secret_stores
        assert!(
            config
                .resource_store
                .get_namespaced("EnvStore", DEFAULT_PROJECT_ID, "secret-env")
                .is_none()
        );
    }

    #[test]
    fn seed_secret_store_from_config_snapshot() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).secret_stores.insert(
            "my-secret".to_string(),
            SecretStoreConfig {
                data: Default::default(),
            },
        );
        seed_store_from_config_snapshot(
            &mut config,
            "SecretStore",
            "my-secret",
            "2024-01-01T00:00:00Z",
        );
        assert!(
            config
                .resource_store
                .get_namespaced("SecretStore", DEFAULT_PROJECT_ID, "my-secret")
                .is_some()
        );
    }

    #[test]
    fn seed_secret_store_not_found_in_env_stores() {
        let mut config = make_default_config_with_project();
        config.ensure_project(None).env_stores.insert(
            "not-secret".to_string(),
            EnvStoreConfig {
                data: Default::default(),
            },
        );
        seed_store_from_config_snapshot(
            &mut config,
            "SecretStore",
            "not-secret",
            "2024-01-01T00:00:00Z",
        );
        assert!(
            config
                .resource_store
                .get_namespaced("SecretStore", DEFAULT_PROJECT_ID, "not-secret")
                .is_none()
        );
    }

    #[test]
    fn seed_project_from_config_snapshot() {
        let mut config = OrchestratorConfig::default();
        config.ensure_project(Some("seed-proj"));
        seed_store_from_config_snapshot(
            &mut config,
            "Project",
            "seed-proj",
            "2024-01-01T00:00:00Z",
        );
        assert!(config.resource_store.get("Project", "seed-proj").is_some());
    }

    #[test]
    fn seed_runtime_policy_when_absent() {
        let mut config = OrchestratorConfig::default();
        seed_store_from_config_snapshot(
            &mut config,
            "RuntimePolicy",
            "runtime",
            "2024-01-01T00:00:00Z",
        );
        assert!(
            config
                .resource_store
                .get("RuntimePolicy", "runtime")
                .is_some()
        );
    }

    #[test]
    fn seed_unknown_kind_is_noop() {
        let mut config = OrchestratorConfig::default();
        seed_store_from_config_snapshot(
            &mut config,
            "UnknownKind",
            "whatever",
            "2024-01-01T00:00:00Z",
        );
    }

    // ── reconcile_single_resource ───────────────────────────────

    #[test]
    fn reconcile_single_agent_writes_to_config() {
        let mut config = make_default_config_with_project();
        let agent = AgentConfig::new();
        let cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "rec-ag".to_string(),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                labels: None,
                annotations: None,
            },
            spec: agent.to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        config.resource_store.put(cr);
        reconcile_single_resource(&mut config, "Agent", Some(DEFAULT_PROJECT_ID), "rec-ag");
        assert!(
            config
                .default_project()
                .unwrap()
                .agents
                .contains_key("rec-ag")
        );
    }

    #[test]
    fn reconcile_single_workspace_writes_to_config() {
        let mut config = make_default_config_with_project();
        let ws = WorkspaceConfig {
            root_path: "/rec".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
            health_policy: Default::default(),
            artifacts_dir: None,
        };
        let cr = crate::crd::types::CustomResource {
            kind: "Workspace".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "rec-ws".to_string(),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                labels: None,
                annotations: None,
            },
            spec: ws.to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        config.resource_store.put(cr);
        reconcile_single_resource(&mut config, "Workspace", Some(DEFAULT_PROJECT_ID), "rec-ws");
        assert!(
            config
                .default_project()
                .unwrap()
                .workspaces
                .contains_key("rec-ws")
        );
    }

    #[test]
    fn reconcile_single_resource_noop_for_missing_cr() {
        let mut config = make_default_config_with_project();
        reconcile_single_resource(&mut config, "Agent", Some(DEFAULT_PROJECT_ID), "no-such");
        assert!(
            !config
                .default_project()
                .unwrap()
                .agents
                .contains_key("no-such")
        );
    }

    #[test]
    fn reconcile_single_runtime_policy_is_noop() {
        let mut config = make_default_config_with_project();
        // Should not panic even when we reconcile RuntimePolicy
        reconcile_single_resource(&mut config, "RuntimePolicy", None, "runtime");
    }

    // ── restore_metadata_from_previous_store ────────────────────

    #[test]
    fn restore_metadata_copies_labels_and_annotations() {
        let mut config = make_default_config_with_project();
        let agent = AgentConfig::new();

        // Put CR in new store without labels
        let cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "meta-ag".to_string(),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                labels: None,
                annotations: None,
            },
            spec: agent.to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        config.resource_store.put(cr);

        // Old store has labels
        let mut old_store = crate::crd::store::ResourceStore::default();
        let old_cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "meta-ag".to_string(),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                labels: Some([("env".to_string(), "prod".to_string())].into()),
                annotations: Some([("note".to_string(), "important".to_string())].into()),
            },
            spec: AgentConfig::new().to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        old_store.put(old_cr);

        restore_metadata_from_previous_store(&mut config, &old_store);

        let restored = config
            .resource_store
            .get_namespaced("Agent", DEFAULT_PROJECT_ID, "meta-ag")
            .expect("should exist");
        assert_eq!(
            restored
                .metadata
                .labels
                .as_ref()
                .unwrap()
                .get("env")
                .unwrap(),
            "prod"
        );
        assert_eq!(
            restored
                .metadata
                .annotations
                .as_ref()
                .unwrap()
                .get("note")
                .unwrap(),
            "important"
        );
    }

    #[test]
    fn restore_metadata_skips_empty_labels_and_annotations() {
        let mut config = make_default_config_with_project();
        let agent = AgentConfig::new();
        let cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "no-meta-ag".to_string(),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                labels: None,
                annotations: None,
            },
            spec: agent.to_cr_spec(),
            generation: 1,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };
        config.resource_store.put(cr);

        // Old store has no labels/annotations
        let old_store = crate::crd::store::ResourceStore::default();
        restore_metadata_from_previous_store(&mut config, &old_store);

        let stored = config
            .resource_store
            .get_namespaced("Agent", DEFAULT_PROJECT_ID, "no-meta-ag")
            .expect("should exist");
        assert!(stored.metadata.labels.is_none());
        assert!(stored.metadata.annotations.is_none());
    }

    // ── sync_config_snapshot_to_store ────────────────────────────

    #[test]
    fn sync_config_snapshot_to_store_populates_resource_store() {
        let mut config = make_default_config_with_project();
        config
            .ensure_project(None)
            .agents
            .insert("sync-ag".to_string(), AgentConfig::new());
        config
            .ensure_project(None)
            .workflows
            .insert("sync-wf".to_string(), make_workflow_config());
        config.ensure_project(None).workspaces.insert(
            "sync-ws".to_string(),
            WorkspaceConfig {
                root_path: "/sync".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
            },
        );
        config.ensure_project(None).step_templates.insert(
            "sync-tmpl".to_string(),
            StepTemplateConfig {
                prompt: "p".to_string(),
                description: None,
            },
        );
        config
            .ensure_project(None)
            .execution_profiles
            .insert("sync-ep".to_string(), ExecutionProfileConfig::default());
        config.ensure_project(None).env_stores.insert(
            "sync-env".to_string(),
            EnvStoreConfig {
                data: Default::default(),
            },
        );
        config.ensure_project(None).secret_stores.insert(
            "sync-sec".to_string(),
            SecretStoreConfig {
                data: Default::default(),
            },
        );

        sync_config_snapshot_to_store(&mut config);

        assert!(
            config
                .resource_store
                .get_namespaced("Agent", DEFAULT_PROJECT_ID, "sync-ag")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("Workflow", DEFAULT_PROJECT_ID, "sync-wf")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("Workspace", DEFAULT_PROJECT_ID, "sync-ws")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("StepTemplate", DEFAULT_PROJECT_ID, "sync-tmpl")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("ExecutionProfile", DEFAULT_PROJECT_ID, "sync-ep")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("EnvStore", DEFAULT_PROJECT_ID, "sync-env")
                .is_some()
        );
        assert!(
            config
                .resource_store
                .get_namespaced("SecretStore", DEFAULT_PROJECT_ID, "sync-sec")
                .is_some()
        );
        // Also creates Project entry
        assert!(
            config
                .resource_store
                .get("Project", DEFAULT_PROJECT_ID)
                .is_some()
        );
    }
}
