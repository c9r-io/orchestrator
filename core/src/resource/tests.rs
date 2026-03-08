#[cfg(test)]
mod tests {
    use super::super::helpers::metadata_from_parts;
    use super::super::*;
    use crate::cli_types::{
        AgentSpec, OrchestratorResource, ResourceMetadata, ResourceSpec, WorkspaceSpec,
    };
    use crate::config::OrchestratorConfig;
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    use super::super::test_fixtures::{
        agent_manifest, defaults_manifest, make_config, project_manifest, runtime_policy_manifest,
        workflow_manifest, workspace_manifest,
    };

    #[test]
    fn resource_dispatch_maps_workspace_manifest() {
        let resource = dispatch_resource(workspace_manifest("dispatch-ws", "workspace/dispatch"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Workspace);
        assert_eq!(resource.name(), "dispatch-ws");
    }

    #[test]
    fn resource_dispatch_rejects_mismatched_spec_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Agent(Box::new(AgentSpec {
                command: "echo {prompt}".to_string(),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            })),
        };

        let error = dispatch_resource(resource).expect_err("dispatch should fail");
        assert!(error.to_string().contains("mismatch"));
    }

    #[test]
    fn resource_registry_has_nine_entries() {
        let registry = resource_registry();
        assert_eq!(registry.len(), 9);
        let kinds: Vec<ResourceKind> = registry.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::StepTemplate));
        assert!(kinds.contains(&ResourceKind::EnvStore));
        assert!(kinds.contains(&ResourceKind::SecretStore));
    }

    #[test]
    fn resource_trait_validate_rejects_empty_name() {
        let resource = dispatch_resource(workspace_manifest("", "workspace/invalid"))
            .expect("dispatch should succeed");
        let result = resource.validate();
        assert!(result.is_err());
    }

    #[test]
    fn resource_trait_to_yaml_serializes_manifest_shape() {
        let resource = dispatch_resource(workspace_manifest("yaml-ws", "workspace/yaml"))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("yaml serialization should work");
        assert!(yaml.contains("apiVersion: orchestrator.dev/v2"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-ws"));
    }

    #[test]
    fn resource_trait_get_from_reads_existing_config() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let active = read_active_config(&state).expect("state should be readable");
        let resource = RegisteredResource::get_from(&active.config, "default")
            .expect("default workspace should exist");
        assert_eq!(resource.kind(), ResourceKind::Workspace);
        assert_eq!(resource.name(), "default");
    }

    #[test]
    fn apply_result_created_when_missing() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workspace_manifest("fresh-ws", "workspace/fresh"))
            .expect("dispatch should succeed");
        let result = resource.apply(&mut config).expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.workspaces.contains_key("fresh-ws"));
    }

    #[test]
    fn apply_result_unchanged_for_identical_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workspace_manifest("same-ws", "workspace/same"))
            .expect("dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn apply_result_configured_when_resource_changes() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let initial = dispatch_resource(workspace_manifest("change-ws", "workspace/v1"))
            .expect("dispatch should succeed");
        assert_eq!(
            initial.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let updated = dispatch_resource(workspace_manifest("change-ws", "workspace/v2"))
            .expect("dispatch should succeed");
        assert_eq!(
            updated.apply(&mut config).expect("apply"),
            ApplyResult::Configured
        );
    }

    // ── RegisteredResource dispatch delegation ─────────────────────────

    #[test]
    fn registered_resource_kind_name_for_all_variants() {
        let ws =
            dispatch_resource(workspace_manifest("rr-ws", "workspace/rr")).expect("dispatch ws");
        assert_eq!(ws.kind(), ResourceKind::Workspace);
        assert_eq!(ws.name(), "rr-ws");

        let ag = dispatch_resource(agent_manifest("rr-ag", "cmd")).expect("dispatch agent");
        assert_eq!(ag.kind(), ResourceKind::Agent);
        assert_eq!(ag.name(), "rr-ag");

        let wf = dispatch_resource(workflow_manifest("rr-wf")).expect("dispatch workflow");
        assert_eq!(wf.kind(), ResourceKind::Workflow);
        assert_eq!(wf.name(), "rr-wf");

        let pr = dispatch_resource(project_manifest("rr-pr", "d")).expect("dispatch project");
        assert_eq!(pr.kind(), ResourceKind::Project);
        assert_eq!(pr.name(), "rr-pr");

        let df = dispatch_resource(defaults_manifest("", "", "")).expect("dispatch defaults");
        assert_eq!(df.kind(), ResourceKind::Defaults);
        assert_eq!(df.name(), "defaults");

        let rp = dispatch_resource(runtime_policy_manifest()).expect("dispatch runtime policy");
        assert_eq!(rp.kind(), ResourceKind::RuntimePolicy);
        assert_eq!(rp.name(), "runtime");
    }

    #[test]
    fn registered_resource_validate_delegates() {
        let ws = dispatch_resource(workspace_manifest("v-ws", "workspace/v"))
            .expect("dispatch validation ws");
        assert!(ws.validate().is_ok());

        let ag =
            dispatch_resource(agent_manifest("v-ag", "cmd")).expect("dispatch validation agent");
        assert!(ag.validate().is_ok());

        let wf =
            dispatch_resource(workflow_manifest("v-wf")).expect("dispatch validation workflow");
        assert!(wf.validate().is_ok());

        let pr =
            dispatch_resource(project_manifest("v-pr", "d")).expect("dispatch validation project");
        assert!(pr.validate().is_ok());

        let df =
            dispatch_resource(defaults_manifest("", "", "")).expect("dispatch validation defaults");
        assert!(df.validate().is_ok());

        let rp = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch validation runtime policy");
        assert!(rp.validate().is_ok());
    }

    #[test]
    fn registered_resource_to_yaml_delegates() {
        let ws = dispatch_resource(workspace_manifest("ty-ws", "workspace/ty"))
            .expect("dispatch yaml ws");
        assert!(ws
            .to_yaml()
            .expect("serialize workspace yaml")
            .contains("Workspace"));

        let ag = dispatch_resource(agent_manifest("ty-ag", "cmd")).expect("dispatch yaml agent");
        assert!(ag
            .to_yaml()
            .expect("serialize agent yaml")
            .contains("Agent"));

        let wf = dispatch_resource(workflow_manifest("ty-wf")).expect("dispatch yaml workflow");
        assert!(wf
            .to_yaml()
            .expect("serialize workflow yaml")
            .contains("Workflow"));

        let pr = dispatch_resource(project_manifest("ty-pr", "d")).expect("dispatch yaml project");
        assert!(pr
            .to_yaml()
            .expect("serialize project yaml")
            .contains("Project"));

        let df = dispatch_resource(defaults_manifest("", "", "")).expect("dispatch yaml defaults");
        assert!(df
            .to_yaml()
            .expect("serialize defaults yaml")
            .contains("Defaults"));

        let rp =
            dispatch_resource(runtime_policy_manifest()).expect("dispatch yaml runtime policy");
        assert!(rp
            .to_yaml()
            .expect("serialize runtime policy yaml")
            .contains("RuntimePolicy"));
    }

    #[test]
    fn registered_resource_get_from_finds_defaults_and_runtime() {
        let config = make_config();
        let defaults = RegisteredResource::get_from(&config, "defaults");
        assert!(defaults.is_some());
        assert_eq!(
            defaults.expect("defaults resource should exist").kind(),
            ResourceKind::Defaults
        );

        let runtime = RegisteredResource::get_from(&config, "runtime");
        assert!(runtime.is_some());
        assert_eq!(
            runtime.expect("runtime policy should exist").kind(),
            ResourceKind::RuntimePolicy
        );
    }

    #[test]
    fn registered_resource_get_from_returns_none_for_unknown() {
        let config = make_config();
        assert!(RegisteredResource::get_from(&config, "no-such-resource-xyz").is_none());
    }

    #[test]
    fn registered_resource_delete_from_removes_workspace() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("rd-ws", "workspace/rd"))
            .expect("dispatch delete ws");
        ws.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-ws"));
        assert!(!config.workspaces.contains_key("rd-ws"));
    }

    #[test]
    fn registered_resource_delete_from_removes_agent() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("rd-ag", "cmd")).expect("dispatch delete agent");
        ag.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-ag"));
        assert!(!config.agents.contains_key("rd-ag"));
    }

    #[test]
    fn registered_resource_delete_from_removes_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("rd-wf")).expect("dispatch delete workflow");
        wf.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-wf"));
        assert!(!config.workflows.contains_key("rd-wf"));
    }

    #[test]
    fn registered_resource_delete_from_removes_project() {
        let mut config = make_config();
        let pr =
            dispatch_resource(project_manifest("rd-pr", "d")).expect("dispatch delete project");
        pr.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-pr"));
        assert!(!config.projects.contains_key("rd-pr"));
    }

    #[test]
    fn registered_resource_delete_from_returns_false_for_unknown() {
        let mut config = make_config();
        assert!(!RegisteredResource::delete_from(
            &mut config,
            "no-such-thing"
        ));
    }

    // ── metadata helpers ────────────────────────────────────────────

    #[test]
    fn metadata_with_name_creates_minimal_metadata() {
        let meta = metadata_with_name("test");
        assert_eq!(meta.name, "test");
        assert!(meta.project.is_none());
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }

    #[test]
    fn metadata_from_parts_creates_full_metadata() {
        let labels = Some([("k".to_string(), "v".to_string())].into());
        let annotations = Some([("a".to_string(), "b".to_string())].into());
        let meta = metadata_from_parts("test", Some("proj".to_string()), labels, annotations);
        assert_eq!(meta.name, "test");
        assert_eq!(meta.project.as_deref(), Some("proj"));
        assert!(meta.labels.is_some());
        assert!(meta.annotations.is_some());
    }

    // ── validate_resource_name ──────────────────────────────────────

    #[test]
    fn validate_resource_name_accepts_valid() {
        assert!(validate_resource_name("valid-name").is_ok());
        assert!(validate_resource_name("a").is_ok());
    }

    #[test]
    fn validate_resource_name_rejects_empty() {
        assert!(validate_resource_name("").is_err());
        assert!(validate_resource_name("  ").is_err());
    }

    // ── serializes_equal ────────────────────────────────────────────

    #[test]
    fn serializes_equal_compares_by_json_value() {
        assert!(serializes_equal(&42, &42));
        assert!(!serializes_equal(&42, &43));
        assert!(serializes_equal(&"hello", &"hello"));
        assert!(!serializes_equal(&"hello", &"world"));
    }

    // ── resource_to_yaml ─────────────────────────────────────────────

    #[test]
    fn resource_to_yaml() {
        let workspace = WorkspaceResource {
            metadata: ResourceMetadata {
                name: "yaml-roundtrip".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: WorkspaceSpec {
                root_path: "workspace/yaml-roundtrip".to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
            },
        };

        let yaml = workspace
            .to_yaml()
            .expect("workspace yaml should serialize");
        assert!(yaml.contains("apiVersion: orchestrator.dev/v2"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-roundtrip"));
        assert!(yaml.contains("root_path: workspace/yaml-roundtrip"));
    }

    // ── apply_to_store ──────────────────────────────────────────────

    #[test]
    fn apply_to_store_returns_created_for_new_resource() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/new".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-new");
        let result = apply_to_store(&mut config, "Workspace", "ws-new", &meta, ws.to_cr_spec());
        assert_eq!(result, ApplyResult::Created);
        assert!(config.resource_store.get("Workspace", "ws-new").is_some());
        assert!(
            config.workspaces.contains_key("ws-new"),
            "legacy field updated"
        );
    }

    #[test]
    fn apply_to_store_returns_unchanged_for_identical() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/same".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-same");
        let spec = ws.to_cr_spec();
        apply_to_store(&mut config, "Workspace", "ws-same", &meta, spec.clone());
        let result = apply_to_store(&mut config, "Workspace", "ws-same", &meta, spec);
        assert_eq!(result, ApplyResult::Unchanged);
    }

    #[test]
    fn apply_to_store_returns_configured_for_changed() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws1 = crate::config::WorkspaceConfig {
            root_path: "/v1".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/v2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-chg");
        apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws1.to_cr_spec());
        let result = apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws2.to_cr_spec());
        assert_eq!(result, ApplyResult::Configured);
        assert_eq!(config.workspaces.get("ws-chg").unwrap().root_path, "/v2");
    }

    #[test]
    fn apply_to_store_seeds_from_legacy_for_correct_change_detection() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        // Pre-populate legacy field without going through store
        config.workspaces.insert(
            "legacy-ws".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/legacy".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        assert!(config
            .resource_store
            .get("Workspace", "legacy-ws")
            .is_none());

        // Apply the identical resource — should return Unchanged because seed detects it
        let ws = crate::config::WorkspaceConfig {
            root_path: "/legacy".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("legacy-ws");
        let result = apply_to_store(
            &mut config,
            "Workspace",
            "legacy-ws",
            &meta,
            ws.to_cr_spec(),
        );
        assert_eq!(
            result,
            ApplyResult::Unchanged,
            "should seed from legacy and detect no change"
        );
    }

    #[test]
    fn apply_to_store_increments_generation() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/g".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-gen");
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws.to_cr_spec());
        let gen1 = config
            .resource_store
            .get("Workspace", "ws-gen")
            .unwrap()
            .generation;

        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/g2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws2.to_cr_spec());
        let gen2 = config
            .resource_store
            .get("Workspace", "ws-gen")
            .unwrap()
            .generation;
        assert!(gen2 > gen1, "generation should increment on update");
    }

    // ── delete_from_store ───────────────────────────────────────────

    #[test]
    fn delete_from_store_removes_from_both_store_and_legacy() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/d".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-del");
        apply_to_store(&mut config, "Workspace", "ws-del", &meta, ws.to_cr_spec());
        assert!(config.workspaces.contains_key("ws-del"));

        let removed = delete_from_store(&mut config, "Workspace", "ws-del");
        assert!(removed);
        assert!(config.resource_store.get("Workspace", "ws-del").is_none());
        assert!(!config.workspaces.contains_key("ws-del"));
    }

    #[test]
    fn delete_from_store_seeds_from_legacy_and_removes() {
        let mut config = OrchestratorConfig::default();
        // Only in legacy, not in store
        config.workspaces.insert(
            "legacy-del".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/ld".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        let removed = delete_from_store(&mut config, "Workspace", "legacy-del");
        assert!(removed, "should seed from legacy then remove");
        assert!(!config.workspaces.contains_key("legacy-del"));
    }

    #[test]
    fn delete_from_store_returns_false_for_missing() {
        let mut config = OrchestratorConfig::default();
        let removed = delete_from_store(&mut config, "Workspace", "no-such");
        assert!(!removed);
    }

    // ── metadata_from_store ─────────────────────────────────────────

    #[test]
    fn metadata_from_store_returns_cr_metadata() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/m".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_from_parts(
            "ws-meta",
            None,
            Some([("env".to_string(), "prod".to_string())].into()),
            Some([("note".to_string(), "hi".to_string())].into()),
        );
        apply_to_store(&mut config, "Workspace", "ws-meta", &meta, ws.to_cr_spec());

        let loaded = metadata_from_store(&config, "Workspace", "ws-meta");
        assert_eq!(loaded.labels.as_ref().unwrap().get("env").unwrap(), "prod");
        assert_eq!(
            loaded.annotations.as_ref().unwrap().get("note").unwrap(),
            "hi"
        );
    }

    #[test]
    fn metadata_from_store_falls_back_to_name_only() {
        let config = OrchestratorConfig::default();
        let meta = metadata_from_store(&config, "Workspace", "missing");
        assert_eq!(meta.name, "missing");
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }
}

#[cfg(test)]
mod apply_to_project_tests {
    use super::super::test_fixtures::{
        agent_manifest, make_config, workflow_manifest, workspace_manifest,
    };
    use super::super::*;

    #[test]
    fn apply_to_project_routes_agent_to_project_scope() {
        let mut config = make_config();
        let resource =
            dispatch_resource(agent_manifest("proj-ag", "echo test")).expect("dispatch agent");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects.contains_key("my-qa"));
        assert!(config.projects["my-qa"].agents.contains_key("proj-ag"));
        // Should NOT be in global agents
        assert!(!config.agents.contains_key("proj-ag"));
    }

    #[test]
    fn apply_to_project_routes_workspace_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workspace_manifest("proj-ws", "workspace/proj"))
            .expect("dispatch ws");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workspaces.contains_key("proj-ws"));
        // Should NOT be in global workspaces
        assert!(!config.workspaces.contains_key("proj-ws"));
    }

    #[test]
    fn apply_to_project_routes_workflow_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workflow_manifest("proj-wf")).expect("dispatch wf");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workflows.contains_key("proj-wf"));
        // Should NOT be in global workflows
        assert!(!config.workflows.contains_key("proj-wf"));
    }

    #[test]
    fn apply_to_project_auto_creates_project_entry() {
        let mut config = make_config();
        assert!(!config.projects.contains_key("auto-proj"));

        let resource =
            dispatch_resource(agent_manifest("auto-ag", "echo auto")).expect("dispatch agent");
        apply_to_project(&resource, &mut config, "auto-proj").expect("apply");

        assert!(config.projects.contains_key("auto-proj"));
    }

    #[test]
    fn apply_to_project_returns_unchanged_for_identical() {
        let mut config = make_config();
        let resource =
            dispatch_resource(agent_manifest("dup-ag", "echo dup")).expect("dispatch agent");

        assert_eq!(
            apply_to_project(&resource, &mut config, "dup-proj").expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            apply_to_project(&resource, &mut config, "dup-proj").expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn apply_to_project_singleton_defaults_goes_to_global() {
        use super::super::test_fixtures::defaults_manifest;
        let mut config = make_config();
        let resource =
            dispatch_resource(defaults_manifest("p", "w", "f")).expect("dispatch defaults");
        // Singletons fall through to global apply
        let result = apply_to_project(&resource, &mut config, "proj-singleton").expect("apply");
        assert!(matches!(
            result,
            ApplyResult::Created | ApplyResult::Configured | ApplyResult::Unchanged
        ));
        // Defaults were applied to global config (project field updated)
        assert_eq!(config.defaults.project, "p");
    }
}
