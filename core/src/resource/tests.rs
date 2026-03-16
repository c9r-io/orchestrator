#[cfg(test)]
mod cases {
    use super::super::helpers::metadata_from_parts;
    use super::super::*;
    use crate::cli_types::{
        AgentSpec, OrchestratorResource, ResourceMetadata, ResourceSpec, WorkspaceSpec,
    };
    use crate::config::OrchestratorConfig;
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    use super::super::test_fixtures::{
        agent_manifest, make_config, project_manifest, runtime_policy_manifest, workflow_manifest,
        workspace_manifest,
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
                enabled: None,
                command: "echo {prompt}".to_string(),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
            })),
        };

        let error = dispatch_resource(resource).expect_err("dispatch should fail");
        assert!(error.to_string().contains("mismatch"));
    }

    #[test]
    fn resource_registry_includes_execution_profile() {
        let registry = resource_registry();
        assert_eq!(registry.len(), 10);
        let kinds: Vec<ResourceKind> = registry.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::ExecutionProfile));
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
        assert!(config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("fresh-ws"));
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

        let rp =
            dispatch_resource(runtime_policy_manifest()).expect("dispatch yaml runtime policy");
        assert!(rp
            .to_yaml()
            .expect("serialize runtime policy yaml")
            .contains("RuntimePolicy"));
    }

    #[test]
    fn registered_resource_get_from_finds_runtime() {
        let config = make_config();
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
        assert!(!config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("rd-ws"));
    }

    #[test]
    fn registered_resource_delete_from_removes_agent() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("rd-ag", "cmd")).expect("dispatch delete agent");
        ag.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-ag"));
        assert!(!config
            .default_project()
            .expect("default project")
            .agents
            .contains_key("rd-ag"));
    }

    #[test]
    fn registered_resource_delete_from_removes_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("rd-wf")).expect("dispatch delete workflow");
        wf.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-wf"));
        assert!(!config
            .default_project()
            .expect("default project")
            .workflows
            .contains_key("rd-wf"));
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
                health_policy: None,
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
            health_policy: Default::default(),
        };
        let meta = metadata_with_name("ws-new");
        let result = apply_to_store(&mut config, "Workspace", "ws-new", &meta, ws.to_cr_spec());
        assert_eq!(result, ApplyResult::Created);
        assert!(config
            .resource_store
            .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "ws-new")
            .is_some());
        assert!(config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("ws-new"));
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
            health_policy: Default::default(),
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
            health_policy: Default::default(),
        };
        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/v2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
            health_policy: Default::default(),
        };
        let meta = metadata_with_name("ws-chg");
        apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws1.to_cr_spec());
        let result = apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws2.to_cr_spec());
        assert_eq!(result, ApplyResult::Configured);
        assert_eq!(
            config
                .default_project()
                .expect("default project")
                .workspaces
                .get("ws-chg")
                .unwrap()
                .root_path,
            "/v2"
        );
    }

    #[test]
    fn apply_to_store_seeds_from_config_snapshot_for_correct_change_detection() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        // Pre-populate the config snapshot without going through the resource store.
        config.ensure_project(None).workspaces.insert(
            "snapshot-ws".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/snapshot".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
            },
        );
        assert!(config
            .resource_store
            .get_namespaced(
                "Workspace",
                crate::config::DEFAULT_PROJECT_ID,
                "snapshot-ws",
            )
            .is_none());

        // Apply the identical resource — should return Unchanged because reconciliation detects it.
        let ws = crate::config::WorkspaceConfig {
            root_path: "/snapshot".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
            health_policy: Default::default(),
        };
        let meta = metadata_with_name("snapshot-ws");
        let result = apply_to_store(
            &mut config,
            "Workspace",
            "snapshot-ws",
            &meta,
            ws.to_cr_spec(),
        );
        assert_eq!(
            result,
            ApplyResult::Unchanged,
            "should seed from the config snapshot and detect no change"
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
            health_policy: Default::default(),
        };
        let meta = metadata_with_name("ws-gen");
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws.to_cr_spec());
        let gen1 = config
            .resource_store
            .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "ws-gen")
            .unwrap()
            .generation;

        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/g2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
            health_policy: Default::default(),
        };
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws2.to_cr_spec());
        let gen2 = config
            .resource_store
            .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "ws-gen")
            .unwrap()
            .generation;
        assert!(gen2 > gen1, "generation should increment on update");
    }

    // ── delete_from_store ───────────────────────────────────────────

    #[test]
    fn delete_from_store_removes_from_store_and_config_snapshot() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/d".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
            health_policy: Default::default(),
        };
        let meta = metadata_with_name("ws-del");
        apply_to_store(&mut config, "Workspace", "ws-del", &meta, ws.to_cr_spec());
        assert!(config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("ws-del"));

        let removed = delete_from_store(&mut config, "Workspace", "ws-del");
        assert!(removed);
        assert!(config
            .resource_store
            .get_namespaced("Workspace", crate::config::DEFAULT_PROJECT_ID, "ws-del")
            .is_none());
        assert!(!config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("ws-del"));
    }

    #[test]
    fn delete_from_store_seeds_from_config_snapshot_and_removes() {
        let mut config = OrchestratorConfig::default();
        // Only in the config snapshot, not in the resource store.
        config.ensure_project(None).workspaces.insert(
            "snapshot-del".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/ld".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: Default::default(),
            },
        );
        let removed = delete_from_store(&mut config, "Workspace", "snapshot-del");
        assert!(removed, "should seed from the config snapshot then remove");
        assert!(!config
            .default_project()
            .expect("default project")
            .workspaces
            .contains_key("snapshot-del"));
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
            health_policy: Default::default(),
        };
        let meta = metadata_from_parts(
            "ws-meta",
            None,
            Some([("env".to_string(), "prod".to_string())].into()),
            Some([("note".to_string(), "hi".to_string())].into()),
        );
        apply_to_store(&mut config, "Workspace", "ws-meta", &meta, ws.to_cr_spec());

        let loaded = metadata_from_store(&config, "Workspace", "ws-meta", None);
        assert_eq!(loaded.labels.as_ref().unwrap().get("env").unwrap(), "prod");
        assert_eq!(
            loaded.annotations.as_ref().unwrap().get("note").unwrap(),
            "hi"
        );
    }

    #[test]
    fn metadata_from_store_falls_back_to_name_only() {
        let config = OrchestratorConfig::default();
        let meta = metadata_from_store(&config, "Workspace", "missing", None);
        assert_eq!(meta.name, "missing");
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }
}

#[cfg(test)]
mod apply_to_project_tests {
    use super::super::test_fixtures::{
        agent_manifest, make_config, runtime_policy_manifest, workflow_manifest, workspace_manifest,
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
    }

    #[test]
    fn apply_to_project_routes_workspace_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workspace_manifest("proj-ws", "workspace/proj"))
            .expect("dispatch ws");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workspaces.contains_key("proj-ws"));
    }

    #[test]
    fn apply_to_project_routes_workflow_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workflow_manifest("proj-wf")).expect("dispatch wf");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workflows.contains_key("proj-wf"));
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
    fn apply_to_project_routes_runtime_policy_through_generic_path() {
        let mut config = make_config();
        let resource = dispatch_resource(runtime_policy_manifest()).expect("dispatch runtime");

        // RuntimePolicy is project-scoped: applying to a new project creates a
        // project-specific override (Created), not updating the _system default.
        let result = apply_to_project(&resource, &mut config, "my-project").expect("apply");
        assert_eq!(result, ApplyResult::Created);

        // Applying again to the same project updates it (Configured or Unchanged).
        let result2 = apply_to_project(&resource, &mut config, "my-project").expect("apply again");
        assert_eq!(result2, ApplyResult::Unchanged);
    }
}

// ── execution_profile tests ─────────────────────────────────────────
#[cfg(test)]
mod execution_profile_tests {
    use super::super::test_fixtures::{
        env_store_manifest, execution_profile_manifest, make_config, secret_store_manifest,
        step_template_manifest,
    };
    use super::super::*;
    use crate::cli_types::{
        ExecutionProfileSpec, OrchestratorResource, ResourceKind, ResourceMetadata, ResourceSpec,
    };
    use crate::config::{
        ExecutionFsMode, ExecutionNetworkMode, ExecutionProfileConfig, ExecutionProfileMode,
    };
    use crate::resource::execution_profile::{
        build_execution_profile, execution_profile_config_to_spec, execution_profile_spec_to_config,
    };

    // ── spec_to_config conversion ───────────────────────────────

    #[test]
    fn spec_to_config_sandbox_mode() {
        let spec = ExecutionProfileSpec {
            mode: "sandbox".to_string(),
            fs_mode: "workspace_readonly".to_string(),
            writable_paths: vec!["/tmp".to_string()],
            network_mode: "deny".to_string(),
            network_allowlist: vec![],
            max_memory_mb: Some(512),
            max_cpu_seconds: Some(60),
            max_processes: Some(10),
            max_open_files: Some(256),
        };
        let config = execution_profile_spec_to_config(&spec);
        assert_eq!(config.mode, ExecutionProfileMode::Sandbox);
        assert_eq!(config.fs_mode, ExecutionFsMode::WorkspaceReadonly);
        assert_eq!(config.writable_paths, vec!["/tmp".to_string()]);
        assert_eq!(config.network_mode, ExecutionNetworkMode::Deny);
        assert_eq!(config.max_memory_mb, Some(512));
        assert_eq!(config.max_cpu_seconds, Some(60));
        assert_eq!(config.max_processes, Some(10));
        assert_eq!(config.max_open_files, Some(256));
    }

    #[test]
    fn spec_to_config_host_mode_defaults() {
        let spec = ExecutionProfileSpec {
            mode: "host".to_string(),
            fs_mode: "inherit".to_string(),
            writable_paths: vec![],
            network_mode: "inherit".to_string(),
            network_allowlist: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        };
        let config = execution_profile_spec_to_config(&spec);
        assert_eq!(config.mode, ExecutionProfileMode::Host);
        assert_eq!(config.fs_mode, ExecutionFsMode::Inherit);
        assert_eq!(config.network_mode, ExecutionNetworkMode::Inherit);
    }

    #[test]
    fn spec_to_config_workspace_rw_scoped() {
        let spec = ExecutionProfileSpec {
            mode: "sandbox".to_string(),
            fs_mode: "workspace_rw_scoped".to_string(),
            writable_paths: vec![],
            network_mode: "allowlist".to_string(),
            network_allowlist: vec!["example.com".to_string()],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        };
        let config = execution_profile_spec_to_config(&spec);
        assert_eq!(config.fs_mode, ExecutionFsMode::WorkspaceRwScoped);
        assert_eq!(config.network_mode, ExecutionNetworkMode::Allowlist);
        assert_eq!(config.network_allowlist, vec!["example.com".to_string()]);
    }

    #[test]
    fn spec_to_config_unknown_mode_defaults_to_host() {
        let spec = ExecutionProfileSpec {
            mode: "unknown_mode".to_string(),
            fs_mode: "unknown_fs".to_string(),
            writable_paths: vec![],
            network_mode: "unknown_net".to_string(),
            network_allowlist: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        };
        let config = execution_profile_spec_to_config(&spec);
        assert_eq!(config.mode, ExecutionProfileMode::Host);
        assert_eq!(config.fs_mode, ExecutionFsMode::Inherit);
        assert_eq!(config.network_mode, ExecutionNetworkMode::Inherit);
    }

    // ── config_to_spec conversion ───────────────────────────────

    #[test]
    fn config_to_spec_sandbox_mode() {
        let config = ExecutionProfileConfig {
            mode: ExecutionProfileMode::Sandbox,
            fs_mode: ExecutionFsMode::WorkspaceReadonly,
            writable_paths: vec!["/out".to_string()],
            network_mode: ExecutionNetworkMode::Allowlist,
            network_allowlist: vec!["api.example.com".to_string()],
            max_memory_mb: Some(1024),
            max_cpu_seconds: Some(120),
            max_processes: Some(50),
            max_open_files: Some(512),
        };
        let spec = execution_profile_config_to_spec(&config);
        assert_eq!(spec.mode, "sandbox");
        assert_eq!(spec.fs_mode, "workspace_readonly");
        assert_eq!(spec.writable_paths, vec!["/out".to_string()]);
        assert_eq!(spec.network_mode, "allowlist");
        assert_eq!(spec.network_allowlist, vec!["api.example.com".to_string()]);
        assert_eq!(spec.max_memory_mb, Some(1024));
        assert_eq!(spec.max_cpu_seconds, Some(120));
        assert_eq!(spec.max_processes, Some(50));
        assert_eq!(spec.max_open_files, Some(512));
    }

    #[test]
    fn config_to_spec_host_mode() {
        let config = ExecutionProfileConfig {
            mode: ExecutionProfileMode::Host,
            fs_mode: ExecutionFsMode::Inherit,
            writable_paths: vec![],
            network_mode: ExecutionNetworkMode::Inherit,
            network_allowlist: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        };
        let spec = execution_profile_config_to_spec(&config);
        assert_eq!(spec.mode, "host");
        assert_eq!(spec.fs_mode, "inherit");
        assert_eq!(spec.network_mode, "inherit");
    }

    #[test]
    fn config_to_spec_workspace_rw_scoped_and_deny() {
        let config = ExecutionProfileConfig {
            mode: ExecutionProfileMode::Sandbox,
            fs_mode: ExecutionFsMode::WorkspaceRwScoped,
            writable_paths: vec![],
            network_mode: ExecutionNetworkMode::Deny,
            network_allowlist: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_processes: None,
            max_open_files: None,
        };
        let spec = execution_profile_config_to_spec(&config);
        assert_eq!(spec.fs_mode, "workspace_rw_scoped");
        assert_eq!(spec.network_mode, "deny");
    }

    // ── roundtrip ───────────────────────────────────────────────

    #[test]
    fn spec_config_roundtrip_identity() {
        let original_spec = ExecutionProfileSpec {
            mode: "sandbox".to_string(),
            fs_mode: "workspace_rw_scoped".to_string(),
            writable_paths: vec!["/a".to_string(), "/b".to_string()],
            network_mode: "allowlist".to_string(),
            network_allowlist: vec!["x.com".to_string()],
            max_memory_mb: Some(256),
            max_cpu_seconds: Some(30),
            max_processes: Some(5),
            max_open_files: Some(100),
        };
        let config = execution_profile_spec_to_config(&original_spec);
        let roundtripped = execution_profile_config_to_spec(&config);
        assert_eq!(original_spec, roundtripped);
    }

    // ── validate ────────────────────────────────────────────────

    #[test]
    fn validate_host_with_non_inherit_fs_mode_fails() {
        let resource = dispatch_resource(execution_profile_manifest(
            "bad-host",
            "host",
            "workspace_readonly",
        ))
        .expect("dispatch");
        let err = resource.validate().expect_err("should fail validation");
        assert!(err.to_string().contains("fs_mode"));
    }

    #[test]
    fn validate_sandbox_with_any_fs_mode_ok() {
        let resource = dispatch_resource(execution_profile_manifest(
            "ok-sandbox",
            "sandbox",
            "workspace_readonly",
        ))
        .expect("dispatch");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn validate_host_with_inherit_fs_mode_ok() {
        let resource = dispatch_resource(execution_profile_manifest("ok-host", "host", "inherit"))
            .expect("dispatch");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn validate_allowlist_with_empty_network_allowlist_fails() {
        let manifest = OrchestratorResource {
            api_version: super::super::API_VERSION.to_string(),
            kind: ResourceKind::ExecutionProfile,
            metadata: ResourceMetadata {
                name: "bad-allowlist".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::ExecutionProfile(ExecutionProfileSpec {
                mode: "sandbox".to_string(),
                fs_mode: "inherit".to_string(),
                writable_paths: vec![],
                network_mode: "allowlist".to_string(),
                network_allowlist: vec![], // empty!
                max_memory_mb: None,
                max_cpu_seconds: None,
                max_processes: None,
                max_open_files: None,
            }),
        };
        let resource = dispatch_resource(manifest).expect("dispatch");
        let err = resource.validate().expect_err("should fail");
        assert!(err.to_string().contains("network_allowlist"));
    }

    #[test]
    fn validate_allowlist_with_entries_ok() {
        let manifest = OrchestratorResource {
            api_version: super::super::API_VERSION.to_string(),
            kind: ResourceKind::ExecutionProfile,
            metadata: ResourceMetadata {
                name: "ok-allowlist".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::ExecutionProfile(ExecutionProfileSpec {
                mode: "sandbox".to_string(),
                fs_mode: "inherit".to_string(),
                writable_paths: vec![],
                network_mode: "allowlist".to_string(),
                network_allowlist: vec!["example.com".to_string()],
                max_memory_mb: None,
                max_cpu_seconds: None,
                max_processes: None,
                max_open_files: None,
            }),
        };
        let resource = dispatch_resource(manifest).expect("dispatch");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn validate_allowlist_with_wildcard_entry_fails() {
        let manifest = OrchestratorResource {
            api_version: super::super::API_VERSION.to_string(),
            kind: ResourceKind::ExecutionProfile,
            metadata: ResourceMetadata {
                name: "bad-allowlist-wildcard".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::ExecutionProfile(ExecutionProfileSpec {
                mode: "sandbox".to_string(),
                fs_mode: "inherit".to_string(),
                writable_paths: vec![],
                network_mode: "allowlist".to_string(),
                network_allowlist: vec!["*.example.com".to_string()],
                max_memory_mb: None,
                max_cpu_seconds: None,
                max_processes: None,
                max_open_files: None,
            }),
        };
        let resource = dispatch_resource(manifest).expect("dispatch");
        let err = resource.validate().expect_err("wildcard should fail");
        assert!(err.to_string().contains("wildcards"));
    }

    // ── build_execution_profile ─────────────────────────────────

    #[test]
    fn build_rejects_wrong_kind() {
        use crate::cli_types::WorkspaceSpec;
        let manifest = OrchestratorResource {
            api_version: super::super::API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "wrong".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/tmp".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: None,
            }),
        };
        assert!(build_execution_profile(manifest).is_err());
    }

    #[test]
    fn build_rejects_mismatched_spec() {
        use crate::cli_types::WorkspaceSpec;
        let manifest = OrchestratorResource {
            api_version: super::super::API_VERSION.to_string(),
            kind: ResourceKind::ExecutionProfile,
            metadata: ResourceMetadata {
                name: "mismatch".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/tmp".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
                health_policy: None,
            }),
        };
        assert!(build_execution_profile(manifest).is_err());
    }

    #[test]
    fn build_succeeds_with_correct_kind_and_spec() {
        let manifest = execution_profile_manifest("good", "sandbox", "inherit");
        let result = build_execution_profile(manifest).expect("should succeed");
        assert_eq!(result.kind(), ResourceKind::ExecutionProfile);
        assert_eq!(result.name(), "good");
    }

    // ── dispatch / kind / name / to_yaml for ExecutionProfile ───

    #[test]
    fn dispatch_maps_execution_profile_manifest() {
        let resource = dispatch_resource(execution_profile_manifest("ep-test", "host", "inherit"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::ExecutionProfile);
        assert_eq!(resource.name(), "ep-test");
    }

    #[test]
    fn execution_profile_to_yaml_contains_kind() {
        let resource = dispatch_resource(execution_profile_manifest(
            "ep-yaml",
            "sandbox",
            "workspace_readonly",
        ))
        .expect("dispatch");
        let yaml = resource.to_yaml().expect("yaml");
        assert!(yaml.contains("ExecutionProfile"));
        assert!(yaml.contains("ep-yaml"));
    }

    // ── apply / get_from / delete_from for ExecutionProfile ─────

    #[test]
    fn apply_execution_profile_creates_and_retrieves() {
        let mut config = make_config();
        let resource =
            dispatch_resource(execution_profile_manifest("apply-ep", "sandbox", "inherit"))
                .expect("dispatch");
        let result = resource.apply(&mut config).expect("apply");
        assert_eq!(result, ApplyResult::Created);

        let found = RegisteredResource::get_from(&config, "apply-ep");
        assert!(found.is_some());
        assert_eq!(found.unwrap().kind(), ResourceKind::ExecutionProfile);
    }

    #[test]
    fn delete_execution_profile_removes() {
        let mut config = make_config();
        let resource = dispatch_resource(execution_profile_manifest("del-ep", "host", "inherit"))
            .expect("dispatch");
        resource.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "del-ep"));
        assert!(RegisteredResource::get_from(&config, "del-ep").is_none());
    }

    // ── StepTemplate variant coverage ───────────────────────────

    #[test]
    fn step_template_kind_name_validate_yaml() {
        let resource = dispatch_resource(step_template_manifest("tmpl-1", "Run QA"))
            .expect("dispatch step template");
        assert_eq!(resource.kind(), ResourceKind::StepTemplate);
        assert_eq!(resource.name(), "tmpl-1");
        assert!(resource.validate().is_ok());
        let yaml = resource.to_yaml().expect("yaml");
        assert!(yaml.contains("StepTemplate"));
        assert!(yaml.contains("tmpl-1"));
    }

    #[test]
    fn step_template_apply_and_delete() {
        let mut config = make_config();
        let resource =
            dispatch_resource(step_template_manifest("tmpl-ad", "prompt")).expect("dispatch");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert!(RegisteredResource::get_from(&config, "tmpl-ad").is_some());
        assert!(RegisteredResource::delete_from(&mut config, "tmpl-ad"));
        assert!(RegisteredResource::get_from(&config, "tmpl-ad").is_none());
    }

    // ── EnvStore variant coverage ───────────────────────────────

    #[test]
    fn env_store_kind_name_validate_yaml() {
        let resource = dispatch_resource(env_store_manifest("env-1")).expect("dispatch env store");
        assert_eq!(resource.kind(), ResourceKind::EnvStore);
        assert_eq!(resource.name(), "env-1");
        assert!(resource.validate().is_ok());
        let yaml = resource.to_yaml().expect("yaml");
        assert!(yaml.contains("EnvStore"));
    }

    #[test]
    fn env_store_apply_and_delete() {
        let mut config = make_config();
        let resource = dispatch_resource(env_store_manifest("env-ad")).expect("dispatch");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert!(RegisteredResource::get_from(&config, "env-ad").is_some());
        assert!(RegisteredResource::delete_from(&mut config, "env-ad"));
        assert!(RegisteredResource::get_from(&config, "env-ad").is_none());
    }

    // ── SecretStore variant coverage ────────────────────────────

    #[test]
    fn secret_store_kind_name_validate_yaml() {
        let resource =
            dispatch_resource(secret_store_manifest("sec-1")).expect("dispatch secret store");
        assert_eq!(resource.kind(), ResourceKind::SecretStore);
        assert_eq!(resource.name(), "sec-1");
        assert!(resource.validate().is_ok());
        let yaml = resource.to_yaml().expect("yaml");
        assert!(yaml.contains("SecretStore"));
    }

    #[test]
    fn secret_store_apply_and_delete() {
        let mut config = make_config();
        let resource = dispatch_resource(secret_store_manifest("sec-ad")).expect("dispatch");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert!(RegisteredResource::get_from(&config, "sec-ad").is_some());
        assert!(RegisteredResource::delete_from(&mut config, "sec-ad"));
        assert!(RegisteredResource::get_from(&config, "sec-ad").is_none());
    }
}
