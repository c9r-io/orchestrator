use agent_orchestrator::cli_types::ResourceKind;
use agent_orchestrator::resource::{
    delete_resource_by_kind, dispatch_resource, kind_as_str, parse_resources_from_yaml,
    ApplyResult, Resource,
};

fn minimal_config() -> agent_orchestrator::config::OrchestratorConfig {
    use agent_orchestrator::config::*;
    use std::collections::HashMap;

    OrchestratorConfig {
        runner: RunnerConfig {
            shell: "/bin/bash".to_string(),
            shell_arg: "-lc".to_string(),
            ..RunnerConfig::default()
        },
        resume: ResumeConfig { auto: false },
        defaults: ConfigDefaults {
            project: String::new(),
            workspace: "default".to_string(),
            workflow: "basic".to_string(),
        },
        projects: HashMap::new(),
        workspaces: {
            let mut ws = HashMap::new();
            ws.insert(
                "default".to_string(),
                WorkspaceConfig {
                    root_path: "workspace/default".to_string(),
                    qa_targets: vec!["docs/qa".to_string()],
                    ticket_dir: "docs/ticket".to_string(),
                    self_referential: false,
                },
            );
            ws
        },
        agents: {
            let mut agents = HashMap::new();
            agents.insert(
                "echo".to_string(),
                AgentConfig {
                    metadata: AgentMetadata::default(),
                    capabilities: vec!["qa".to_string()],
                    templates: {
                        let mut t = HashMap::new();
                        t.insert("qa".to_string(), "echo qa".to_string());
                        t
                    },
                    selection: AgentSelectionConfig::default(),
                },
            );
            agents
        },
        workflows: {
            let mut workflows = HashMap::new();
            workflows.insert(
                "basic".to_string(),
                WorkflowConfig {
                    steps: vec![WorkflowStepConfig {
                        id: "run_qa".to_string(),
                        description: None,
                        step_type: Some(WorkflowStepType::Qa),
                        builtin: None,
                        required_capability: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: Vec::new(),
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                    }],
                    loop_policy: WorkflowLoopConfig {
                        mode: LoopMode::Once,
                        guard: WorkflowLoopGuardConfig::default(),
                    },
                    finalize: WorkflowFinalizeConfig { rules: vec![] },
                    qa: None,
                    fix: None,
                    retest: None,
                    dynamic_steps: vec![],
                    safety: agent_orchestrator::config::SafetyConfig::default(),
                },
            );
            workflows
        },
        resource_meta: ResourceMetadataStore::default(),
    }
}

fn workspace_yaml(name: &str, root_path: &str) -> String {
    format!(
        r#"apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: {name}
spec:
  root_path: {root_path}
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#
    )
}

fn agent_yaml(name: &str, qa_template: &str) -> String {
    format!(
        r#"apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: {name}
spec:
  templates:
    qa: "{qa_template}"
"#
    )
}

#[test]
fn apply_creates_new_workspace_in_config() {
    let mut config = minimal_config();
    let yaml = workspace_yaml("new-ws", "workspace/new-ws");
    let resources = parse_resources_from_yaml(&yaml).expect("should parse");
    assert_eq!(resources.len(), 1);

    let registered = dispatch_resource(resources.into_iter().next().unwrap()).expect("dispatch");
    assert_eq!(registered.kind(), ResourceKind::Workspace);
    assert_eq!(registered.name(), "new-ws");
    registered.validate().expect("should be valid");

    let result = registered.apply(&mut config);
    assert_eq!(result, ApplyResult::Created);
    assert!(config.workspaces.contains_key("new-ws"));
    assert_eq!(config.workspaces["new-ws"].root_path, "workspace/new-ws");
}

#[test]
fn apply_updates_existing_workspace() {
    let mut config = minimal_config();

    let v1 = parse_resources_from_yaml(&workspace_yaml("default", "workspace/v1")).unwrap();
    let r1 = dispatch_resource(v1.into_iter().next().unwrap()).unwrap();
    let result = r1.apply(&mut config);
    assert_eq!(result, ApplyResult::Configured);
    assert_eq!(config.workspaces["default"].root_path, "workspace/v1");

    let v2 = parse_resources_from_yaml(&workspace_yaml("default", "workspace/v2")).unwrap();
    let r2 = dispatch_resource(v2.into_iter().next().unwrap()).unwrap();
    let result = r2.apply(&mut config);
    assert_eq!(result, ApplyResult::Configured);
    assert_eq!(config.workspaces["default"].root_path, "workspace/v2");
}

#[test]
fn apply_returns_unchanged_for_identical_resource() {
    let mut config = minimal_config();
    let yaml = workspace_yaml("default", "workspace/default");

    let resources = parse_resources_from_yaml(&yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    let result = registered.apply(&mut config);
    assert_eq!(result, ApplyResult::Unchanged);
}

#[test]
fn apply_preserves_unmentioned_resources() {
    let mut config = minimal_config();

    let yaml = workspace_yaml("new-ws", "workspace/new-ws");
    let resources = parse_resources_from_yaml(&yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    registered.apply(&mut config);

    assert!(config.workspaces.contains_key("default"));
    assert!(config.workspaces.contains_key("new-ws"));
}

#[test]
fn multi_document_apply_parses_all_resources() {
    let yaml = format!(
        "{}---\n{}---\n{}",
        workspace_yaml("ws-a", "workspace/ws-a"),
        workspace_yaml("ws-b", "workspace/ws-b"),
        agent_yaml("test-agent", "echo test")
    );

    let resources = parse_resources_from_yaml(&yaml).expect("should parse multi-doc");
    assert_eq!(resources.len(), 3);
    assert_eq!(resources[0].kind, ResourceKind::Workspace);
    assert_eq!(resources[0].metadata.name, "ws-a");
    assert_eq!(resources[1].kind, ResourceKind::Workspace);
    assert_eq!(resources[1].metadata.name, "ws-b");
    assert_eq!(resources[2].kind, ResourceKind::Agent);
    assert_eq!(resources[2].metadata.name, "test-agent");
}

#[test]
fn multi_document_apply_all_to_config() {
    let mut config = minimal_config();
    let yaml = format!(
        "{}---\n{}",
        workspace_yaml("ws-extra", "workspace/ws-extra"),
        agent_yaml("agent-extra", "echo extra")
    );

    let resources = parse_resources_from_yaml(&yaml).unwrap();
    for resource in resources {
        let registered = dispatch_resource(resource).unwrap();
        registered.validate().unwrap();
        registered.apply(&mut config);
    }

    assert!(config.workspaces.contains_key("ws-extra"));
    assert!(config.agents.contains_key("agent-extra"));
}

#[test]
fn validation_rejects_empty_workspace_root_path() {
    let yaml = r#"apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: bad-ws
spec:
  root_path: "  "
  qa_targets: []
  ticket_dir: docs/ticket
"#;
    let resources = parse_resources_from_yaml(yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    let result = registered.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("root_path"));
}

#[test]
fn validation_rejects_empty_agent_templates() {
    let yaml = r#"apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: empty-agent
spec:
  templates: {}
"#;
    let resources = parse_resources_from_yaml(yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    let result = registered.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("at least one template"));
}

#[test]
fn validation_rejects_invalid_api_version() {
    let yaml = r#"apiVersion: wrong/v2
kind: Workspace
metadata:
  name: invalid
spec:
  root_path: /tmp
  qa_targets: []
  ticket_dir: docs/ticket
"#;
    let resources = parse_resources_from_yaml(yaml).unwrap();
    let resource = &resources[0];
    let result = resource.validate_version();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("wrong/v2"));
}

#[test]
fn delete_removes_workspace_from_config() {
    let mut config = minimal_config();
    config.workspaces.insert(
        "to-delete".to_string(),
        agent_orchestrator::config::WorkspaceConfig {
            root_path: "workspace/to-delete".to_string(),
            qa_targets: vec!["docs/qa".to_string()],
            ticket_dir: "docs/ticket".to_string(),
            self_referential: false,
        },
    );

    let deleted =
        delete_resource_by_kind(&mut config, "workspace", "to-delete").expect("should succeed");
    assert!(deleted);
    assert!(!config.workspaces.contains_key("to-delete"));
    assert!(config.workspaces.contains_key("default"));
}

#[test]
fn delete_returns_false_for_missing_resource() {
    let mut config = minimal_config();
    let deleted =
        delete_resource_by_kind(&mut config, "workspace", "nonexistent").expect("should succeed");
    assert!(!deleted);
}

#[test]
fn delete_rejects_unknown_resource_type() {
    let mut config = minimal_config();
    let result = delete_resource_by_kind(&mut config, "unknown", "foo");
    assert!(result.is_err());
}

#[test]
fn kind_as_str_covers_all_resource_kinds() {
    assert_eq!(kind_as_str(ResourceKind::Workspace), "workspace");
    assert_eq!(kind_as_str(ResourceKind::Agent), "agent");
    assert_eq!(kind_as_str(ResourceKind::Workflow), "workflow");
}

#[test]
fn apply_then_delete_roundtrip() {
    let mut config = minimal_config();

    let yaml = agent_yaml("temp-agent", "echo temp");
    let resources = parse_resources_from_yaml(&yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    assert_eq!(registered.apply(&mut config), ApplyResult::Created);
    assert!(config.agents.contains_key("temp-agent"));

    let deleted = delete_resource_by_kind(&mut config, "agent", "temp-agent").unwrap();
    assert!(deleted);
    assert!(!config.agents.contains_key("temp-agent"));
}

#[test]
fn resource_to_yaml_roundtrip() {
    let mut config = minimal_config();
    let yaml = workspace_yaml("roundtrip-ws", "workspace/roundtrip");
    let resources = parse_resources_from_yaml(&yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    registered.apply(&mut config);

    let exported = registered.to_yaml().expect("should serialize to yaml");
    assert!(exported.contains("apiVersion: orchestrator.dev/v2"));
    assert!(exported.contains("kind: Workspace"));
    assert!(exported.contains("name: roundtrip-ws"));
    assert!(exported.contains("workspace/roundtrip"));

    let re_parsed = parse_resources_from_yaml(&exported).unwrap();
    assert_eq!(re_parsed.len(), 1);
    assert_eq!(re_parsed[0].metadata.name, "roundtrip-ws");
}

#[test]
fn apply_persists_labels_and_annotations_for_selector_usage() {
    let mut config = minimal_config();
    let yaml = r#"apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: labeled-ws
  labels:
    env: test
  annotations:
    owner: platform
spec:
  root_path: workspace/labeled
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#;

    let resources = parse_resources_from_yaml(yaml).unwrap();
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    assert_eq!(registered.apply(&mut config), ApplyResult::Created);

    let stored = config
        .resource_meta
        .workspaces
        .get("labeled-ws")
        .expect("metadata should be stored");
    assert_eq!(
        stored.labels.as_ref().and_then(|m| m.get("env")),
        Some(&"test".to_string())
    );
    assert_eq!(
        stored.annotations.as_ref().and_then(|m| m.get("owner")),
        Some(&"platform".to_string())
    );
}

// ── Bootstrap pipeline tests ──────────────────────────────────────────────────

#[test]
fn sdlc_step_types_round_trip() {
    use agent_orchestrator::config::WorkflowStepType;
    use std::str::FromStr;

    let sdlc_types = [
        ("qa_doc_gen", WorkflowStepType::QaDocGen),
        ("qa_testing", WorkflowStepType::QaTesting),
        ("ticket_fix", WorkflowStepType::TicketFix),
        ("doc_governance", WorkflowStepType::DocGovernance),
        ("align_tests", WorkflowStepType::AlignTests),
        ("plan", WorkflowStepType::Plan),
        ("implement", WorkflowStepType::Implement),
    ];

    for (s, expected_variant) in &sdlc_types {
        let parsed =
            WorkflowStepType::from_str(s).unwrap_or_else(|e| panic!("from_str({s}) failed: {e}"));
        assert_eq!(parsed, *expected_variant, "variant mismatch for '{s}'");
        assert_eq!(parsed.as_str(), *s, "as_str round-trip failed for '{s}'");
    }
}

#[test]
fn parse_self_bootstrap_fixture_resources() {
    let yaml = std::fs::read_to_string("../fixtures/manifests/bundles/self-bootstrap-test.yaml")
        .expect("fixture file missing");
    let resources = parse_resources_from_yaml(&yaml).expect("should parse");

    let workspace_count = resources
        .iter()
        .filter(|r| r.kind == ResourceKind::Workspace)
        .count();
    let agent_count = resources
        .iter()
        .filter(|r| r.kind == ResourceKind::Agent)
        .count();
    let workflow_count = resources
        .iter()
        .filter(|r| r.kind == ResourceKind::Workflow)
        .count();

    assert_eq!(workspace_count, 1, "expected 1 workspace");
    assert!(
        agent_count >= 6,
        "expected at least 6 agents, got {agent_count}"
    );
    assert!(
        workflow_count >= 5,
        "expected at least 5 workflows, got {workflow_count}"
    );

    let ws = resources
        .iter()
        .find(|r| r.kind == ResourceKind::Workspace)
        .expect("workspace missing");
    assert_eq!(ws.metadata.name, "bootstrap-ws");
}

#[test]
fn workspace_self_referential_parses() {
    let yaml = r#"apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: self-ws
spec:
  root_path: workspace/self
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
  self_referential: true
"#;
    let resources = parse_resources_from_yaml(yaml).expect("should parse");
    let registered = dispatch_resource(resources.into_iter().next().unwrap()).unwrap();
    let mut config = minimal_config();
    registered.apply(&mut config);

    let ws = config.workspaces.get("self-ws").expect("workspace missing");
    assert!(ws.self_referential, "self_referential should be true");
}

fn multi_agent_config() -> agent_orchestrator::config::OrchestratorConfig {
    use agent_orchestrator::config::*;
    use std::collections::HashMap;

    fn make_agent(capabilities: &[&str], templates: &[(&str, &str)]) -> AgentConfig {
        AgentConfig {
            metadata: AgentMetadata::default(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            templates: templates
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            selection: AgentSelectionConfig::default(),
        }
    }

    let mut agents = HashMap::new();
    agents.insert(
        "architect".to_string(),
        make_agent(
            &["plan", "qa_doc_gen"],
            &[("plan", "echo plan"), ("qa_doc_gen", "echo qa_doc_gen")],
        ),
    );
    agents.insert(
        "coder".to_string(),
        make_agent(
            &["implement", "ticket_fix", "align_tests"],
            &[
                ("implement", "echo implement"),
                ("ticket_fix", "echo ticket_fix"),
                ("align_tests", "echo align_tests"),
            ],
        ),
    );
    agents.insert(
        "tester".to_string(),
        make_agent(&["qa_testing"], &[("qa_testing", "echo qa_testing")]),
    );
    agents.insert(
        "reviewer".to_string(),
        make_agent(
            &["doc_governance", "review", "loop_guard"],
            &[
                ("doc_governance", "echo doc_governance"),
                ("review", "echo review"),
                ("loop_guard", "echo loop_guard"),
            ],
        ),
    );

    OrchestratorConfig {
        runner: RunnerConfig::default(),
        resume: ResumeConfig { auto: false },
        defaults: ConfigDefaults {
            project: String::new(),
            workspace: "default".to_string(),
            workflow: "bootstrap".to_string(),
        },
        projects: HashMap::new(),
        workspaces: {
            let mut ws = HashMap::new();
            ws.insert(
                "default".to_string(),
                WorkspaceConfig {
                    root_path: "workspace/default".to_string(),
                    qa_targets: vec!["docs/qa".to_string()],
                    ticket_dir: "docs/ticket".to_string(),
                    self_referential: false,
                },
            );
            ws
        },
        agents,
        workflows: {
            let mut workflows = HashMap::new();
            workflows.insert(
                "bootstrap".to_string(),
                WorkflowConfig {
                    steps: vec![
                        WorkflowStepConfig {
                            id: "plan".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::Plan),
                            builtin: None,
                            required_capability: Some("plan".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "qa_doc_gen".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::QaDocGen),
                            builtin: None,
                            required_capability: Some("qa_doc_gen".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "implement".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::Implement),
                            builtin: None,
                            required_capability: Some("implement".to_string()),
                            enabled: true,
                            repeatable: true,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "qa_testing".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::QaTesting),
                            builtin: None,
                            required_capability: Some("qa_testing".to_string()),
                            enabled: true,
                            repeatable: true,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "ticket_fix".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::TicketFix),
                            builtin: None,
                            required_capability: Some("ticket_fix".to_string()),
                            enabled: true,
                            repeatable: true,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "align_tests".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::AlignTests),
                            builtin: None,
                            required_capability: Some("align_tests".to_string()),
                            enabled: true,
                            repeatable: true,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "doc_governance".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::DocGovernance),
                            builtin: None,
                            required_capability: Some("doc_governance".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "loop_guard".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::LoopGuard),
                            builtin: Some("loop_guard".to_string()),
                            required_capability: None,
                            enabled: true,
                            repeatable: true,
                            is_guard: true,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                        chain_steps: vec![],
                        },
                    ],
                    loop_policy: WorkflowLoopConfig {
                        mode: LoopMode::Once,
                        guard: WorkflowLoopGuardConfig::default(),
                    },
                    finalize: WorkflowFinalizeConfig { rules: vec![] },
                    qa: None,
                    fix: None,
                    retest: None,
                    dynamic_steps: vec![],
                    safety: SafetyConfig::default(),
                },
            );
            workflows
        },
        resource_meta: ResourceMetadataStore::default(),
    }
}

#[test]
fn multi_agent_capability_config_validates() {
    use agent_orchestrator::config_load::validate_workflow_config;

    let config = multi_agent_config();
    let workflow = config.workflows.get("bootstrap").unwrap();
    let result = validate_workflow_config(&config, workflow, "bootstrap");
    assert!(
        result.is_ok(),
        "multi-agent config should validate: {:?}",
        result
    );
}

#[test]
fn build_execution_plan_contains_all_bootstrap_steps() {
    use agent_orchestrator::config::WorkflowStepType;
    use agent_orchestrator::config_load::build_execution_plan;

    let config = multi_agent_config();
    let workflow = config.workflows.get("bootstrap").unwrap();
    let plan =
        build_execution_plan(&config, workflow, "bootstrap").expect("execution plan should build");

    let step_ids: Vec<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();
    assert!(step_ids.contains(&"plan"), "missing plan step");
    assert!(step_ids.contains(&"qa_doc_gen"), "missing qa_doc_gen step");
    assert!(step_ids.contains(&"implement"), "missing implement step");
    assert!(step_ids.contains(&"qa_testing"), "missing qa_testing step");
    assert!(step_ids.contains(&"ticket_fix"), "missing ticket_fix step");
    assert!(
        step_ids.contains(&"align_tests"),
        "missing align_tests step"
    );
    assert!(
        step_ids.contains(&"doc_governance"),
        "missing doc_governance step"
    );
    assert!(step_ids.contains(&"loop_guard"), "missing loop_guard step");

    // Verify expected step properties
    let plan_step = plan.steps.iter().find(|s| s.id == "plan").unwrap();
    assert_eq!(plan_step.step_type, Some(WorkflowStepType::Plan));
    assert!(!plan_step.repeatable, "plan step should not be repeatable");

    let loop_guard_step = plan.steps.iter().find(|s| s.id == "loop_guard").unwrap();
    assert!(
        loop_guard_step.is_guard,
        "loop_guard should be a guard step"
    );
    assert_eq!(loop_guard_step.builtin.as_deref(), Some("loop_guard"));
}

#[test]
fn normalize_workflow_sets_required_capability_for_sdlc_steps() {
    use agent_orchestrator::config::{
        LoopMode, WorkflowConfig, WorkflowFinalizeConfig, WorkflowLoopConfig,
        WorkflowLoopGuardConfig, WorkflowStepConfig, WorkflowStepType,
    };
    use agent_orchestrator::config_load::normalize_workflow_config;

    let mut workflow = WorkflowConfig {
        steps: vec![
            WorkflowStepConfig {
                id: "qa_doc_gen".to_string(),
                description: None,
                step_type: Some(WorkflowStepType::QaDocGen),
                builtin: None,
                required_capability: None, // not set — normalize should fill it in
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: Vec::new(),
                pipe_to: None,
                command: None,
                        chain_steps: vec![],
            },
            WorkflowStepConfig {
                id: "align_tests".to_string(),
                description: None,
                step_type: Some(WorkflowStepType::AlignTests),
                builtin: None,
                required_capability: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: Vec::new(),
                pipe_to: None,
                command: None,
                        chain_steps: vec![],
            },
        ],
        loop_policy: WorkflowLoopConfig {
            mode: LoopMode::Once,
            guard: WorkflowLoopGuardConfig::default(),
        },
        finalize: WorkflowFinalizeConfig { rules: vec![] },
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: vec![],
        safety: agent_orchestrator::config::SafetyConfig::default(),
    };

    normalize_workflow_config(&mut workflow);

    let qa_doc_gen = workflow
        .steps
        .iter()
        .find(|s| s.id == "qa_doc_gen")
        .expect("qa_doc_gen step missing after normalize");
    assert_eq!(
        qa_doc_gen.required_capability.as_deref(),
        Some("qa_doc_gen"),
        "normalize should set required_capability for QaDocGen"
    );

    let align_tests = workflow
        .steps
        .iter()
        .find(|s| s.id == "align_tests")
        .expect("align_tests step missing after normalize");
    assert_eq!(
        align_tests.required_capability.as_deref(),
        Some("align_tests"),
        "normalize should set required_capability for AlignTests"
    );
}

#[test]
fn sdlc_full_pipeline_workflow_parses_from_fixture() {
    use agent_orchestrator::config::WorkflowStepType;

    let yaml = std::fs::read_to_string("../fixtures/manifests/bundles/self-bootstrap-test.yaml")
        .expect("fixture file missing");
    let resources = parse_resources_from_yaml(&yaml).expect("should parse");
    let mut config = minimal_config();
    for resource in resources {
        let registered = dispatch_resource(resource).unwrap();
        registered.apply(&mut config);
    }

    let workflow = config
        .workflows
        .get("sdlc_full_pipeline")
        .expect("sdlc_full_pipeline workflow missing");

    let step_types: Vec<Option<WorkflowStepType>> =
        workflow.steps.iter().map(|s| s.step_type.clone()).collect();

    assert!(
        step_types.contains(&Some(WorkflowStepType::Plan)),
        "sdlc_full_pipeline should have plan step"
    );
    assert!(
        step_types.contains(&Some(WorkflowStepType::QaDocGen)),
        "sdlc_full_pipeline should have qa_doc_gen step"
    );
    assert!(
        step_types.contains(&Some(WorkflowStepType::QaTesting)),
        "sdlc_full_pipeline should have qa_testing step"
    );
    assert!(
        step_types.contains(&Some(WorkflowStepType::TicketFix)),
        "sdlc_full_pipeline should have ticket_fix step"
    );
    assert!(
        step_types.contains(&Some(WorkflowStepType::AlignTests)),
        "sdlc_full_pipeline should have align_tests step"
    );
    assert!(
        step_types.contains(&Some(WorkflowStepType::DocGovernance)),
        "sdlc_full_pipeline should have doc_governance step"
    );
}
