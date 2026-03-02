//! Preflight cross-reference validation for orchestrator configuration.
//!
//! Pure logic layer — no DB, no async. The CLI handler loads the config and
//! calls [`run_checks`], then renders the resulting [`CheckReport`].

use crate::config::{
    is_known_builtin_step_name, resolve_step_semantic_kind, ActiveConfig, ExecutionMode,
    OrchestratorConfig, StepSemanticKind, WorkflowStepConfig,
};
use crate::anomaly::Severity;
use crate::scheduler::trace::find_template_vars;
use serde::Serialize;
use std::collections::HashSet;
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

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub checks: Vec<CheckResult>,
    pub summary: CheckSummary,
}

#[derive(Debug, Serialize, Clone)]
pub struct CheckResult {
    pub rule: String,
    pub severity: Severity,
    pub passed: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckSummary {
    pub total: u32,
    pub passed: u32,
    pub errors: u32,
    pub warnings: u32,
}

// ── Entry point ─────────────────────────────────────────────────────

/// Run all preflight checks against the loaded configuration.
/// `workflow_filter`: if Some, only check steps in that workflow.
pub fn run_checks(
    config: &ActiveConfig,
    app_root: &Path,
    workflow_filter: Option<&str>,
) -> CheckReport {
    let oc = &config.config;
    let mut checks = Vec::new();

    check_workspace_roots(oc, app_root, &mut checks);
    check_qa_targets(oc, app_root, &mut checks);
    check_capability_coverage(oc, workflow_filter, &mut checks);
    check_capability_templates(oc, &mut checks);
    check_builtin_names(oc, workflow_filter, &mut checks);
    check_pipe_to_refs(oc, workflow_filter, &mut checks);
    check_template_vars(oc, workflow_filter, &mut checks);
    check_empty_workflows(oc, workflow_filter, &mut checks);

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

// ── Individual checks ───────────────────────────────────────────────

fn check_workspace_roots(oc: &OrchestratorConfig, app_root: &Path, out: &mut Vec<CheckResult>) {
    for (ws_id, ws) in &oc.workspaces {
        let full = app_root.join(&ws.root_path);
        let exists = full.exists();
        out.push(CheckResult {
            rule: "workspace_root_missing".into(),
            severity: Severity::Error,
            passed: exists,
            message: if exists {
                format!("workspace \"{ws_id}\": root path exists")
            } else {
                format!(
                    "workspace \"{ws_id}\": root path \"{}\" does not exist",
                    ws.root_path
                )
            },
            context: Some(full.display().to_string()),
        });
    }
}

fn check_qa_targets(oc: &OrchestratorConfig, app_root: &Path, out: &mut Vec<CheckResult>) {
    for (ws_id, ws) in &oc.workspaces {
        let ws_root = app_root.join(&ws.root_path);
        for target in &ws.qa_targets {
            let full = ws_root.join(target);
            let exists = full.exists();
            out.push(CheckResult {
                rule: "qa_targets_missing".into(),
                severity: Severity::Warning,
                passed: exists,
                message: if exists {
                    format!("workspace \"{ws_id}\": qa_target \"{target}\" exists")
                } else {
                    format!("workspace \"{ws_id}\": qa_target \"{target}\" does not exist")
                },
                context: Some(full.display().to_string()),
            });
        }
    }
}

fn check_capability_coverage(
    oc: &OrchestratorConfig,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let all_caps: HashSet<&str> = oc
        .agents
        .values()
        .flat_map(|a| a.capabilities.iter().map(|s| s.as_str()))
        .collect();

    for (wf_id, wf) in &oc.workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        check_steps_capability(&wf.steps, wf_id, &all_caps, out);
    }
}

fn check_steps_capability(
    steps: &[WorkflowStepConfig],
    wf_id: &str,
    all_caps: &HashSet<&str>,
    out: &mut Vec<CheckResult>,
) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if let Ok(StepSemanticKind::Agent { capability }) = resolve_step_semantic_kind(step) {
            let cap = capability;
            let covered = all_caps.contains(cap.as_str());
            out.push(CheckResult {
                rule: "capability_no_agent".into(),
                severity: Severity::Error,
                passed: covered,
                message: if covered {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": capability \"{}\" is provided",
                        step.id, cap
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": requires capability \"{}\" but no agent provides it",
                        step.id, cap
                    )
                },
                context: None,
            });
        }
        // Recurse into chain_steps
        if !step.chain_steps.is_empty() {
            check_steps_capability(&step.chain_steps, wf_id, all_caps, out);
        }
    }
}

fn check_capability_templates(oc: &OrchestratorConfig, out: &mut Vec<CheckResult>) {
    for (agent_id, agent) in &oc.agents {
        for cap in &agent.capabilities {
            let has_template = agent.templates.contains_key(cap);
            out.push(CheckResult {
                rule: "capability_no_template".into(),
                severity: Severity::Error,
                passed: has_template,
                message: if has_template {
                    format!("agent \"{agent_id}\": has template for capability \"{cap}\"")
                } else {
                    format!(
                        "agent \"{agent_id}\": declares capability \"{cap}\" but has no \"{cap}\" template"
                    )
                },
                context: None,
            });
        }
    }
}

fn check_builtin_names(
    oc: &OrchestratorConfig,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in &oc.workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        check_steps_builtin(&wf.steps, wf_id, out);
    }
}

fn check_steps_builtin(steps: &[WorkflowStepConfig], wf_id: &str, out: &mut Vec<CheckResult>) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if step.builtin.is_some() && step.required_capability.is_some() {
            out.push(CheckResult {
                rule: "step_semantic_conflict".into(),
                severity: Severity::Error,
                passed: false,
                message: format!(
                    "workflow \"{wf_id}\" step \"{}\": cannot define both builtin and required_capability",
                    step.id
                ),
                context: None,
            });
        }
        if let Some(ref builtin) = step.builtin {
            let known = is_known_builtin_step_name(builtin);
            out.push(CheckResult {
                rule: "builtin_unknown".into(),
                severity: Severity::Error,
                passed: known,
                message: if known {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": builtin \"{builtin}\" is known",
                        step.id
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": builtin \"{builtin}\" is not a known builtin",
                        step.id
                    )
                },
                context: Some(
                    "known builtins: [\"init_once\", \"loop_guard\", \"ticket_scan\", \"self_test\"]"
                        .to_string(),
                ),
            });
        }
        match resolve_step_semantic_kind(step) {
            Ok(semantic) => {
                let matches_execution = match semantic {
                    StepSemanticKind::Builtin { ref name } => {
                        step.behavior.execution == ExecutionMode::Builtin { name: name.clone() }
                    }
                    StepSemanticKind::Agent { .. } => {
                        step.behavior.execution == ExecutionMode::Agent
                    }
                    StepSemanticKind::Command => {
                        step.behavior.execution
                            == ExecutionMode::Builtin {
                                name: step.id.clone(),
                            }
                    }
                    StepSemanticKind::Chain => step.behavior.execution == ExecutionMode::Chain,
                };
                out.push(CheckResult {
                    rule: "execution_mode_mismatch".into(),
                    severity: Severity::Error,
                    passed: matches_execution,
                    message: if matches_execution {
                        format!(
                            "workflow \"{wf_id}\" step \"{}\": execution mode matches semantic meaning",
                            step.id
                        )
                    } else {
                        format!(
                            "workflow \"{wf_id}\" step \"{}\": execution mode does not match builtin/capability semantics",
                            step.id
                        )
                    },
                    context: None,
                });
            }
            Err(err) => out.push(CheckResult {
                rule: "step_semantic_invalid".into(),
                severity: Severity::Error,
                passed: false,
                message: format!("workflow \"{wf_id}\" step \"{}\": {err}", step.id),
                context: None,
            }),
        }
        if !step.chain_steps.is_empty() {
            check_steps_builtin(&step.chain_steps, wf_id, out);
        }
    }
}

fn check_pipe_to_refs(
    oc: &OrchestratorConfig,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in &oc.workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let step_ids: HashSet<&str> = collect_step_ids(&wf.steps);
        check_steps_pipe_to(&wf.steps, wf_id, &step_ids, out);
    }
}

fn collect_step_ids(steps: &[WorkflowStepConfig]) -> HashSet<&str> {
    let mut ids = HashSet::new();
    for step in steps {
        ids.insert(step.id.as_str());
        for child in &step.chain_steps {
            ids.insert(child.id.as_str());
        }
    }
    ids
}

fn check_steps_pipe_to(
    steps: &[WorkflowStepConfig],
    wf_id: &str,
    step_ids: &HashSet<&str>,
    out: &mut Vec<CheckResult>,
) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if let Some(ref target) = step.pipe_to {
            let known = step_ids.contains(target.as_str());
            out.push(CheckResult {
                rule: "pipe_to_unknown".into(),
                severity: Severity::Error,
                passed: known,
                message: if known {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": pipe_to \"{target}\" exists",
                        step.id
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": pipe_to \"{target}\" is not a step in this workflow",
                        step.id
                    )
                },
                context: None,
            });
        }
        if !step.chain_steps.is_empty() {
            check_steps_pipe_to(&step.chain_steps, wf_id, step_ids, out);
        }
    }
}

fn check_template_vars(
    oc: &OrchestratorConfig,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let sys_vars: HashSet<&str> = KNOWN_SYSTEM_VARS.iter().copied().collect();

    // Collect pipeline-derived vars per workflow
    for (wf_id, wf) in &oc.workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let mut pipeline_vars: HashSet<String> = HashSet::new();
        for step in &wf.steps {
            pipeline_vars.insert(format!("{}_output", step.id));
            pipeline_vars.insert(format!("{}_output_path", step.id));
            for child in &step.chain_steps {
                pipeline_vars.insert(format!("{}_output", child.id));
                pipeline_vars.insert(format!("{}_output_path", child.id));
            }
        }

        // Check each agent template for unknown vars
        for (agent_id, agent) in &oc.agents {
            for (tmpl_name, tmpl_body) in &agent.templates {
                for var_with_braces in find_template_vars(tmpl_body) {
                    // strip braces: {foo} -> foo
                    let var = &var_with_braces[1..var_with_braces.len() - 1];
                    if sys_vars.contains(var) {
                        continue;
                    }
                    if pipeline_vars.contains(var) {
                        continue;
                    }
                    out.push(CheckResult {
                        rule: "template_unknown_var".into(),
                        severity: Severity::Warning,
                        passed: false,
                        message: format!(
                            "agent \"{agent_id}\": template \"{tmpl_name}\" references {var_with_braces} \
                             — not a known system variable (may come from pipeline)"
                        ),
                        context: None,
                    });
                }
            }
        }
    }
}

fn check_empty_workflows(
    oc: &OrchestratorConfig,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in &oc.workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let enabled_count = wf.steps.iter().filter(|s| s.enabled).count();
        let is_empty = enabled_count == 0;
        out.push(CheckResult {
            rule: "empty_workflow".into(),
            severity: Severity::Warning,
            passed: !is_empty,
            message: if is_empty {
                format!("workflow \"{wf_id}\": has 0 enabled steps")
            } else {
                format!("workflow \"{wf_id}\": has {enabled_count} enabled steps")
            },
            context: None,
        });
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use std::collections::HashMap;

    /// Build a minimal valid ActiveConfig for testing.
    fn base_config() -> ActiveConfig {
        let mut agents = HashMap::new();
        agents.insert(
            "agent1".into(),
            AgentConfig {
                metadata: AgentMetadata::default(),
                capabilities: vec!["plan".into(), "implement".into()],
                templates: {
                    let mut t = HashMap::new();
                    t.insert("plan".into(), "echo plan {task_id}".into());
                    t.insert("implement".into(), "echo impl {diff}".into());
                    t
                },
                selection: AgentSelectionConfig::default(),
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
                        builtin: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                    },
                    WorkflowStepConfig {
                        id: "implement".into(),
                        description: None,
                        required_capability: Some("implement".into()),
                        builtin: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: None,
                        behavior: StepBehavior::default(),
                    },
                    WorkflowStepConfig {
                        id: "loop_guard".into(),
                        description: None,
                        required_capability: None,
                        builtin: Some("loop_guard".into()),
                        enabled: true,
                        repeatable: true,
                        is_guard: true,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
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
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig { rules: vec![] },
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                safety: SafetyConfig::default(),
            },
        );

        ActiveConfig {
            config: OrchestratorConfig {
                workspaces,
                agents,
                workflows,
                ..OrchestratorConfig::default()
            },
            workspaces: HashMap::new(),
            projects: HashMap::new(),
            default_project_id: String::new(),
            default_workspace_id: "default".into(),
            default_workflow_id: "test-wf".into(),
        }
    }

    fn make_temp_ws(app_root: &Path) {
        std::fs::create_dir_all(app_root.join("ws/docs/qa")).unwrap();
    }

    #[test]
    fn clean_config_no_errors() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().unwrap();
        let app_root = tmp.path();
        make_temp_ws(app_root);

        let report = run_checks(&cfg, app_root, None);
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
        let tmp = tempfile::tempdir().unwrap();
        // Do NOT create ws dir
        let report = run_checks(&cfg, tmp.path(), None);
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
        let tmp = tempfile::tempdir().unwrap();
        // Create ws root but not docs/qa
        std::fs::create_dir_all(tmp.path().join("ws")).unwrap();

        let report = run_checks(&cfg, tmp.path(), None);
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
        cfg.config
            .workflows
            .get_mut("test-wf")
            .unwrap()
            .steps
            .push(WorkflowStepConfig {
                id: "deploy".into(),
                description: None,
                required_capability: Some("deploy".into()),
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            });

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed);
        assert!(found, "expected capability_no_agent error");
    }

    #[test]
    fn capability_no_template() {
        let mut cfg = base_config();
        // Add a capability without a template
        cfg.config
            .agents
            .get_mut("agent1")
            .unwrap()
            .capabilities
            .push("qa".into());

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_template" && !c.passed);
        assert!(found, "expected capability_no_template error");
    }

    #[test]
    fn builtin_unknown() {
        let mut cfg = base_config();
        cfg.config
            .workflows
            .get_mut("test-wf")
            .unwrap()
            .steps
            .push(WorkflowStepConfig {
                id: "bad_builtin".into(),
                description: None,
                required_capability: None,
                builtin: Some("nonexistent".into()),
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            });

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "builtin_unknown" && !c.passed);
        assert!(found, "expected builtin_unknown error");
    }

    #[test]
    fn step_semantic_conflict() {
        let mut cfg = base_config();
        cfg.config
            .workflows
            .get_mut("test-wf")
            .unwrap()
            .steps
            .push(WorkflowStepConfig {
                id: "conflict".into(),
                description: None,
                required_capability: Some("plan".into()),
                builtin: Some("self_test".into()),
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            });

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "step_semantic_conflict" && !c.passed);
        assert!(found, "expected step_semantic_conflict error");
    }

    #[test]
    fn execution_mode_mismatch() {
        let mut cfg = base_config();
        cfg.config.workflows.get_mut("test-wf").unwrap().steps[0]
            .behavior
            .execution = ExecutionMode::Builtin {
            name: "plan".into(),
        };

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "execution_mode_mismatch" && !c.passed);
        assert!(found, "expected execution_mode_mismatch error");
    }

    #[test]
    fn command_steps_skip_capability_requirement() {
        let mut cfg = base_config();
        cfg.config.workflows.get_mut("test-wf").unwrap().steps = vec![WorkflowStepConfig {
            id: "shell".into(),
            description: None,
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: false,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
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
        }];

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed);
        assert!(!found, "command step should not require agent capability");
    }

    #[test]
    fn pipe_to_unknown() {
        let mut cfg = base_config();
        cfg.config.workflows.get_mut("test-wf").unwrap().steps[0].pipe_to = Some("ghost".into());

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "pipe_to_unknown" && !c.passed);
        assert!(found, "expected pipe_to_unknown error");
    }

    #[test]
    fn template_unknown_var() {
        let mut cfg = base_config();
        cfg.config
            .agents
            .get_mut("agent1")
            .unwrap()
            .templates
            .insert("plan".into(), "echo {unknown_var}".into());

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "template_unknown_var" && !c.passed);
        assert!(found, "expected template_unknown_var warning");
    }

    #[test]
    fn template_system_var_ok() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
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
        cfg.config
            .agents
            .get_mut("agent1")
            .unwrap()
            .templates
            .insert("implement".into(), "echo {plan_output}".into());

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
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
        cfg.config.workflows.insert(
            "empty-wf".into(),
            WorkflowConfig {
                steps: vec![WorkflowStepConfig {
                    id: "disabled".into(),
                    description: None,
                    required_capability: None,
                    builtin: None,
                    enabled: false,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                }],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig { rules: vec![] },
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                safety: SafetyConfig::default(),
            },
        );

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
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
        cfg.config
            .workflows
            .get_mut("test-wf")
            .unwrap()
            .steps
            .push(WorkflowStepConfig {
                id: "parent".into(),
                description: None,
                required_capability: Some("plan".into()),
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![WorkflowStepConfig {
                    id: "child".into(),
                    description: None,
                    required_capability: Some("deploy".into()),
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                }],
                scope: None,
                behavior: StepBehavior::default(),
            });

        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);
        let found = report
            .checks
            .iter()
            .any(|c| c.rule == "capability_no_agent" && !c.passed && c.message.contains("child"));
        assert!(found, "chain_step child should be checked for capability");
    }

    #[test]
    fn json_roundtrip() {
        let cfg = base_config();
        let tmp = tempfile::tempdir().unwrap();
        make_temp_ws(tmp.path());
        let report = run_checks(&cfg, tmp.path(), None);

        let json = serde_json::to_string(&report).expect("serialize");
        let _: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
    }
}
