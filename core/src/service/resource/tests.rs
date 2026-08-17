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
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n  driver:\n    provider: shell\n    transport: cli\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: {delete_workflow_name}\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo delete\"\n  loop:\n    mode: once\n"
    )
}

fn project_subset_manifest(workspace_root: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: shared-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: shared-agent\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n  driver:\n    provider: shell\n    transport: cli\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: keep-me\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n"
    )
}

fn labeled_bundle_manifest(project: &str, workspace_root: &str) -> String {
    format!(
        "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: labeled-ws\n  labels:\n    env: dev\n    tier: qa\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: unlabeled-ws\nspec:\n  root_path: \"{workspace_root}\"\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n  self_referential: false\n---\napiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: labeled-agent\n  labels:\n    env: dev\nspec:\n  capabilities:\n    - implement\n  command: \"echo '{{\\\"confidence\\\":1.0,\\\"quality_score\\\":1.0,\\\"artifacts\\\":[]}}'\"\n  driver:\n    provider: shell\n    transport: cli\n---\napiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: labeled-workflow\n  project: {project}\n  labels:\n    env: dev\nspec:\n  steps:\n    - id: implement\n      type: implement\n      enabled: true\n      command: \"echo keep\"\n  loop:\n    mode: once\n"
    )
}

fn expert_resource_extensions_manifest() -> &'static str {
    "apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: reviewed-step
spec:
  description: Reviewed expert resource fixture
  prompt: Review the selected resource.
---
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: reviewed-profile
spec:
  mode: sandbox
  fs_mode: workspace_readonly
  network_mode: deny
"
}

#[test]
fn apply_command_only_agent_is_rejected_and_not_persisted() {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let manifest = "apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: legacy-command
spec:
  capabilities: [implement]
  command: echo {prompt}
";

    // Apply reports a manifest rejection in-band (DD-137): the call succeeds and
    // the refusal arrives in `errors`/`diagnostics`. An `expect_err` here would
    // fail even against a correct implementation.
    let response = apply_manifests(
        &state,
        manifest,
        false,
        Some(crate::config::DEFAULT_PROJECT_ID),
        false,
    )
    .expect("apply returns the rejection in-band, not as a transport error");

    assert!(
        response.results.is_empty(),
        "nothing may be applied: {:?}",
        response.results
    );
    let error = response.errors.join(" | ");
    assert!(error.contains("agent.spec.driver is required"), "{error}");
    assert!(error.contains("provider: shell"), "{error}");
    assert!(
        response
            .diagnostics
            .iter()
            .any(|d| d.code == "manifest_invalid"),
        "the refusal must be machine-readable: {:?}",
        response.diagnostics
    );

    // The rejection has to happen before anything is persisted. Asserting only
    // the error would pass on an implementation that stored the Agent and then
    // complained.
    let active = read_active_config(&state).expect("read active config");
    assert!(
        !active.config.projects[crate::config::DEFAULT_PROJECT_ID]
            .agents
            .contains_key("legacy-command"),
        "a rejected Agent must not be persisted"
    );
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
    assert!(named.contains(&format!("work_dir: {ws_root_str}")));

    let editable = describe_resource(
        &state,
        "workspace/labeled-ws",
        "yaml",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("describe editable workspace");
    let parsed = crate::resource::parse_manifests_from_yaml(&editable)
        .expect("described builtin must remain an apply-compatible manifest");
    assert_eq!(parsed.len(), 1);
    assert!(editable.contains("apiVersion: orchestrator.dev/v2"));
    assert!(!editable.contains("generation:"));

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
fn expert_resource_catalog_is_structured_bounded_and_revision_stable() {
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
    .expect("seed base expert resources");
    apply_manifests(
        &state,
        expert_resource_extensions_manifest(),
        false,
        Some(crate::config::DEFAULT_PROJECT_ID),
        false,
    )
    .expect("seed step template and execution profile");

    let first_page = list_resource_summaries(
        &state,
        "workspaces",
        Some(crate::config::DEFAULT_PROJECT_ID),
        None,
        1,
    )
    .expect("first catalog page");
    assert_eq!(first_page.resources.len(), 1);
    let cursor = first_page.next_cursor.expect("bounded page cursor");
    let second_page = list_resource_summaries(
        &state,
        "workspaces",
        Some(crate::config::DEFAULT_PROJECT_ID),
        Some(&cursor),
        100,
    )
    .expect("second catalog page");
    assert!(!second_page.resources.is_empty());
    assert!(
        second_page
            .resources
            .iter()
            .all(|resource| resource.name > cursor)
    );

    for (query, kind, expected_name) in [
        ("workspaces", "Workspace", "labeled-ws"),
        ("workflows", "Workflow", "labeled-workflow"),
        ("agents", "Agent", "labeled-agent"),
        ("steptemplates", "StepTemplate", "reviewed-step"),
        ("executionprofiles", "ExecutionProfile", "reviewed-profile"),
    ] {
        let page = list_resource_summaries(
            &state,
            query,
            Some(crate::config::DEFAULT_PROJECT_ID),
            None,
            100,
        )
        .expect("catalog kind");
        let summary = page
            .resources
            .iter()
            .find(|resource| resource.name == expected_name)
            .expect("expected resource summary");
        assert_eq!(summary.kind, kind);
        assert_eq!(summary.project_id, crate::config::DEFAULT_PROJECT_ID);
        assert_eq!(summary.revision.len(), 64);
    }

    let step = get_resource(
        &state,
        "steptemplate/reviewed-step",
        None,
        "yaml",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("named step template");
    assert!(step.contains("Review the selected resource"));
    let profile = get_resource(
        &state,
        "executionprofile/reviewed-profile",
        None,
        "yaml",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("named execution profile");
    assert!(profile.contains("workspace_readonly"));

    let described_revision = resource_content_revision(
        &describe_resource(
            &state,
            "workspace/labeled-ws",
            "yaml",
            Some(crate::config::DEFAULT_PROJECT_ID),
        )
        .expect("describe labeled workspace"),
    )
    .expect("describe revision");
    let current_revision = current_resource_revision(
        &state,
        crate::cli_types::ResourceKind::Workspace,
        "labeled-ws",
        Some(crate::config::DEFAULT_PROJECT_ID),
    )
    .expect("current revision")
    .expect("existing workspace");
    assert_eq!(described_revision, current_revision);

    let ordered = "kind: Workspace\nmetadata:\n  name: stable\n";
    let reordered = "metadata:\n  name: stable\nkind: Workspace\n";
    assert_eq!(
        resource_content_revision(ordered).expect("ordered hash"),
        resource_content_revision(reordered).expect("reordered hash")
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
                kind: Default::default(),
                root_path: "workspace/default".to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
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
                driver: None,
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
        source_task_templates: HashMap::new(),
        source_task_bindings: HashMap::new(),
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

#[test]
fn driver_apply_errors_expose_stable_structured_diagnostics() {
    let diagnostic = apply_diagnostic(
        "[driver_multi_turn_required] Workflow / pilot step implement requires multi-turn",
    );
    assert_eq!(diagnostic.code, "driver_multi_turn_required");
    assert_eq!(
        diagnostic.field_path.as_deref(),
        Some("spec.steps[].behavior.driverRequirements")
    );

    let mut config = crate::config::OrchestratorConfig::default();
    config.projects.insert(
        crate::config::DEFAULT_PROJECT_ID.to_string(),
        crate::config::ProjectConfig::default(),
    );
    let project = config
        .projects
        .get_mut(crate::config::DEFAULT_PROJECT_ID)
        .expect("default project");
    let mut agent = crate::config::AgentConfig::new();
    agent.driver = Some(crate::config::AgentDriverConfig {
        provider: crate::config::DriverProvider::Codex,
        transport: crate::config::DriverTransport::Cli,
        binary: None,
        options: Default::default(),
        claude: None,
        codex: None,
        shell: None,
        raw_args: vec!["--dangerously-bypass-approvals-and-sandbox".to_string()],
        unsafe_raw_args: true,
    });
    project.agents.insert("unsafe-codex".to_string(), agent);
    let mut errors = Vec::new();
    validate_driver_raw_args(
        &config,
        crate::config::DEFAULT_PROJECT_ID,
        false,
        &mut errors,
    );
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("driver_raw_args_unsafe_mode_required"));
}

/// FR-171: which read entry points recognise which `ResourceKind`.
///
/// The property under test is **entry-point reachability**, not the arm count of
/// any one function. That distinction is the whole point: FR-171 was filed
/// claiming `describe` supported five kinds because `describe_builtin_resource`
/// has five typed arms, and it was wrong — that function returns `Ok(None)` for
/// the rest and `describe_resource` falls back to `get_resource`. A gate that
/// counted arms would have preserved the error it was written to prevent, so
/// every probe below goes through the public entry point a command calls.
mod resource_observability_matrix {
    use super::*;
    use crate::cli_types::ResourceKind;

    /// What each kind must answer, per the FR-171 adjudication.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Adjudication {
        /// `orchestrator get <plural>` recognises the type.
        list: bool,
        /// `orchestrator get <singular>/<name>` recognises the type.
        single: bool,
    }

    /// Query names and adjudication per kind.
    ///
    /// Wildcard-free on purpose, following the `apply_action_naming` idiom in
    /// `crates/daemon/src/server/resource.rs`: a thirteenth `ResourceKind`
    /// variant does not fail an assertion, it **fails to compile**, so the
    /// adjudication cannot silently omit it. The compiler is the derivation from
    /// the enum; `ALL_KINDS` below only keeps the iteration list honest.
    fn adjudicated(kind: ResourceKind) -> (&'static str, &'static str, Adjudication) {
        let both = Adjudication {
            list: true,
            single: true,
        };
        match kind {
            ResourceKind::Workspace => ("workspaces", "workspace", both),
            ResourceKind::Agent => ("agents", "agent", both),
            ResourceKind::Workflow => ("workflows", "workflow", both),
            ResourceKind::StepTemplate => ("steptemplates", "steptemplate", both),
            ResourceKind::ExecutionProfile => ("executionprofiles", "executionprofile", both),
            ResourceKind::Trigger => ("triggers", "trigger", both),
            ResourceKind::SourceTaskTemplate => ("sourcetasktemplates", "sourcetasktemplate", both),
            ResourceKind::SourceTaskBinding => ("sourcetaskbindings", "sourcetaskbinding", both),
            // Added by FR-171.
            ResourceKind::EnvStore => ("envstores", "envstore", both),
            ResourceKind::SecretStore => ("secretstores", "secretstore", both),
            ResourceKind::Project => ("projects", "project", both),
            // RuntimePolicy is deliberately not listable. It is a resolved
            // singleton — `get_from_project` returns the effective policy for
            // any name, walking project -> `_system` -> defaults, and
            // `delete_from_project` is hardcoded false — so a collection query
            // would imply a second one could be applied. Single read only.
            ResourceKind::RuntimePolicy => (
                "runtimepolicies",
                "runtimepolicy",
                Adjudication {
                    list: false,
                    single: true,
                },
            ),
        }
    }

    const ALL_KINDS: [ResourceKind; 12] = [
        ResourceKind::Workspace,
        ResourceKind::Agent,
        ResourceKind::Workflow,
        ResourceKind::Project,
        ResourceKind::RuntimePolicy,
        ResourceKind::StepTemplate,
        ResourceKind::SourceTaskTemplate,
        ResourceKind::SourceTaskBinding,
        ResourceKind::ExecutionProfile,
        ResourceKind::EnvStore,
        ResourceKind::SecretStore,
        ResourceKind::Trigger,
    ];

    #[test]
    fn all_kinds_covers_every_variant() {
        fn index(kind: ResourceKind) -> usize {
            match kind {
                ResourceKind::Workspace => 0,
                ResourceKind::Agent => 1,
                ResourceKind::Workflow => 2,
                ResourceKind::Project => 3,
                ResourceKind::RuntimePolicy => 4,
                ResourceKind::StepTemplate => 5,
                ResourceKind::SourceTaskTemplate => 6,
                ResourceKind::SourceTaskBinding => 7,
                ResourceKind::ExecutionProfile => 8,
                ResourceKind::EnvStore => 9,
                ResourceKind::SecretStore => 10,
                ResourceKind::Trigger => 11,
            }
        }
        let mut seen = [false; 12];
        for kind in ALL_KINDS {
            seen[index(kind)] = true;
        }
        assert!(
            seen.iter().all(|hit| *hit),
            "ALL_KINDS has fallen behind ResourceKind"
        );
    }

    /// The adjudication above is written down, not derived — §4.4's rule is that
    /// a judgement has no ledger to derive from. But it is not unconstrained
    /// either: the codebase had already declared which kinds are collections,
    /// twice, before anyone wrote the reason down.
    ///
    /// `builtin_crd_definitions()` carries `CrdScope`, and `RuntimePolicy` is the
    /// only builtin marked `Singleton` (the enum's own doc comment says
    /// "Singleton resources such as RuntimePolicy"). Independently,
    /// `is_builtin_alias` reserves a plural name against CRD use for eleven of
    /// twelve kinds and reserves only the singular for RuntimePolicy.
    ///
    /// This test asserts the two agree. If someone changes a scope, it fails
    /// naming the kind rather than silently following the change — which is what
    /// deriving `list` from `CrdScope` would have done instead.
    #[test]
    fn adjudication_agrees_with_the_declared_crd_scope() {
        use crate::crd::builtin_defs::builtin_crd_definitions;
        use crate::crd::scope::CrdScope;

        let scopes: std::collections::HashMap<String, CrdScope> = builtin_crd_definitions()
            .iter()
            .map(|def| (def.kind.clone(), def.scope))
            .collect();

        // The two registries are not mirrors of each other, which this test
        // found rather than assumed: `Trigger` is a `ResourceKind` with no
        // builtin CRD definition, and the registry separately carries
        // `WorkflowStore` and `StoreBackendProvider`, which are CRDs and not
        // `ResourceKind` variants — the same class of drift DD-182 found when the
        // guide listed `WorkflowStore` among the built-in kinds.
        //
        // So the cross-check runs over the intersection and *names* what it
        // skipped. A silently shrinking comparison set is how this kind of test
        // reports success over ground it stopped covering.
        let mut unchecked = Vec::new();
        for kind in ALL_KINDS {
            let (_, _, want) = adjudicated(kind);
            let canonical = format!("{kind:?}");
            let Some(scope) = scopes.get(&canonical) else {
                unchecked.push(canonical);
                continue;
            };
            let listable_by_scope = *scope != CrdScope::Singleton;
            assert_eq!(
                want.list, listable_by_scope,
                "{canonical}: adjudication says list={}, declared CrdScope {:?} implies list={}",
                want.list, scope, listable_by_scope
            );
        }
        assert_eq!(
            unchecked,
            vec!["Trigger".to_string()],
            "the set of ResourceKinds without a builtin CRD definition changed; \
             each one is a kind this cross-check cannot see"
        );

        // The adjudication rests on RuntimePolicy being the only singleton. That
        // is asserted directly, so a second kind becoming `Singleton` fails here
        // instead of quietly making the rule ambiguous.
        let singletons: Vec<&String> = scopes
            .iter()
            .filter(|(_, scope)| **scope == CrdScope::Singleton)
            .map(|(kind, _)| kind)
            .collect();
        assert_eq!(
            singletons,
            vec!["RuntimePolicy"],
            "RuntimePolicy is expected to be the only Singleton builtin"
        );
    }

    /// Recognition is probed with **no instance applied**, which is what
    /// separates "this entry point knows the type" from "there is one of these".
    /// A recognised singular read of an absent name says `<Kind> not found`; an
    /// unrecognised one says `unknown resource type`. Those are different
    /// sentences, and the difference is the measurement.
    #[test]
    fn every_entry_point_matches_its_adjudication() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let project = Some(crate::config::DEFAULT_PROJECT_ID);

        for kind in ALL_KINDS {
            let (plural, singular, want) = adjudicated(kind);

            let list = get_resource(&state, plural, None, "yaml", project);
            let list_recognised = match &list {
                Ok(_) => true,
                Err(err) => !err.to_string().contains("unknown list resource type"),
            };
            assert_eq!(
                list_recognised, want.list,
                "list recognition for {kind:?} via `get {plural}`: {list:?}"
            );

            let named = format!("{singular}/fr171-absent-probe");
            let single = get_resource(&state, &named, None, "yaml", project);
            let single_recognised = match &single {
                Ok(_) => true,
                Err(err) => !err.to_string().contains("unknown resource type"),
            };
            assert_eq!(
                single_recognised, want.single,
                "single recognition for {kind:?} via `get {named}`: {single:?}"
            );

            // `describe` and the single read must agree **exactly**, on success
            // and on failure alike.
            //
            // The first version of this check asked only whether describe's error
            // mentioned `unknown resource type`, and that was §4.4 shape 1 written
            // into the very test meant to guard against it: verified by mutation,
            // making `describe_builtin_resource`'s `_ => Ok(None)` return an error
            // instead left all four tests green, because a *different* error
            // message read as "recognised". Comparing the two outcomes needs no
            // taxonomy of error text — a fallback that stops working makes the two
            // entry points disagree, whatever it says.
            //
            // Absent instances are compared on purpose: for a present resource the
            // two paths render differently (typed vs. store), which FR-171
            // deliberately does not converge.
            let described = describe_resource(&state, &named, "yaml", project);
            let single_outcome = match &single {
                Ok(content) => Ok(content.clone()),
                Err(err) => Err(err.to_string()),
            };
            let describe_outcome = match &described {
                Ok(content) => Ok(content.clone()),
                Err(err) => Err(err.to_string()),
            };
            assert_eq!(
                describe_outcome, single_outcome,
                "`describe {named}` and `get {named}` must return the same outcome for {kind:?}"
            );
        }
    }

    /// The GUI's resource catalog (`list_resource_summaries`) renders every row
    /// through `describe_builtin_resource`, so it can only page kinds with a typed
    /// renderer — eight of twelve. Before FR-171 it served five and answered the
    /// other seven with `unsupported expert resource catalog type`, which reports
    /// "the product has not decided" as "you typed something invalid".
    ///
    /// The three excluded-but-readable kinds and the singleton now get distinct
    /// diagnostics. Asserting the diagnostics rather than the exit code is the
    /// point: three refusals that differ only in exit status cannot tell an
    /// operator which of three reasons applies.
    #[test]
    fn the_resource_catalog_pages_eight_kinds_and_explains_the_rest() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let project = Some(crate::config::DEFAULT_PROJECT_ID);

        let manifest = concat!(
            "apiVersion: orchestrator.dev/v2\nkind: EnvStore\nmetadata:\n  name: catalog-env\n",
            "spec:\n  data:\n    A: b\n",
            "---\napiVersion: orchestrator.dev/v2\nkind: SecretStore\nmetadata:\n  name: catalog-secret\n",
            "spec:\n  data:\n    K: catalog-secret-value\n",
        );
        apply_manifests(&state, manifest, false, project, false).expect("seed catalog resources");

        for (query, expected_name) in [
            ("envstores", "catalog-env"),
            ("secretstores", "catalog-secret"),
            ("projects", crate::config::DEFAULT_PROJECT_ID),
        ] {
            let page = list_resource_summaries(&state, query, project, None, 50)
                .unwrap_or_else(|err| panic!("catalog must page {query}: {err:?}"));
            assert!(
                page.resources.iter().any(|row| row.name == expected_name),
                "catalog page for {query} is missing {expected_name}: {:?}",
                page.resources
            );
        }

        // A paged SecretStore row carries no spec, so there is nothing to redact —
        // asserted rather than assumed, because "the page has no values" is the
        // property that lets the catalog list secret stores at all.
        let secret_page = list_resource_summaries(&state, "secretstores", project, None, 50)
            .expect("page secret stores");
        let rendered = format!("{:?}", secret_page.resources);
        assert!(
            !rendered.contains("catalog-secret-value"),
            "a catalog page must not carry secret values: {rendered}"
        );

        let singleton = list_resource_summaries(&state, "runtimepolicies", project, None, 50)
            .expect_err("RuntimePolicy is not a collection");
        assert!(
            singleton.to_string().contains("singleton"),
            "the refusal must say why, not just refuse: {singleton}"
        );

        // Trigger exercises the asymmetry between the two registries: it is a
        // `ResourceKind` with no builtin CRD definition, so the refusal can give
        // the reason but not the canonical spelling. Asserted as it actually is,
        // rather than asserting a canonical name the code cannot produce.
        let no_renderer = list_resource_summaries(&state, "triggers", project, None, 50)
            .expect_err("Trigger has no typed renderer");
        let message = no_renderer.to_string();
        assert!(message.contains("triggers"), "got: {message}");
        assert!(
            message.contains("typed renderer"),
            "the refusal must distinguish itself from the singleton case: {message}"
        );

        // SourceTaskTemplate does have a CRD definition, so the same refusal
        // names the canonical kind. Both shapes are covered because they take
        // different arms.
        let named_renderer =
            list_resource_summaries(&state, "sourcetasktemplates", project, None, 50)
                .expect_err("SourceTaskTemplate has no typed renderer");
        assert!(
            named_renderer.to_string().contains("SourceTaskTemplate"),
            "got: {named_renderer}"
        );

        let unknown = list_resource_summaries(&state, "nonsuchkind", project, None, 50)
            .expect_err("an unknown type is still an unknown type");
        assert!(
            unknown
                .to_string()
                .contains("unknown resource catalog type"),
            "got: {unknown}"
        );
    }

    /// The recognition probes above read a diagnostic, which is a proxy. This
    /// one observes the fact itself: seed each kind FR-171 added and require the
    /// resource to come back.
    #[test]
    fn kinds_added_by_fr171_are_actually_retrievable() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let project = Some(crate::config::DEFAULT_PROJECT_ID);

        let manifest = concat!(
            "apiVersion: orchestrator.dev/v2\nkind: EnvStore\nmetadata:\n  name: shared-env\n",
            "spec:\n  data:\n    LOG_LEVEL: debug\n",
            "---\napiVersion: orchestrator.dev/v2\nkind: SecretStore\nmetadata:\n  name: api-keys\n",
            "spec:\n  data:\n    OPENAI_API_KEY: sk-matrix-7c31\n",
        );
        apply_manifests(&state, manifest, false, project, false).expect("seed stores");

        let env_listed =
            get_resource(&state, "envstores", None, "yaml", project).expect("list env stores");
        assert!(env_listed.contains("shared-env"), "got: {env_listed}");

        let env_single = get_resource(&state, "envstore/shared-env", None, "yaml", project)
            .expect("get env store");
        assert!(env_single.contains("LOG_LEVEL"), "got: {env_single}");

        let secret_listed = get_resource(&state, "secretstores", None, "yaml", project)
            .expect("list secret stores");
        assert!(secret_listed.contains("api-keys"), "got: {secret_listed}");

        // Listing yields names only, and a single read redacts. Both halves are
        // asserted: absence of the value alone would also pass if SecretStore
        // dropped out of the read path entirely.
        let secret_single = get_resource(&state, "secretstore/api-keys", None, "yaml", project)
            .expect("get secret store");
        assert!(
            !secret_single.contains("sk-matrix-7c31"),
            "a read must not expose a secret value: {secret_single}"
        );
        assert!(
            secret_single.contains(crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER),
            "a read must show the placeholder, not omit the key: {secret_single}"
        );
        assert!(
            secret_single.contains("OPENAI_API_KEY"),
            "a read must still show which keys the store defines: {secret_single}"
        );

        // Project is the one non-project-scoped kind: it is read from the whole
        // config, so `--project` does not narrow it.
        let projects_listed =
            get_resource(&state, "projects", None, "yaml", project).expect("list projects");
        assert!(
            projects_listed.contains(crate::config::DEFAULT_PROJECT_ID),
            "got: {projects_listed}"
        );

        // A name that lists must also read. Without this the ruling is only half
        // asserted, and "listable but reports not-found on a single read" — the
        // inconsistency FR-171's acceptance names — would pass.
        let project_single = get_resource(
            &state,
            &format!("project/{}", crate::config::DEFAULT_PROJECT_ID),
            None,
            "yaml",
            project,
        )
        .expect("get the project that the list just returned");
        assert!(
            project_single.contains(crate::config::DEFAULT_PROJECT_ID),
            "got: {project_single}"
        );
        assert!(
            project_single.contains("Project"),
            "a single read must render the kind: {project_single}"
        );

        // Every name the list returns must read, not merely the one this test
        // happens to name — the difference between an assertion about a fixture
        // and an assertion about the surface.
        for line in projects_listed.lines() {
            let name = line.trim_start_matches("- ").trim();
            if name.is_empty() || name == "[]" {
                continue;
            }
            get_resource(&state, &format!("project/{name}"), None, "yaml", project)
                .unwrap_or_else(|err| panic!("listed project {name} must be readable: {err:?}"));
        }

        // RuntimePolicy resolves for any name and is absent from the list
        // surface — the two halves of its adjudication, asserted together so
        // neither can be satisfied alone.
        let policy = get_resource(&state, "runtimepolicy/effective", None, "yaml", project)
            .expect("runtime policy resolves for any name");
        assert!(policy.contains("RuntimePolicy"), "got: {policy}");
        let policy_list = get_resource(&state, "runtimepolicies", None, "yaml", project);
        assert!(
            policy_list
                .as_ref()
                .err()
                .map(|err| err.to_string().contains("unknown list resource type"))
                .unwrap_or(false),
            "RuntimePolicy must not be listable: {policy_list:?}"
        );
    }
}
