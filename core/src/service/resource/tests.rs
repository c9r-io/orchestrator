use super::*;
use crate::config_load::read_active_config;
use crate::dto::CreateTaskPayload;
use crate::task_ops::create_task_impl;
use crate::test_utils::TestState;
use serde_json::Value;
use std::collections::HashMap;

fn workflow_manifest(name: &str, command: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"{command}\"\n  loop:\n    mode: once\n"
    )
}

fn project_bundle_manifest(delete_workflow_name: &str, workspace_root: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {delete_workflow_name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo delete\"\n  loop:\n    mode: once\n"
    )
}

fn project_subset_manifest(workspace_root: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n"
    )
}

fn labeled_bundle_manifest(project: &str, workspace_root: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: labeled-ws\n  labels:\n    env: dev\n    tier: qa\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: unlabeled-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: labeled-agent\n  labels:\n    env: dev\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: labeled-workflow\n  project: {project}\n  labels:\n    env: dev\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n"
    )
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

    assert!(
        dry_run
            .results
            .iter()
            .any(|entry| entry.name == "delete-me" && entry.action == "deleted")
    );

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
        .data_dir
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

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();
    let bundle = project_bundle_manifest("delete-me", &ws_root_str);
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

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();

    let qa_file = ws_root.join("docs/qa/cross-project.md");
    std::fs::write(&qa_file, "# cross project\n").expect("seed qa file");

    let bundle = project_bundle_manifest("delete-me", &ws_root_str);
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
        &project_subset_manifest(&ws_root_str),
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

#[test]
fn get_resource_supports_named_queries_describe_and_selector_helpers() {
    let mut fixture = TestState::new();
    let state = fixture.build();

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();

    apply_manifests(
        &state,
        &labeled_bundle_manifest(crate::config::DEFAULT_PROJECT_ID, &ws_root_str),
        false,
        Some(crate::config::DEFAULT_PROJECT_ID),
        false,
    )
    .expect("seed labeled resources");

    let named = get_resource(
        &state,
        "workspace/labeled-ws",
        None,
        "yaml",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("get named workspace");
    assert!(named.contains(&format!("root_path: {}", ws_root_str)));

    let listed = get_resource(
        &state,
        "workspaces",
        None,
        "json",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("list workspaces");
    let listed_json: Value = serde_json::from_str(&listed).expect("parse filtered list");
    let listed_values = listed_json.as_array().expect("workspace name array");
    assert!(listed_values.contains(&Value::String("labeled-ws".to_string())));
    assert!(listed_values.contains(&Value::String("unlabeled-ws".to_string())));

    let described = describe_resource(
        &state,
        "agent/labeled-agent",
        "json",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("describe agent");
    assert!(described.contains("\"command\""));

    let named_with_selector = get_resource(
        &state,
        "workflow/labeled-workflow",
        Some("env=dev"),
        "json",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect_err("named query with selector should fail");
    assert!(
        named_with_selector
            .to_string()
            .contains("label selector (-l) cannot be used")
    );

    let conditions = parse_label_selector("env=dev,tier=qa").expect("parse selector");
    assert_eq!(
        conditions,
        vec![
            ("env".to_string(), "dev".to_string()),
            ("tier".to_string(), "qa".to_string())
        ]
    );

    let mut labels = std::collections::HashMap::new();
    labels.insert("env".to_string(), "dev".to_string());
    labels.insert("tier".to_string(), "qa".to_string());
    assert!(match_labels(Some(&labels), &conditions));
    assert!(!match_labels(
        Some(&labels),
        &[("env".to_string(), "prod".to_string())]
    ));

    let invalid_selector = parse_label_selector("env").expect_err("invalid selector should fail");
    assert!(
        invalid_selector
            .to_string()
            .contains("invalid label selector")
    );
}

#[test]
fn apply_manifests_reports_metadata_project_mismatch() {
    let mut fixture = TestState::new();
    let state = fixture.build();

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();

    let response = apply_manifests(
        &state,
        &labeled_bundle_manifest("beta", &ws_root_str),
        false,
        Some("alpha"),
        false,
    )
    .expect("apply should return response");

    assert!(
        response
            .errors
            .iter()
            .any(|error| error.contains("project mismatch"))
    );
}

#[test]
fn delete_resource_covers_force_dry_run_and_actual_delete() {
    let mut fixture = TestState::new();
    let state = fixture.build();

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();

    apply_manifests(
        &state,
        &project_bundle_manifest("delete-me", &ws_root_str),
        false,
        Some("alpha"),
        false,
    )
    .expect("seed alpha project");

    let missing_force = delete_resource(&state, "workflow/delete-me", false, Some("alpha"), false)
        .expect_err("force should be required");
    assert!(missing_force.to_string().contains("use --force"));

    let missing = delete_resource(&state, "workflow/missing", true, Some("alpha"), true)
        .expect_err("missing dry run should fail");
    assert!(missing.to_string().contains("not found in project 'alpha'"));

    delete_resource(&state, "workflow/delete-me", true, Some("alpha"), true)
        .expect("dry run should succeed for existing workflow");
    delete_resource(&state, "workflow/delete-me", true, Some("alpha"), false)
        .expect("actual workflow delete");

    let active = read_active_config(&state).expect("read active config");
    let alpha = active.config.projects.get("alpha").expect("alpha project");
    assert!(!alpha.workflows.contains_key("delete-me"));
}

#[test]
fn export_manifests_supports_json_and_yaml() {
    let mut fixture = TestState::new();
    let state = fixture.build();

    let ws_root = state.data_dir.join("workspace/default");
    let ws_root_str = ws_root.to_string_lossy();

    apply_manifests(
        &state,
        &project_bundle_manifest("delete-me", &ws_root_str),
        false,
        Some("alpha"),
        false,
    )
    .expect("seed project for export");

    let json = export_manifests(&state, "json").expect("export json");
    let json_value: Value = serde_json::from_str(&json).expect("parse export json");
    let docs = json_value.as_array().expect("json export array");
    assert!(!docs.is_empty());
    assert!(
        docs.iter()
            .any(|doc| doc.get("kind") == Some(&Value::String("Workspace".to_string())))
    );

    let yaml = export_manifests(&state, "yaml").expect("export yaml");
    assert!(yaml.contains("kind: Workspace"));
    assert!(yaml.contains("kind: Workflow"));
}

#[test]
fn helper_functions_cover_delete_and_projection_paths() {
    let mut project = crate::config::ProjectConfig {
        description: None,
        workspaces: HashMap::from([(
            "ws".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "workspace/default".to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
                health_policy: Default::default(),
            },
        )]),
        agents: HashMap::from([(
            "agent".to_string(),
            crate::config::AgentConfig {
                enabled: true,
                metadata: crate::config::AgentMetadata {
                    name: "agent".to_string(),
                    description: None,
                    version: None,
                    cost: None,
                },
                capabilities: vec!["implement".to_string()],
                command: "echo '{\"confidence\":1.0,\"quality_score\":1.0,\"artifacts\":[]}'"
                    .to_string(),
                selection: crate::config::AgentSelectionConfig::default(),
                env: None,
                prompt_delivery: crate::config::PromptDelivery::default(),
                health_policy: Default::default(),
                command_rules: Vec::new(),
            },
        )]),
        workflows: HashMap::from([(
            "wf".to_string(),
            crate::config::WorkflowConfig {
                steps: vec![],
                execution: Default::default(),
                loop_policy: crate::config::WorkflowLoopConfig {
                    mode: crate::config::LoopMode::Once,
                    guard: crate::config::WorkflowLoopGuardConfig::default(),
                    convergence_expr: None,
                },
                finalize: crate::config::WorkflowFinalizeConfig::default(),
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                adaptive: None,
                safety: crate::config::SafetyConfig::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                item_isolation: None,
            },
        )]),
        step_templates: HashMap::new(),
        env_stores: HashMap::new(),
        secret_stores: HashMap::new(),
        execution_profiles: HashMap::new(),
        triggers: HashMap::new(),
    };

    assert_eq!(
        canonical_project_kind("execution_profile").expect("canonical kind"),
        "ExecutionProfile"
    );
    assert!(canonical_project_kind("unknown").is_err());
    assert!(
        delete_resource_from_project(&mut project, "workspace", "ws").expect("delete workspace")
    );
    assert!(delete_resource_from_project(&mut project, "agent", "agent").expect("delete agent"));
    assert!(delete_resource_from_project(&mut project, "workflow", "wf").expect("delete workflow"));
    assert!(
        !delete_resource_from_project(&mut project, "workflow", "missing")
            .expect("missing workflow")
    );

    let mut config = crate::config::OrchestratorConfig::default();
    autofill_defaults_for_manifest_mode(&mut config);
    assert!(
        config
            .projects
            .contains_key(crate::config::DEFAULT_PROJECT_ID)
    );

    assert_eq!(apply_action_label(ApplyResult::Created), "created");
    assert_eq!(apply_action_label(ApplyResult::Configured), "updated");
    assert_eq!(apply_action_label(ApplyResult::Unchanged), "unchanged");
}
