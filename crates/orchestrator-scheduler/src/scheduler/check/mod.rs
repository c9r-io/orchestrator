//! Preflight cross-reference validation for orchestrator configuration.
//!
//! Pure logic layer — no DB, no async. The CLI handler loads the config and
//! calls [`run_checks`], then renders the resulting [`CheckReport`].

mod capability;
mod execution;
mod safety;
mod workflow;
mod workspace;

use agent_orchestrator::anomaly::Severity;
use agent_orchestrator::config::ActiveConfig;
use agent_orchestrator::self_referential_policy::PolicyDiagnostic;
use serde::Serialize;
use std::path::Path;

// ── Known constants ─────────────────────────────────────────────────

const KNOWN_SYSTEM_VARS: &[&str] = &[
    "task_id",
    "item_id",
    "cycle",
    "phase",
    "workspace_root",
    "source_tree",
    "build_output",
    "test_output",
    "diff",
    "build_errors",
    "test_failures",
    "rel_path",
];

// ── Data structures ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
/// Full preflight report containing every emitted check and aggregate counts.
pub struct CheckReport {
    /// Individual validation results emitted by the preflight engine.
    pub checks: Vec<CheckResult>,
    /// Aggregate counts grouped by outcome severity.
    pub summary: CheckSummary,
}

#[derive(Debug, Serialize, Clone)]
/// One preflight validation result.
pub struct CheckResult {
    /// Source subsystem that emitted the result.
    pub source: String,
    /// Stable rule identifier.
    pub rule: String,
    /// Severity assigned to the rule.
    pub severity: Severity,
    /// Whether the rule passed.
    pub passed: bool,
    /// Whether a failing result should block execution.
    pub blocking: bool,
    /// Human-readable result message.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional contextual details for the result.
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional scope label such as workflow or resource name.
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional actual value observed by the rule.
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional expected value communicated by the rule.
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional risk statement attached to the result.
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional remediation hint for the failing result.
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
/// Aggregate counts for a batch of preflight checks.
pub struct CheckSummary {
    /// Total number of emitted checks.
    pub total: u32,
    /// Number of checks that passed.
    pub passed: u32,
    /// Number of failing checks with error severity.
    pub errors: u32,
    /// Number of failing checks with warning severity.
    pub warnings: u32,
}

impl CheckResult {
    fn simple(
        rule: impl Into<String>,
        severity: Severity,
        passed: bool,
        message: impl Into<String>,
        context: Option<String>,
    ) -> Self {
        Self {
            source: "preflight".to_string(),
            rule: rule.into(),
            severity: severity.clone(),
            passed,
            blocking: !passed && severity == Severity::Error,
            message: message.into(),
            context,
            scope: None,
            actual: None,
            expected: None,
            risk: None,
            suggested_fix: None,
        }
    }
}

impl From<PolicyDiagnostic> for CheckResult {
    fn from(value: PolicyDiagnostic) -> Self {
        Self {
            source: value.source,
            rule: value.rule_id,
            severity: value.severity,
            passed: value.passed,
            blocking: value.blocking,
            message: value.message,
            context: None,
            scope: Some(value.scope),
            actual: value.actual,
            expected: value.expected,
            risk: value.risk,
            suggested_fix: value.suggested_fix,
        }
    }
}

// ── Entry point ─────────────────────────────────────────────────────

/// Run all preflight checks against the loaded configuration.
/// `workflow_filter`: if Some, only check steps in that workflow.
/// `project_id`: if Some, check project-scoped resources instead of global.
pub fn run_checks(
    config: &ActiveConfig,
    app_root: &Path,
    workflow_filter: Option<&str>,
    project_id: Option<&str>,
) -> CheckReport {
    let oc = &config.config;
    let mut checks = Vec::new();

    let effective_project = project_id.unwrap_or(agent_orchestrator::config::DEFAULT_PROJECT_ID);
    let (workspaces, agents, workflows, step_templates) =
        if let Some(project) = oc.projects.get(effective_project) {
            (
                &project.workspaces,
                &project.agents,
                &project.workflows,
                &project.step_templates,
            )
        } else {
            checks.push(CheckResult::simple(
                "project_not_found",
                Severity::Error,
                false,
                format!("project \"{effective_project}\" not found in config"),
                None,
            ));
            return build_report(checks);
        };

    workspace::check_workspace_roots(workspaces, app_root, &mut checks);
    workspace::check_qa_targets(workspaces, app_root, &mut checks);
    execution::check_execution_profile_backend_support(
        workspaces,
        workflows,
        effective_project,
        &oc.projects,
        app_root,
        workflow_filter,
        &mut checks,
    );
    capability::check_capability_coverage(agents, workflows, workflow_filter, &mut checks);
    capability::check_prompt_delivery(agents, &mut checks);
    capability::check_capability_templates(agents, &mut checks);
    workflow::check_builtin_names(workflows, workflow_filter, &mut checks);
    workflow::check_pipe_to_refs(workflows, workflow_filter, &mut checks);
    workflow::check_template_vars(step_templates, workflows, workflow_filter, &mut checks);
    workflow::check_empty_workflows(workflows, workflow_filter, &mut checks);
    safety::check_self_referential_policy(workspaces, workflows, workflow_filter, &mut checks);

    // Health policy summary per agent.
    // Resolve effective policy: agent-explicit > workspace-inherited > default.
    // When there is exactly one workspace, use its health policy as the
    // workspace-level fallback.  When multiple workspaces exist, there is no
    // agent-to-workspace mapping at config time, so fall back to the global
    // default to avoid incorrectly attributing another workspace's policy.
    let ws_health_policy: Option<&agent_orchestrator::config::HealthPolicyConfig> =
        if workspaces.len() == 1 {
            workspaces
                .values()
                .next()
                .map(|ws| &ws.health_policy)
                .filter(|hp| !hp.is_default())
        } else {
            None
        };
    for (name, agent) in agents {
        let hp = &agent.health_policy;
        if hp.is_default() {
            if let Some(ws_hp) = ws_health_policy {
                let disease_note = if ws_hp.disease_duration_hours == 0 {
                    "disease DISABLED".to_string()
                } else {
                    format!("duration={}h", ws_hp.disease_duration_hours)
                };
                checks.push(CheckResult::simple(
                    "agent_health_policy",
                    Severity::Info,
                    true,
                    format!(
                        "agent \"{name}\": health policy = inherited from workspace ({disease_note}, threshold={}, cap_success={})",
                        ws_hp.disease_threshold, ws_hp.capability_success_threshold
                    ),
                    None,
                ));
            } else {
                checks.push(CheckResult::simple(
                    "agent_health_policy",
                    Severity::Info,
                    true,
                    format!("agent \"{name}\": health policy = default (duration=5h, threshold=2, cap_success=0.5)"),
                    None,
                ));
            }
        } else {
            let disease_note = if hp.disease_duration_hours == 0 {
                "disease DISABLED".to_string()
            } else {
                format!("duration={}h", hp.disease_duration_hours)
            };
            checks.push(CheckResult::simple(
                "agent_health_policy",
                Severity::Info,
                true,
                format!(
                    "agent \"{name}\": health policy = custom ({disease_note}, threshold={}, cap_success={})",
                    hp.disease_threshold, hp.capability_success_threshold
                ),
                None,
            ));
        }
    }

    // Trigger reference integrity: action.workflow and action.workspace must exist.
    if let Some(project) = oc.projects.get(effective_project) {
        for (name, trigger) in &project.triggers {
            let wf_exists = workflows.contains_key(&trigger.action.workflow);
            checks.push(CheckResult::simple(
                "trigger_workflow_ref",
                Severity::Error,
                wf_exists,
                if wf_exists {
                    format!(
                        "trigger \"{}\" references workflow \"{}\" (found)",
                        name, trigger.action.workflow
                    )
                } else {
                    format!(
                        "trigger \"{}\" references workflow \"{}\" which does not exist",
                        name, trigger.action.workflow
                    )
                },
                None,
            ));
            let ws_exists = workspaces.contains_key(&trigger.action.workspace);
            checks.push(CheckResult::simple(
                "trigger_workspace_ref",
                Severity::Error,
                ws_exists,
                if ws_exists {
                    format!(
                        "trigger \"{}\" references workspace \"{}\" (found)",
                        name, trigger.action.workspace
                    )
                } else {
                    format!(
                        "trigger \"{}\" references workspace \"{}\" which does not exist",
                        name, trigger.action.workspace
                    )
                },
                None,
            ));
        }
    }

    build_report(checks)
}

fn build_report(checks: Vec<CheckResult>) -> CheckReport {
    let total = checks.len() as u32;
    let passed = checks.iter().filter(|c| c.passed).count() as u32;
    let errors = checks
        .iter()
        .filter(|c| !c.passed && c.severity == Severity::Error)
        .count() as u32;
    let warnings = checks
        .iter()
        .filter(|c| !c.passed && c.severity == Severity::Warning)
        .count() as u32;

    CheckReport {
        checks,
        summary: CheckSummary {
            total,
            passed,
            errors,
            warnings,
        },
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::config::*;
    use std::collections::HashMap;

    /// Build a minimal valid ActiveConfig for testing.
    fn base_config() -> ActiveConfig {
        let mut agents = HashMap::new();
        agents.insert(
            "agent1".into(),
            AgentConfig {
                enabled: true,
                metadata: AgentMetadata::default(),
                capabilities: vec!["plan".into(), "implement".into()],
                command: "echo test".to_string(),
                selection: AgentSelectionConfig::default(),
                env: None,
                prompt_delivery: PromptDelivery::default(),
                health_policy: Default::default(),
            },
        );

        let mut workspaces = HashMap::new();
        // workspace root will be resolved relative to app_root in tests
        workspaces.insert(
            "default".into(),
            WorkspaceConfig {
                root_path: "ws".into(),
                qa_targets: vec!["docs/qa".into()],
                ticket_dir: "tickets".into(),
                self_referential: false,
                health_policy: Default::default(),
            },
        );

        let mut workflows = HashMap::new();
        workflows.insert(
            "test-wf".into(),
            WorkflowConfig {
                steps: vec![
                    WorkflowStepConfig {
                        id: "plan".into(),
                        description: None,
                        required_capability: Some("plan".into()),
                        execution_profile: None,
                        builtin: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        stagger_delay_ms: None,
                        timeout_secs: None,
                        stall_timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    WorkflowStepConfig {
                        id: "implement".into(),
                        description: None,
                        required_capability: Some("implement".into()),
                        execution_profile: None,
                        builtin: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                        max_parallel: None,
                        stagger_delay_ms: None,
                        timeout_secs: None,
                        stall_timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    WorkflowStepConfig {
                        id: "loop_guard".into(),
                        description: None,
                        required_capability: None,
                        execution_profile: None,
                        builtin: Some("loop_guard".into()),
                        enabled: true,
                        repeatable: true,
                        is_guard: true,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        template: None,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior {
                            execution: ExecutionMode::Builtin {
                                name: "loop_guard".into(),
                            },
                            ..StepBehavior::default()
                        },
                        max_parallel: None,
                        stagger_delay_ms: None,
                        timeout_secs: None,
                        stall_timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                execution: Default::default(),
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig { rules: vec![] },
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetyConfig::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                item_isolation: None,
            },
        );

        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(agent_orchestrator::config::DEFAULT_PROJECT_ID.to_string())
            .or_insert(agent_orchestrator::config::ProjectConfig {
                description: None,
                workspaces,
                agents,
                workflows,
                step_templates: HashMap::new(),
                env_stores: HashMap::new(),
                execution_profiles: HashMap::new(),
                triggers: HashMap::new(),
            });
        ActiveConfig {
            config,
            workspaces: HashMap::new(),
            projects: HashMap::new(),
        }
    }

    fn default_project_mut(
        cfg: &mut ActiveConfig,
    ) -> &mut agent_orchestrator::config::ProjectConfig {
        cfg.config
            .project_mut(None)
            .expect("default project should exist")
    }

    fn make_temp_ws(app_root: &Path) {
        std::fs::create_dir_all(app_root.join("ws/docs/qa")).expect("create temp workspace");
    }

    #[test]
    fn clean_config_no_errors() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        let app_root = tmp.path();
        make_temp_ws(app_root);

        let report = run_checks(&cfg, app_root, None, None);
        let errors: Vec<_> = report
            .checks
            .iter()
            .filter(|c| !c.passed && c.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "expected 0 errors, got: {errors:#?}");
        assert_eq!(report.summary.errors, 0);
    }

    #[test]
    fn workspace_root_missing() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        // Do NOT create ws dir
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "workspace_root_missing" && !c.passed);
        assert!(found, "expected workspace_root_missing error");
        assert!(report.summary.errors > 0);
    }

    #[test]
    fn qa_targets_missing() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        // Create ws root but not docs/qa
        std::fs::create_dir_all(tmp.path().join("ws")).expect("create ws dir");

        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "qa_targets_missing" && !c.passed);
        assert!(found, "expected qa_targets_missing warning");
        assert!(report.summary.warnings > 0);
    }

    #[test]
    fn capability_no_agent() {
        let mut cfg = base_config();
        // Add a step requiring a capability no agent has
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps
            .push(WorkflowStepConfig {
                id: "deploy".into(),
                description: None,
                required_capability: Some("deploy".into()),
                execution_profile: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            });

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed);
        assert!(found, "expected capability_no_agent error");
    }

    #[test]
    fn agent_missing_command() {
        let mut cfg = base_config();
        // Set agent command to empty string
        default_project_mut(&mut cfg)
            .agents
            .get_mut("agent1")
            .expect("agent1 should exist")
            .command = String::new();

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "agent_has_command" && !c.passed);
        assert!(found, "expected agent_has_command error");
    }

    #[test]
    fn builtin_unknown() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps
            .push(WorkflowStepConfig {
                id: "bad_builtin".into(),
                description: None,
                required_capability: None,
                execution_profile: None,
                builtin: Some("nonexistent".into()),
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            });

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "builtin_unknown" && !c.passed);
        assert!(found, "expected builtin_unknown error");
    }

    #[test]
    fn step_semantic_conflict() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps
            .push(WorkflowStepConfig {
                id: "conflict".into(),
                description: None,
                required_capability: Some("plan".into()),
                execution_profile: None,
                builtin: Some("self_test".into()),
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            });

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "step_semantic_conflict" && !c.passed);
        assert!(found, "expected step_semantic_conflict error");
    }

    #[test]
    fn execution_mode_mismatch() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps[0]
            .behavior
            .execution = ExecutionMode::Builtin {
            name: "plan".into(),
        };

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "execution_mode_mismatch" && !c.passed);
        assert!(found, "expected execution_mode_mismatch error");
    }

    #[test]
    fn command_steps_skip_capability_requirement() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps = vec![WorkflowStepConfig {
            id: "shell".into(),
            description: None,
            required_capability: None,
            execution_profile: None,
            builtin: None,
            enabled: true,
            repeatable: false,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: Some("echo ok".into()),
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior {
                execution: ExecutionMode::Builtin {
                    name: "shell".into(),
                },
                ..StepBehavior::default()
            },
            max_parallel: None,
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
        }];

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed);
        assert!(!found, "command step should not require agent capability");
    }

    #[test]
    fn pipe_to_unknown() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps[0]
            .pipe_to = Some("ghost".into());

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "pipe_to_unknown" && !c.passed);
        assert!(found, "expected pipe_to_unknown error");
    }

    #[test]
    fn template_unknown_var() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg).step_templates.insert(
            "plan".into(),
            StepTemplateConfig {
                prompt: "echo {unknown_var}".into(),
                description: None,
            },
        );

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "template_unknown_var" && !c.passed);
        assert!(found, "expected template_unknown_var warning");
    }

    #[test]
    fn template_system_var_ok() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        // {task_id} and {diff} are system vars — should not trigger warning
        let bad = report
            .checks
            .iter()
            .any(|c| c.rule == "template_unknown_var" && !c.passed);
        assert!(!bad, "system vars should not trigger unknown var warning");
    }

    #[test]
    fn template_pipeline_var_ok() {
        let mut cfg = base_config();
        // plan_output is derived from step "plan" → should not warn
        default_project_mut(&mut cfg).step_templates.insert(
            "implement".into(),
            StepTemplateConfig {
                prompt: "echo {plan_output}".into(),
                description: None,
            },
        );

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let bad = report.checks.iter().any(|c| {
            c.rule == "template_unknown_var" && !c.passed && c.message.contains("plan_output")
        });
        assert!(
            !bad,
            "pipeline-derived var {{plan_output}} should not trigger warning"
        );
    }

    #[test]
    fn empty_workflow() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg).workflows.insert(
            "empty-wf".into(),
            WorkflowConfig {
                steps: vec![WorkflowStepConfig {
                    id: "disabled".into(),
                    description: None,
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
                    enabled: false,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    stagger_delay_ms: None,
                    timeout_secs: None,
                    stall_timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                execution: Default::default(),
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig { rules: vec![] },
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                adaptive: None,
                safety: SafetyConfig::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                item_isolation: None,
            },
        );

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "empty_workflow" && !c.passed);
        assert!(found, "expected empty_workflow warning");
    }

    #[test]
    fn chain_steps_checked() {
        let mut cfg = base_config();
        // Add a step with a chain_step requiring unknown capability
        default_project_mut(&mut cfg)
            .workflows
            .get_mut("test-wf")
            .expect("test-wf should exist")
            .steps
            .push(WorkflowStepConfig {
                id: "parent".into(),
                description: None,
                required_capability: Some("plan".into()),
                execution_profile: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![WorkflowStepConfig {
                    id: "child".into(),
                    description: None,
                    required_capability: Some("deploy".into()),
                    execution_profile: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    stagger_delay_ms: None,
                    timeout_secs: None,
                    stall_timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            });

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed && c.message.contains("child"));
        assert!(found, "chain_step child should be checked for capability");
    }

    #[test]
    fn json_roundtrip() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);

        let json = serde_json::to_string(&report).expect("serialize");
        let _: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
    }

    #[test]
    fn prompt_delivery_stdin_warns_on_prompt_placeholder() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .agents
            .get_mut("agent1")
            .expect("agent1")
            .prompt_delivery = PromptDelivery::Stdin;
        default_project_mut(&mut cfg)
            .agents
            .get_mut("agent1")
            .expect("agent1")
            .command = "claude -p \"{prompt}\"".to_string();

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "prompt_delivery_placeholder_ignored" && !c.passed);
        assert!(found, "stdin delivery with {{prompt}} should warn");
    }

    #[test]
    fn prompt_delivery_file_warns_missing_prompt_file_placeholder() {
        let mut cfg = base_config();
        default_project_mut(&mut cfg)
            .agents
            .get_mut("agent1")
            .expect("agent1")
            .prompt_delivery = PromptDelivery::File;
        default_project_mut(&mut cfg)
            .agents
            .get_mut("agent1")
            .expect("agent1")
            .command = "claude --file input.txt".to_string();

        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "prompt_delivery_missing_placeholder" && !c.passed);
        assert!(found, "file delivery without {{prompt_file}} should warn");
    }

    #[test]
    fn prompt_delivery_arg_no_warning() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().expect("create temp dir");
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None, None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule.starts_with("prompt_delivery") && !c.passed);
        assert!(!found, "default arg delivery should not trigger warnings");
    }
}
