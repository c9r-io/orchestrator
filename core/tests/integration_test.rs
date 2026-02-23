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
                },
            );
            workflows
        },
        resource_meta: ResourceMetadataStore::default(),
    }
}

fn workspace_yaml(name: &str, root_path: &str) -> String {
    format!(
        r#"apiVersion: orchestrator.dev/v1
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
        r#"apiVersion: orchestrator.dev/v1
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
    let yaml = r#"apiVersion: orchestrator.dev/v1
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
    let yaml = r#"apiVersion: orchestrator.dev/v1
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
    assert!(exported.contains("apiVersion: orchestrator.dev/v1"));
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
    let yaml = r#"apiVersion: orchestrator.dev/v1
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
