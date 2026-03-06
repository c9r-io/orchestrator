use crate::config::{
    AgentConfig, ConfigDefaults, EnvStoreConfig, OrchestratorConfig, ProjectConfig,
    StepTemplateConfig, WorkflowConfig, WorkspaceConfig,
};
use crate::crd::projection::{CrdProjectable, RuntimePolicyProjection, SecretStoreProjection};

/// Project a builtin kind from the ResourceStore back to the legacy config fields.
///
/// This ensures that all code reading `config.agents`, `config.workflows`, etc.
/// continues to work without modification.
pub fn project_builtin_kind(config: &mut OrchestratorConfig, kind: &str) {
    match kind {
        "Agent" => config.agents = config.resource_store.project_map::<AgentConfig>(),
        "Workflow" => config.workflows = config.resource_store.project_map::<WorkflowConfig>(),
        "Workspace" => config.workspaces = config.resource_store.project_map::<WorkspaceConfig>(),
        "Project" => {
            // Project projection must preserve sub-resources (agents/workflows/workspaces)
            // that are managed by apply_to_project, not by the Project CRD spec itself.
            let projected = config.resource_store.project_map::<ProjectConfig>();
            for (name, proj) in projected {
                config
                    .projects
                    .entry(name)
                    .and_modify(|existing| {
                        existing.description = proj.description.clone();
                    })
                    .or_insert(proj);
            }
            // Remove projects that no longer exist in the store
            let store_keys: std::collections::HashSet<String> = config
                .resource_store
                .list_by_kind("Project")
                .iter()
                .map(|cr| cr.metadata.name.clone())
                .collect();
            config.projects.retain(|name, _| store_keys.contains(name));
        }
        "Defaults" => {
            if let Some(d) = config.resource_store.project_singleton::<ConfigDefaults>() {
                config.defaults = d;
            }
        }
        "RuntimePolicy" => {
            if let Some(rp) = config
                .resource_store
                .project_singleton::<RuntimePolicyProjection>()
            {
                config.runner = rp.runner;
                config.resume = rp.resume;
            }
        }
        "StepTemplate" => {
            config.step_templates = config.resource_store.project_map::<StepTemplateConfig>()
        }
        "EnvStore" => {
            // Merge non-sensitive env stores
            let env_map = config.resource_store.project_map::<EnvStoreConfig>();
            // Keep existing sensitive stores, replace non-sensitive ones
            config.env_stores.retain(|_, v| v.sensitive);
            config.env_stores.extend(env_map);
        }
        "SecretStore" => {
            // Merge sensitive stores
            let secret_map = config.resource_store.project_map::<SecretStoreProjection>();
            // Keep existing non-sensitive stores, replace sensitive ones
            config.env_stores.retain(|_, v| !v.sensitive);
            for (name, proj) in secret_map {
                config.env_stores.insert(name, proj.0);
            }
        }
        // WorkflowStore and StoreBackendProvider have no legacy config fields;
        // they live exclusively in the ResourceStore and are accessed via StoreManager.
        "WorkflowStore" | "StoreBackendProvider" => {}
        _ => {} // User CRD, no projection needed
    }
}

/// Seed a single resource entry from a legacy config field into the ResourceStore.
///
/// Called by `apply_to_store` / `delete_from_store` when the store doesn't have
/// the entry yet but the legacy field might. This ensures correct change detection.
pub fn seed_store_from_legacy(config: &mut OrchestratorConfig, kind: &str, name: &str, now: &str) {
    use crate::cli_types::ResourceMetadata;
    use crate::crd::projection::CrdProjectable;
    use crate::crd::types::CustomResource;

    let make_cr = |spec: serde_json::Value| -> CustomResource {
        CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    };

    match kind {
        "Agent" => {
            if let Some(v) = config.agents.get(name) {
                config.resource_store.put(make_cr(v.to_cr_spec()));
            }
        }
        "Workflow" => {
            if let Some(v) = config.workflows.get(name) {
                config.resource_store.put(make_cr(v.to_cr_spec()));
            }
        }
        "Workspace" => {
            if let Some(v) = config.workspaces.get(name) {
                config.resource_store.put(make_cr(v.to_cr_spec()));
            }
        }
        "Project" => {
            if let Some(v) = config.projects.get(name) {
                config.resource_store.put(make_cr(v.to_cr_spec()));
            }
        }
        "Defaults" => {
            config
                .resource_store
                .put(make_cr(config.defaults.to_cr_spec()));
        }
        "RuntimePolicy" => {
            let rp = RuntimePolicyProjection {
                runner: config.runner.clone(),
                resume: config.resume.clone(),
            };
            config.resource_store.put(make_cr(rp.to_cr_spec()));
        }
        "StepTemplate" => {
            if let Some(v) = config.step_templates.get(name) {
                config.resource_store.put(make_cr(v.to_cr_spec()));
            }
        }
        "EnvStore" => {
            if let Some(v) = config.env_stores.get(name) {
                if !v.sensitive {
                    config.resource_store.put(make_cr(v.to_cr_spec()));
                }
            }
        }
        "SecretStore" => {
            if let Some(v) = config.env_stores.get(name) {
                if v.sensitive {
                    let proj = SecretStoreProjection(v.clone());
                    config.resource_store.put(make_cr(proj.to_cr_spec()));
                }
            }
        }
        _ => {}
    }
}

/// Write back a single resource entry from the ResourceStore to the legacy config field.
///
/// Unlike `project_builtin_kind` which replaces the entire map, this only
/// inserts/updates one entry — safe to call when the store is incomplete.
pub fn write_back_single(config: &mut OrchestratorConfig, kind: &str, name: &str) {
    use crate::crd::projection::CrdProjectable;

    let Some(cr) = config.resource_store.get(kind, name) else {
        return;
    };
    let spec = cr.spec.clone();

    match kind {
        "Agent" => {
            if let Ok(v) = AgentConfig::from_cr_spec(&spec) {
                config.agents.insert(name.to_string(), v);
            }
        }
        "Workflow" => {
            if let Ok(v) = WorkflowConfig::from_cr_spec(&spec) {
                config.workflows.insert(name.to_string(), v);
            }
        }
        "Workspace" => {
            if let Ok(v) = WorkspaceConfig::from_cr_spec(&spec) {
                config.workspaces.insert(name.to_string(), v);
            }
        }
        "Project" => {
            if let Ok(v) = ProjectConfig::from_cr_spec(&spec) {
                config
                    .projects
                    .entry(name.to_string())
                    .and_modify(|existing| {
                        existing.description = v.description.clone();
                    })
                    .or_insert(v);
            }
        }
        "Defaults" => {
            if let Ok(v) = ConfigDefaults::from_cr_spec(&spec) {
                config.defaults = v;
            }
        }
        "RuntimePolicy" => {
            if let Ok(rp) = RuntimePolicyProjection::from_cr_spec(&spec) {
                config.runner = rp.runner;
                config.resume = rp.resume;
            }
        }
        "StepTemplate" => {
            if let Ok(v) = StepTemplateConfig::from_cr_spec(&spec) {
                config.step_templates.insert(name.to_string(), v);
            }
        }
        "EnvStore" => {
            if let Ok(v) = EnvStoreConfig::from_cr_spec(&spec) {
                config.env_stores.insert(name.to_string(), v);
            }
        }
        "SecretStore" => {
            if let Ok(v) = SecretStoreProjection::from_cr_spec(&spec) {
                config.env_stores.insert(name.to_string(), v.0);
            }
        }
        _ => {}
    }
}

/// Remove a single resource entry from the legacy config field.
pub fn remove_from_legacy(config: &mut OrchestratorConfig, kind: &str, name: &str) {
    match kind {
        "Agent" => {
            config.agents.remove(name);
        }
        "Workflow" => {
            config.workflows.remove(name);
        }
        "Workspace" => {
            config.workspaces.remove(name);
        }
        "Project" => {
            config.projects.remove(name);
        }
        "StepTemplate" => {
            config.step_templates.remove(name);
        }
        "EnvStore" | "SecretStore" => {
            config.env_stores.remove(name);
        }
        // Singletons (Defaults, RuntimePolicy) cannot be deleted
        _ => {}
    }
}

/// Project all builtin kinds from the ResourceStore to legacy fields.
pub fn project_all_builtins(config: &mut OrchestratorConfig) {
    for kind in &[
        "Agent",
        "Workflow",
        "Workspace",
        "Project",
        "Defaults",
        "RuntimePolicy",
        "StepTemplate",
        "EnvStore",
        "SecretStore",
        "WorkflowStore",
        "StoreBackendProvider",
    ] {
        project_builtin_kind(config, kind);
    }
}

/// Restore resource metadata (labels, annotations) from an old ResourceStore
/// into the current one.
///
/// Called after `sync_legacy_to_store` rebuilds the store from spec-only legacy
/// fields — this merges back any labels/annotations that were stored in the
/// previous resource store so they survive normalization.
pub fn restore_metadata_from_old_store(
    config: &mut OrchestratorConfig,
    old_store: &crate::crd::store::ResourceStore,
) {
    for (key, old_cr) in old_store.resources() {
        // Only restore metadata when the old CR actually had labels or annotations
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

        // Look up the corresponding new CR and merge metadata
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

/// Sync legacy config fields into the ResourceStore.
///
/// Used during migration: when `resource_store` is empty but legacy fields have data,
/// we populate the store from the legacy data.
pub fn sync_legacy_to_store(config: &mut OrchestratorConfig) {
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::CustomResource;

    let now = chrono::Utc::now().to_rfc3339();

    // Helper to create a CR from a typed config
    fn make_cr(kind: &str, name: &str, spec: serde_json::Value, now: &str) -> CustomResource {
        CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }
    }

    // Agents
    for (name, agent) in &config.agents {
        let spec = agent.to_cr_spec();
        config
            .resource_store
            .put(make_cr("Agent", name, spec, &now));
    }

    // Workflows
    for (name, workflow) in &config.workflows {
        let spec = workflow.to_cr_spec();
        config
            .resource_store
            .put(make_cr("Workflow", name, spec, &now));
    }

    // Workspaces
    for (name, workspace) in &config.workspaces {
        let spec = workspace.to_cr_spec();
        config
            .resource_store
            .put(make_cr("Workspace", name, spec, &now));
    }

    // Projects
    for (name, project) in &config.projects {
        let spec = project.to_cr_spec();
        config
            .resource_store
            .put(make_cr("Project", name, spec, &now));
    }

    // Defaults (singleton)
    {
        let spec = config.defaults.to_cr_spec();
        config
            .resource_store
            .put(make_cr("Defaults", "defaults", spec, &now));
    }

    // RuntimePolicy (singleton)
    {
        let rp = RuntimePolicyProjection {
            runner: config.runner.clone(),
            resume: config.resume.clone(),
        };
        let spec = rp.to_cr_spec();
        config
            .resource_store
            .put(make_cr("RuntimePolicy", "runtime", spec, &now));
    }

    // StepTemplates
    for (name, tmpl) in &config.step_templates {
        let spec = tmpl.to_cr_spec();
        config
            .resource_store
            .put(make_cr("StepTemplate", name, spec, &now));
    }

    // EnvStores (non-sensitive)
    for (name, store) in &config.env_stores {
        if !store.sensitive {
            let spec = store.to_cr_spec();
            config
                .resource_store
                .put(make_cr("EnvStore", name, spec, &now));
        }
    }

    // SecretStores (sensitive)
    for (name, store) in &config.env_stores {
        if store.sensitive {
            let proj = SecretStoreProjection(store.clone());
            let spec = proj.to_cr_spec();
            config
                .resource_store
                .put(make_cr("SecretStore", name, spec, &now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use crate::crd::projection::CrdProjectable;

    #[test]
    fn project_agents_to_legacy_fields() {
        let mut config = OrchestratorConfig::default();

        // Put an agent into the resource store
        let agent = AgentConfig {
            command: "echo {prompt}".to_string(),
            capabilities: vec!["plan".to_string()],
            ..Default::default()
        };
        let spec = agent.to_cr_spec();
        let cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "test-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        config.resource_store.put(cr);

        // Project back
        project_builtin_kind(&mut config, "Agent");

        assert_eq!(config.agents.len(), 1);
        let loaded = config.agents.get("test-agent").expect("should exist");
        assert_eq!(loaded.command, "echo {prompt}");
    }

    #[test]
    fn sync_legacy_to_store_round_trips() {
        let mut config = OrchestratorConfig::default();
        config.agents.insert(
            "my-agent".to_string(),
            AgentConfig {
                command: "echo {prompt}".to_string(),
                ..Default::default()
            },
        );

        assert!(config.resource_store.is_empty());
        sync_legacy_to_store(&mut config);
        assert!(!config.resource_store.is_empty());

        // Verify the agent is in the store
        let cr = config.resource_store.get("Agent", "my-agent");
        assert!(cr.is_some());
    }

    #[test]
    fn project_all_builtins_syncs_everything() {
        let mut config = OrchestratorConfig::default();

        // Seed store with an agent
        let agent = AgentConfig {
            command: "echo test".to_string(),
            ..Default::default()
        };
        let cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "bulk-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: agent.to_cr_spec(),
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        config.resource_store.put(cr);

        project_all_builtins(&mut config);
        assert!(config.agents.contains_key("bulk-agent"));
    }

    // ── Helper ──────────────────────────────────────────────────────────

    fn make_test_cr(
        kind: &str,
        name: &str,
        spec: serde_json::Value,
    ) -> crate::crd::types::CustomResource {
        crate::crd::types::CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    // ── write_back_single tests ─────────────────────────────────────────

    #[test]
    fn write_back_single_workspace() {
        let mut config = OrchestratorConfig::default();
        let ws = WorkspaceConfig {
            root_path: "/ws".to_string(),
            qa_targets: vec!["src".to_string()],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        config
            .resource_store
            .put(make_test_cr("Workspace", "ws1", ws.to_cr_spec()));
        write_back_single(&mut config, "Workspace", "ws1");
        assert_eq!(config.workspaces.get("ws1").unwrap().root_path, "/ws");
    }

    #[test]
    fn write_back_single_workflow() {
        let mut config = OrchestratorConfig::default();
        let wf =
            crate::config_load::tests::make_workflow(vec![crate::config_load::tests::make_step(
                "plan", true,
            )]);
        config
            .resource_store
            .put(make_test_cr("Workflow", "wf1", wf.to_cr_spec()));
        write_back_single(&mut config, "Workflow", "wf1");
        assert!(config.workflows.contains_key("wf1"));
    }

    #[test]
    fn write_back_single_defaults() {
        let mut config = OrchestratorConfig::default();
        let d = ConfigDefaults {
            project: "p".to_string(),
            workspace: "w".to_string(),
            workflow: "wf".to_string(),
        };
        config
            .resource_store
            .put(make_test_cr("Defaults", "defaults", d.to_cr_spec()));
        write_back_single(&mut config, "Defaults", "defaults");
        assert_eq!(config.defaults.workspace, "w");
    }

    #[test]
    fn write_back_single_runtime_policy() {
        use crate::config::ResumeConfig;
        use crate::crd::projection::RuntimePolicyProjection;
        let mut config = OrchestratorConfig::default();
        let rp = RuntimePolicyProjection {
            runner: crate::config::RunnerConfig {
                shell: "/bin/zsh".to_string(),
                ..Default::default()
            },
            resume: ResumeConfig { auto: true },
        };
        config
            .resource_store
            .put(make_test_cr("RuntimePolicy", "runtime", rp.to_cr_spec()));
        write_back_single(&mut config, "RuntimePolicy", "runtime");
        assert_eq!(config.runner.shell, "/bin/zsh");
        assert!(config.resume.auto);
    }

    #[test]
    fn write_back_single_step_template() {
        let mut config = OrchestratorConfig::default();
        let st = crate::config::StepTemplateConfig {
            prompt: "do qa".to_string(),
            description: Some("desc".to_string()),
        };
        config
            .resource_store
            .put(make_test_cr("StepTemplate", "tpl", st.to_cr_spec()));
        write_back_single(&mut config, "StepTemplate", "tpl");
        assert_eq!(config.step_templates.get("tpl").unwrap().prompt, "do qa");
    }

    #[test]
    fn write_back_single_env_store() {
        let mut config = OrchestratorConfig::default();
        let es = EnvStoreConfig {
            data: [("K".to_string(), "V".to_string())].into(),
            sensitive: false,
        };
        config
            .resource_store
            .put(make_test_cr("EnvStore", "env1", es.to_cr_spec()));
        write_back_single(&mut config, "EnvStore", "env1");
        let loaded = config.env_stores.get("env1").unwrap();
        assert_eq!(loaded.data.get("K").unwrap(), "V");
        assert!(!loaded.sensitive);
    }

    #[test]
    fn write_back_single_secret_store() {
        let mut config = OrchestratorConfig::default();
        let ss = SecretStoreProjection(EnvStoreConfig {
            data: [("SECRET".to_string(), "val".to_string())].into(),
            sensitive: true,
        });
        config
            .resource_store
            .put(make_test_cr("SecretStore", "sec1", ss.to_cr_spec()));
        write_back_single(&mut config, "SecretStore", "sec1");
        let loaded = config.env_stores.get("sec1").unwrap();
        assert_eq!(loaded.data.get("SECRET").unwrap(), "val");
        assert!(loaded.sensitive);
    }

    #[test]
    fn write_back_single_project_preserves_sub_resources() {
        let mut config = OrchestratorConfig::default();
        // Pre-existing project with sub-resources
        let mut proj_ws = std::collections::HashMap::new();
        proj_ws.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: "/p".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        config.projects.insert(
            "proj1".to_string(),
            ProjectConfig {
                description: Some("old".to_string()),
                workspaces: proj_ws,
                agents: Default::default(),
                workflows: Default::default(),
            },
        );
        // Store has updated description only
        let proj = ProjectConfig {
            description: Some("new desc".to_string()),
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
        };
        config
            .resource_store
            .put(make_test_cr("Project", "proj1", proj.to_cr_spec()));
        write_back_single(&mut config, "Project", "proj1");

        let loaded = config.projects.get("proj1").unwrap();
        assert_eq!(loaded.description.as_deref(), Some("new desc"));
        assert!(
            loaded.workspaces.contains_key("ws1"),
            "sub-resources preserved"
        );
    }

    // ── remove_from_legacy tests ────────────────────────────────────────

    #[test]
    fn remove_from_legacy_agent() {
        let mut config = OrchestratorConfig::default();
        config.agents.insert(
            "rm-ag".to_string(),
            AgentConfig {
                command: "echo".to_string(),
                ..Default::default()
            },
        );
        remove_from_legacy(&mut config, "Agent", "rm-ag");
        assert!(!config.agents.contains_key("rm-ag"));
    }

    #[test]
    fn remove_from_legacy_workflow() {
        let mut config = OrchestratorConfig::default();
        config.workflows.insert(
            "rm-wf".to_string(),
            crate::config_load::tests::make_workflow(vec![]),
        );
        remove_from_legacy(&mut config, "Workflow", "rm-wf");
        assert!(!config.workflows.contains_key("rm-wf"));
    }

    #[test]
    fn remove_from_legacy_workspace() {
        let mut config = OrchestratorConfig::default();
        config.workspaces.insert(
            "rm-ws".to_string(),
            WorkspaceConfig {
                root_path: "/x".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        remove_from_legacy(&mut config, "Workspace", "rm-ws");
        assert!(!config.workspaces.contains_key("rm-ws"));
    }

    #[test]
    fn remove_from_legacy_step_template() {
        let mut config = OrchestratorConfig::default();
        config.step_templates.insert(
            "rm-st".to_string(),
            StepTemplateConfig {
                prompt: "x".to_string(),
                description: None,
            },
        );
        remove_from_legacy(&mut config, "StepTemplate", "rm-st");
        assert!(!config.step_templates.contains_key("rm-st"));
    }

    #[test]
    fn remove_from_legacy_env_store() {
        let mut config = OrchestratorConfig::default();
        config.env_stores.insert(
            "rm-env".to_string(),
            EnvStoreConfig {
                data: Default::default(),
                sensitive: false,
            },
        );
        remove_from_legacy(&mut config, "EnvStore", "rm-env");
        assert!(!config.env_stores.contains_key("rm-env"));
    }

    #[test]
    fn remove_from_legacy_ignores_singletons() {
        let mut config = OrchestratorConfig::default();
        let orig_workspace = config.defaults.workspace.clone();
        remove_from_legacy(&mut config, "Defaults", "defaults");
        // Singletons cannot be deleted — value unchanged
        assert_eq!(config.defaults.workspace, orig_workspace);
    }

    // ── seed_store_from_legacy tests ────────────────────────────────────

    #[test]
    fn seed_store_from_legacy_agent() {
        let mut config = OrchestratorConfig::default();
        config.agents.insert(
            "seed-ag".to_string(),
            AgentConfig {
                command: "echo {prompt}".to_string(),
                ..Default::default()
            },
        );
        seed_store_from_legacy(&mut config, "Agent", "seed-ag", "2026-01-01T00:00:00Z");
        assert!(config.resource_store.get("Agent", "seed-ag").is_some());
    }

    #[test]
    fn seed_store_from_legacy_workspace() {
        let mut config = OrchestratorConfig::default();
        config.workspaces.insert(
            "seed-ws".to_string(),
            WorkspaceConfig {
                root_path: "/x".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        seed_store_from_legacy(&mut config, "Workspace", "seed-ws", "2026-01-01T00:00:00Z");
        assert!(config.resource_store.get("Workspace", "seed-ws").is_some());
    }

    #[test]
    fn seed_store_from_legacy_env_store_non_sensitive() {
        let mut config = OrchestratorConfig::default();
        config.env_stores.insert(
            "seed-env".to_string(),
            EnvStoreConfig {
                data: [("K".to_string(), "V".to_string())].into(),
                sensitive: false,
            },
        );
        seed_store_from_legacy(&mut config, "EnvStore", "seed-env", "2026-01-01T00:00:00Z");
        assert!(config.resource_store.get("EnvStore", "seed-env").is_some());
    }

    #[test]
    fn seed_store_from_legacy_secret_store_sensitive() {
        let mut config = OrchestratorConfig::default();
        config.env_stores.insert(
            "seed-sec".to_string(),
            EnvStoreConfig {
                data: [("S".to_string(), "V".to_string())].into(),
                sensitive: true,
            },
        );
        seed_store_from_legacy(
            &mut config,
            "SecretStore",
            "seed-sec",
            "2026-01-01T00:00:00Z",
        );
        assert!(config
            .resource_store
            .get("SecretStore", "seed-sec")
            .is_some());
    }

    #[test]
    fn seed_store_from_legacy_skips_non_sensitive_for_secret_store() {
        let mut config = OrchestratorConfig::default();
        config.env_stores.insert(
            "plain".to_string(),
            EnvStoreConfig {
                data: Default::default(),
                sensitive: false,
            },
        );
        seed_store_from_legacy(&mut config, "SecretStore", "plain", "2026-01-01T00:00:00Z");
        assert!(config.resource_store.get("SecretStore", "plain").is_none());
    }

    #[test]
    fn seed_store_from_legacy_defaults() {
        let mut config = OrchestratorConfig::default();
        config.defaults.workspace = "ws".to_string();
        seed_store_from_legacy(&mut config, "Defaults", "defaults", "2026-01-01T00:00:00Z");
        assert!(config.resource_store.get("Defaults", "defaults").is_some());
    }

    #[test]
    fn seed_store_from_legacy_runtime_policy() {
        let mut config = OrchestratorConfig::default();
        config.runner.shell = "/bin/zsh".to_string();
        seed_store_from_legacy(
            &mut config,
            "RuntimePolicy",
            "runtime",
            "2026-01-01T00:00:00Z",
        );
        assert!(config
            .resource_store
            .get("RuntimePolicy", "runtime")
            .is_some());
    }

    // ── sync_legacy_to_store covers all 9 kinds ─────────────────────────

    #[test]
    fn sync_legacy_to_store_all_nine_kinds() {
        let mut config = OrchestratorConfig::default();
        // Seed each kind in legacy
        config.agents.insert(
            "ag".to_string(),
            AgentConfig {
                command: "echo".to_string(),
                ..Default::default()
            },
        );
        config.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                root_path: "/x".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        config.workflows.insert(
            "wf".to_string(),
            crate::config_load::tests::make_workflow(vec![]),
        );
        config.projects.insert(
            "proj".to_string(),
            ProjectConfig {
                description: Some("test".to_string()),
                workspaces: Default::default(),
                agents: Default::default(),
                workflows: Default::default(),
            },
        );
        config.step_templates.insert(
            "st".to_string(),
            StepTemplateConfig {
                prompt: "p".to_string(),
                description: None,
            },
        );
        config.env_stores.insert(
            "env".to_string(),
            EnvStoreConfig {
                data: Default::default(),
                sensitive: false,
            },
        );
        config.env_stores.insert(
            "sec".to_string(),
            EnvStoreConfig {
                data: Default::default(),
                sensitive: true,
            },
        );
        // Defaults and RuntimePolicy are always present via OrchestratorConfig::default()

        sync_legacy_to_store(&mut config);

        assert!(config.resource_store.get("Agent", "ag").is_some(), "Agent");
        assert!(
            config.resource_store.get("Workspace", "ws").is_some(),
            "Workspace"
        );
        assert!(
            config.resource_store.get("Workflow", "wf").is_some(),
            "Workflow"
        );
        assert!(
            config.resource_store.get("Project", "proj").is_some(),
            "Project"
        );
        assert!(
            config.resource_store.get("StepTemplate", "st").is_some(),
            "StepTemplate"
        );
        assert!(
            config.resource_store.get("EnvStore", "env").is_some(),
            "EnvStore"
        );
        assert!(
            config.resource_store.get("SecretStore", "sec").is_some(),
            "SecretStore"
        );
        assert!(
            config.resource_store.get("Defaults", "defaults").is_some(),
            "Defaults"
        );
        assert!(
            config
                .resource_store
                .get("RuntimePolicy", "runtime")
                .is_some(),
            "RuntimePolicy"
        );
    }

    // ── project_builtin_kind for EnvStore/SecretStore splitting ──────────

    #[test]
    fn project_env_store_preserves_sensitive_stores() {
        let mut config = OrchestratorConfig::default();
        // Pre-existing sensitive store in legacy
        config.env_stores.insert(
            "secret".to_string(),
            EnvStoreConfig {
                data: [("S".to_string(), "V".to_string())].into(),
                sensitive: true,
            },
        );
        // Put a non-sensitive store in the resource store
        let es = EnvStoreConfig {
            data: [("K".to_string(), "V".to_string())].into(),
            sensitive: false,
        };
        config
            .resource_store
            .put(make_test_cr("EnvStore", "plain", es.to_cr_spec()));

        project_builtin_kind(&mut config, "EnvStore");

        assert!(
            config.env_stores.contains_key("plain"),
            "non-sensitive projected"
        );
        assert!(
            config.env_stores.contains_key("secret"),
            "sensitive preserved"
        );
        assert!(config.env_stores.get("secret").unwrap().sensitive);
    }

    #[test]
    fn project_secret_store_preserves_non_sensitive_stores() {
        let mut config = OrchestratorConfig::default();
        // Pre-existing non-sensitive store in legacy
        config.env_stores.insert(
            "plain".to_string(),
            EnvStoreConfig {
                data: [("K".to_string(), "V".to_string())].into(),
                sensitive: false,
            },
        );
        // Put a sensitive store in the resource store
        let ss = SecretStoreProjection(EnvStoreConfig {
            data: [("S".to_string(), "V".to_string())].into(),
            sensitive: true,
        });
        config
            .resource_store
            .put(make_test_cr("SecretStore", "secret", ss.to_cr_spec()));

        project_builtin_kind(&mut config, "SecretStore");

        assert!(
            config.env_stores.contains_key("plain"),
            "non-sensitive preserved"
        );
        assert!(
            config.env_stores.contains_key("secret"),
            "sensitive projected"
        );
        assert!(config.env_stores.get("secret").unwrap().sensitive);
    }
}
