use crate::config::{
    is_known_builtin_step_name, normalize_step_execution_mode, OrchestratorConfig, WorkflowConfig,
    WorkflowStepConfig,
};
use anyhow::Result;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
/// Enumerates automatic config rewrite rules applied during self-healing.
pub enum ConfigSelfHealRule {
    /// Removes deprecated `required_capability` from builtin steps.
    DropRequiredCapabilityFromBuiltinStep,
    /// Aligns `behavior.execution` with the effective step shape.
    NormalizeStepExecutionMode,
}

impl ConfigSelfHealRule {
    /// Returns the stable label used in heal logs and API responses.
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::DropRequiredCapabilityFromBuiltinStep => "DropRequiredCapabilityFromBuiltinStep",
            Self::NormalizeStepExecutionMode => "NormalizeStepExecutionMode",
        }
    }
}

impl fmt::Display for ConfigSelfHealRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Records one mutation applied by the config self-heal pass.
pub struct ConfigSelfHealChange {
    /// Fully scoped workflow identifier that contained the healed step.
    pub workflow_id: String,
    /// Step identifier affected by the rewrite.
    pub step_id: String,
    /// Rewrite rule that produced this change.
    pub rule: ConfigSelfHealRule,
    /// Human-readable explanation of the rewrite.
    pub detail: String,
}

#[derive(Debug, Clone)]
/// Summary returned when config loading succeeds after an automatic self-heal pass.
pub struct ConfigSelfHealReport {
    /// Original validation error that triggered the self-heal attempt.
    pub original_error: String,
    /// Version assigned to the healed config snapshot.
    pub healed_version: i64,
    /// Timestamp when the healed snapshot was persisted.
    pub healed_at: String,
    /// Individual changes applied during the self-heal pass.
    pub changes: Vec<ConfigSelfHealChange>,
}

pub(crate) fn apply_self_heal_to_step(
    workflow_id: &str,
    step: &mut WorkflowStepConfig,
    changes: &mut Vec<ConfigSelfHealChange>,
) -> Result<()> {
    if let Some(builtin) = step.builtin.as_deref() {
        if is_known_builtin_step_name(builtin) {
            if let Some(required_capability) = step.required_capability.take() {
                changes.push(ConfigSelfHealChange {
                    workflow_id: workflow_id.to_string(),
                    step_id: step.id.clone(),
                    rule: ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep,
                    detail: format!(
                        "removed deprecated required_capability '{}' from builtin '{}'",
                        required_capability, builtin
                    ),
                });
            }
        }
    }

    let previous_execution = step.behavior.execution.clone();
    normalize_step_execution_mode(step).map_err(anyhow::Error::msg)?;
    if step.behavior.execution != previous_execution {
        changes.push(ConfigSelfHealChange {
            workflow_id: workflow_id.to_string(),
            step_id: step.id.clone(),
            rule: ConfigSelfHealRule::NormalizeStepExecutionMode,
            detail: format!(
                "normalized behavior.execution from {:?} to {:?}",
                previous_execution, step.behavior.execution
            ),
        });
    }

    for chain_step in &mut step.chain_steps {
        apply_self_heal_to_step(workflow_id, chain_step, changes)?;
    }

    Ok(())
}

pub(crate) fn apply_self_heal_to_workflow(
    workflow_id: &str,
    workflow: &mut WorkflowConfig,
    changes: &mut Vec<ConfigSelfHealChange>,
) -> Result<()> {
    for step in &mut workflow.steps {
        apply_self_heal_to_step(workflow_id, step, changes)?;
    }
    Ok(())
}

pub(crate) fn apply_self_heal_pass(
    config: &OrchestratorConfig,
) -> Result<Option<(OrchestratorConfig, Vec<ConfigSelfHealChange>)>> {
    let mut healed = config.clone();
    let mut changes = Vec::new();

    for (project_id, project) in &mut healed.projects {
        for (workflow_id, workflow) in &mut project.workflows {
            let scoped_workflow_id = format!("{project_id}/{workflow_id}");
            apply_self_heal_to_workflow(&scoped_workflow_id, workflow, &mut changes)?;
        }
    }

    if changes.is_empty() {
        Ok(None)
    } else {
        Ok(Some((healed, changes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExecutionMode;
    use crate::config_load::tests::make_minimal_buildable_config;

    #[test]
    fn self_heal_drops_required_capability_from_known_builtin_step() {
        let mut config = make_minimal_buildable_config();
        let workflow = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get_mut("basic")
            .expect("missing basic workflow");
        let step = workflow
            .steps
            .first_mut()
            .expect("missing builtin self_test step");
        step.required_capability = Some("self_test".to_string());

        let healed = apply_self_heal_pass(&config)
            .expect("self-heal pass should run")
            .expect("expected a self-heal change");

        let healed_step = healed
            .0
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get("basic")
            .and_then(|wf| wf.steps.first())
            .expect("missing healed step");
        assert!(healed_step.required_capability.is_none());
        assert!(
            healed
                .1
                .iter()
                .any(|change| change.rule
                    == ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep)
        );
    }

    #[test]
    fn self_heal_normalizes_execution_mode_mismatch() {
        let mut config = make_minimal_buildable_config();
        let workflow = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get_mut("basic")
            .expect("missing basic workflow");
        let step = workflow
            .steps
            .first_mut()
            .expect("missing builtin self_test step");
        step.behavior.execution = ExecutionMode::Agent;

        let healed = apply_self_heal_pass(&config)
            .expect("self-heal pass should run")
            .expect("expected a normalization change");

        let healed_step = healed
            .0
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .get("basic")
            .and_then(|wf| wf.steps.first())
            .expect("missing healed step");
        assert_eq!(
            healed_step.behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
        assert!(healed
            .1
            .iter()
            .any(|change| change.rule == ConfigSelfHealRule::NormalizeStepExecutionMode));
    }

    #[test]
    fn self_heal_returns_none_when_config_is_canonical() {
        let config = make_minimal_buildable_config();

        let healed = apply_self_heal_pass(&config).expect("self-heal pass should run");

        assert!(healed.is_none(), "canonical config should not be rewritten");
    }

    #[test]
    fn self_heal_does_not_mutate_project_scoped_validation_errors() {
        let mut config = make_minimal_buildable_config();
        config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project should exist")
            .workspaces
            .clear();

        let healed = apply_self_heal_pass(&config).expect("self-heal pass should run");

        assert!(
            healed.is_none(),
            "missing workspace is not a healable drift"
        );
    }

    #[test]
    fn config_self_heal_rule_display_returns_label() {
        assert_eq!(
            ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep.to_string(),
            "DropRequiredCapabilityFromBuiltinStep"
        );
        assert_eq!(
            ConfigSelfHealRule::NormalizeStepExecutionMode.to_string(),
            "NormalizeStepExecutionMode"
        );
    }

    #[test]
    fn config_self_heal_rule_as_label_matches_display() {
        for rule in [
            ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep,
            ConfigSelfHealRule::NormalizeStepExecutionMode,
        ] {
            assert_eq!(rule.as_label(), rule.to_string());
        }
    }

    #[test]
    fn config_self_heal_rule_serializes_as_string() {
        let json =
            serde_json::to_string(&ConfigSelfHealRule::DropRequiredCapabilityFromBuiltinStep)
                .expect("self-heal rule should serialize");
        assert!(json.contains("DropRequiredCapabilityFromBuiltinStep"));
    }
}
