use crate::anomaly::Severity;
use crate::config::{
    default_scope_for_step_id, resolve_step_semantic_kind, CheckpointStrategy, StepScope,
    StepSemanticKind, WorkflowConfig, WorkflowSafetyProfile,
};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct PolicyDiagnostic {
    pub source: String,
    pub rule_id: String,
    pub severity: Severity,
    pub passed: bool,
    pub blocking: bool,
    pub message: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<String>,
}

impl PolicyDiagnostic {
    pub fn error(
        rule_id: impl Into<String>,
        message: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            source: "self_referential_policy".to_string(),
            rule_id: rule_id.into(),
            severity: Severity::Error,
            passed: false,
            blocking: true,
            message: message.into(),
            scope: scope.into(),
            actual: None,
            expected: None,
            risk: None,
            suggested_fix: None,
        }
    }

    pub fn warning(
        rule_id: impl Into<String>,
        message: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            source: "self_referential_policy".to_string(),
            rule_id: rule_id.into(),
            severity: Severity::Warning,
            passed: false,
            blocking: false,
            message: message.into(),
            scope: scope.into(),
            actual: None,
            expected: None,
            risk: None,
            suggested_fix: None,
        }
    }

    pub fn with_details(
        mut self,
        actual: impl Into<String>,
        expected: impl Into<String>,
        risk: impl Into<String>,
        suggested_fix: impl Into<String>,
    ) -> Self {
        self.actual = Some(actual.into());
        self.expected = Some(expected.into());
        self.risk = Some(risk.into());
        self.suggested_fix = Some(suggested_fix.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct PolicyEvaluation {
    pub diagnostics: Vec<PolicyDiagnostic>,
}

impl PolicyEvaluation {
    pub fn has_blocking_errors(&self) -> bool {
        self.diagnostics.iter().any(|diag| diag.blocking)
    }

    pub fn failing_diagnostics(&self) -> impl Iterator<Item = &PolicyDiagnostic> {
        self.diagnostics.iter().filter(|diag| !diag.passed)
    }
}

pub fn evaluate_self_referential_policy(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    workspace_id: &str,
    workspace_is_self_referential: bool,
) -> Result<PolicyEvaluation> {
    let scope = format!("workflow={workflow_id}, workspace={workspace_id}");
    let mut diagnostics = Vec::new();

    if workflow.safety.profile == WorkflowSafetyProfile::SelfReferentialProbe
        && !workspace_is_self_referential
    {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.probe_requires_self_referential_workspace",
                format!(
                    "workflow '{workflow_id}' uses self_referential_probe but workspace '{workspace_id}' is not self_referential"
                ),
                &scope,
            )
            .with_details(
                "workspace.self_referential=false",
                "workspace.self_referential=true",
                "probe workflows would run without the self-referential safety contract",
                "mark the target workspace as self_referential or use the standard safety profile",
            ),
        );
    }

    if !workspace_is_self_referential {
        return Ok(PolicyEvaluation { diagnostics });
    }

    if matches!(
        workflow.safety.checkpoint_strategy,
        CheckpointStrategy::None
    ) {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.checkpoint_strategy_required",
                format!(
                    "workflow '{workflow_id}' targets self_referential workspace '{workspace_id}' but safety.checkpoint_strategy is 'none'"
                ),
                &scope,
            )
            .with_details(
                "safety.checkpoint_strategy=none",
                "safety.checkpoint_strategy in {git_tag, git_stash}",
                "the orchestrator cannot create a recovery checkpoint before mutating its own source tree",
                "set safety.checkpoint_strategy to git_tag or git_stash",
            ),
        );
    }

    if !workflow.safety.auto_rollback {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.auto_rollback_required",
                format!(
                    "workflow '{workflow_id}' targets self_referential workspace '{workspace_id}' but safety.auto_rollback is disabled"
                ),
                &scope,
            )
            .with_details(
                "safety.auto_rollback=false",
                "safety.auto_rollback=true",
                "failed self-modification cycles may leave the orchestrator in a broken state without automatic recovery",
                "set safety.auto_rollback to true",
            ),
        );
    }

    if !has_enabled_self_test(workflow)? {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.self_test_required",
                format!(
                    "workflow '{workflow_id}' targets self_referential workspace '{workspace_id}' but has no enabled self_test step"
                ),
                &scope,
            )
            .with_details(
                "self_test step missing",
                "at least one enabled builtin self_test step",
                "the workflow can modify the orchestrator without compiling and testing the result before proceeding",
                "add an enabled builtin self_test step to the workflow",
            ),
        );
    }

    if !workflow.safety.binary_snapshot {
        diagnostics.push(
            PolicyDiagnostic::warning(
                "self_ref.binary_snapshot_recommended",
                format!(
                    "workflow '{workflow_id}' targets self_referential workspace '{workspace_id}' without safety.binary_snapshot"
                ),
                &scope,
            )
            .with_details(
                "safety.binary_snapshot=false",
                "safety.binary_snapshot=true",
                "binary rollback will rely only on checkpoints and may take longer to recover after a bad release build",
                "set safety.binary_snapshot to true for self-bootstrap workflows",
            ),
        );
    }

    if workflow.safety.profile == WorkflowSafetyProfile::SelfReferentialProbe {
        append_probe_diagnostics(workflow, workflow_id, &scope, &mut diagnostics)?;
    }

    Ok(PolicyEvaluation { diagnostics })
}

fn has_enabled_self_test(workflow: &WorkflowConfig) -> Result<bool> {
    for step in workflow.steps.iter().filter(|step| step.enabled) {
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if matches!(semantic, StepSemanticKind::Builtin { ref name } if name == "self_test") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn append_probe_diagnostics(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    scope: &str,
    diagnostics: &mut Vec<PolicyDiagnostic>,
) -> Result<()> {
    if !matches!(
        workflow.safety.checkpoint_strategy,
        CheckpointStrategy::GitTag
    ) {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.probe_requires_git_tag_checkpoint",
                format!(
                    "workflow '{workflow_id}' uses self_referential_probe but safety.checkpoint_strategy is not git_tag"
                ),
                scope,
            )
            .with_details(
                format!(
                    "safety.checkpoint_strategy={}",
                    checkpoint_strategy_label(&workflow.safety.checkpoint_strategy)
                ),
                "safety.checkpoint_strategy=git_tag",
                "probe workflows need a stable git-tagged checkpoint for deterministic rollback during strict validation runs",
                "set safety.checkpoint_strategy to git_tag",
            ),
        );
    }

    if !matches!(workflow.loop_policy.mode, crate::config::LoopMode::Once) {
        diagnostics.push(
            PolicyDiagnostic::error(
                "self_ref.probe_requires_loop_mode_once",
                format!(
                    "workflow '{workflow_id}' uses self_referential_probe but loop.mode is not once"
                ),
                scope,
            )
            .with_details(
                    format!("loop.mode={:?}", workflow.loop_policy.mode),
                "loop.mode=once",
                "probe workflows are intended to be single-pass validation probes, not iterative self-bootstrap loops",
                "set loop.mode to once",
            ),
        );
    }

    for step in workflow.steps.iter().filter(|step| step.enabled) {
        let step_scope = step
            .scope
            .unwrap_or_else(|| default_scope_for_step_id(&step.id));
        if step_scope != StepScope::Task {
            diagnostics.push(
                PolicyDiagnostic::error(
                    "self_ref.probe_task_scope_only",
                    format!(
                        "workflow '{workflow_id}' uses self_referential_probe but step '{}' is not task-scoped",
                        step.id
                    ),
                    scope,
                )
                .with_details(
                    format!("step.scope={step_scope:?}"),
                    "step.scope=task",
                    "probe workflows must stay single-pass and avoid per-item fan-out behavior",
                    format!("set step '{}' scope to task", step.id),
                ),
            );
        }

        if !step.chain_steps.is_empty() {
            diagnostics.push(
                PolicyDiagnostic::error(
                    "self_ref.probe_no_chain_steps",
                    format!(
                        "workflow '{workflow_id}' uses self_referential_probe but step '{}' defines chain_steps",
                        step.id
                    ),
                    scope,
                )
                .with_details(
                    "chain_steps present",
                    "no chain_steps",
                    "probe workflows should remain linear and auditable",
                    format!("remove chain_steps from step '{}'", step.id),
                ),
            );
        }

        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        let allows_semantic = matches!(semantic, StepSemanticKind::Command)
            || matches!(semantic, StepSemanticKind::Builtin { ref name } if name == "self_test");
        if !allows_semantic {
            diagnostics.push(
                PolicyDiagnostic::error(
                    "self_ref.probe_command_steps_only",
                    format!(
                        "workflow '{workflow_id}' uses self_referential_probe but step '{}' is not a self-contained command or self_test builtin",
                        step.id
                    ),
                    scope,
                )
                .with_details(
                    format!("step.semantic={}", semantic_label(&semantic)),
                    "step.semantic in {command, builtin:self_test}",
                    "probe workflows should be self-contained and deterministic aside from the mandatory self_test gate",
                    format!("convert step '{}' into a command step or keep only the builtin self_test", step.id),
                ),
            );
        }

        if matches!(
            step.id.as_str(),
            "qa" | "qa_testing"
                | "fix"
                | "ticket_fix"
                | "retest"
                | "guard"
                | "build"
                | "test"
                | "lint"
                | "smoke_chain"
                | "ticket_scan"
                | "init_once"
                | "loop_guard"
        ) {
            diagnostics.push(
                PolicyDiagnostic::error(
                    "self_ref.probe_forbidden_phase",
                    format!(
                        "workflow '{workflow_id}' uses self_referential_probe but step '{}' is a forbidden phase",
                        step.id
                    ),
                    scope,
                )
                .with_details(
                    format!("step.id={}", step.id),
                    "probe-only command steps plus builtin self_test",
                    "strict phases and orchestration builtins broaden probe behavior beyond a narrow validation run",
                    format!("remove or rename step '{}' for probe workflows", step.id),
                ),
            );
        }
    }

    Ok(())
}

pub fn format_blocking_policy_error(evaluation: &PolicyEvaluation) -> String {
    let mut rendered = String::from(
        "[SELF_REF_POLICY_VIOLATION] self-referential safety policy rejected the workflow:",
    );
    for diagnostic in evaluation.diagnostics.iter().filter(|diag| diag.blocking) {
        rendered.push_str(&format!(
            "\n- {}: {}",
            diagnostic.rule_id, diagnostic.message
        ));
        if let Some(actual) = diagnostic.actual.as_deref() {
            rendered.push_str(&format!("\n  actual: {actual}"));
        }
        if let Some(expected) = diagnostic.expected.as_deref() {
            rendered.push_str(&format!("\n  expected: {expected}"));
        }
        if let Some(risk) = diagnostic.risk.as_deref() {
            rendered.push_str(&format!("\n  risk: {risk}"));
        }
        if let Some(fix) = diagnostic.suggested_fix.as_deref() {
            rendered.push_str(&format!("\n  suggested_fix: {fix}"));
        }
    }
    rendered
}

fn semantic_label(semantic: &StepSemanticKind) -> String {
    match semantic {
        StepSemanticKind::Builtin { name } => format!("builtin:{name}"),
        StepSemanticKind::Agent { capability } => format!("agent:{capability}"),
        StepSemanticKind::Command => "command".to_string(),
        StepSemanticKind::Chain => "chain".to_string(),
    }
}

fn checkpoint_strategy_label(strategy: &CheckpointStrategy) -> &'static str {
    match strategy {
        CheckpointStrategy::None => "none",
        CheckpointStrategy::GitTag => "git_tag",
        CheckpointStrategy::GitStash => "git_stash",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        LoopMode, SafetyConfig, StepBehavior, WorkflowFinalizeConfig, WorkflowLoopConfig,
        WorkflowLoopGuardConfig, WorkflowStepConfig,
    };

    fn command_step(id: &str) -> WorkflowStepConfig {
        WorkflowStepConfig {
            id: id.to_string(),
            description: None,
            builtin: None,
            required_capability: None,
            execution_profile: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: Some("echo ok".to_string()),
            chain_steps: vec![],
            scope: Some(StepScope::Task),
            behavior: StepBehavior::default(),
            max_parallel: None,
            timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
        }
    }

    fn builtin_self_test() -> WorkflowStepConfig {
        WorkflowStepConfig {
            id: "self_test".to_string(),
            builtin: Some("self_test".to_string()),
            required_capability: None,
            command: None,
            scope: Some(StepScope::Task),
            ..command_step("self_test")
        }
    }

    fn workflow() -> WorkflowConfig {
        WorkflowConfig {
            steps: vec![command_step("implement"), builtin_self_test()],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig::default(),
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: SafetyConfig {
                checkpoint_strategy: CheckpointStrategy::GitTag,
                auto_rollback: true,
                binary_snapshot: true,
                profile: WorkflowSafetyProfile::Standard,
                ..SafetyConfig::default()
            },
            max_parallel: None,
        }
    }

    #[test]
    fn non_self_referential_workspace_skips_standard_rules() {
        let report =
            evaluate_self_referential_policy(&workflow(), "wf", "ws", false).expect("policy");
        assert!(!report.has_blocking_errors());
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn self_referential_workspace_requires_self_test() {
        let mut wf = workflow();
        wf.steps.retain(|step| step.id != "self_test");
        let report = evaluate_self_referential_policy(&wf, "wf", "ws", true).expect("policy");
        assert!(report.has_blocking_errors());
        assert!(report
            .diagnostics
            .iter()
            .any(|diag| diag.rule_id == "self_ref.self_test_required"));
    }

    #[test]
    fn probe_profile_allows_builtin_self_test() {
        let mut wf = workflow();
        wf.safety.profile = WorkflowSafetyProfile::SelfReferentialProbe;
        let report = evaluate_self_referential_policy(&wf, "wf", "ws", true).expect("policy");
        assert!(!report.has_blocking_errors());
    }
}
