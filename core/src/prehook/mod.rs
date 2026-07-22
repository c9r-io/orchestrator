mod cel;
mod context;
/// Finalize-rule and prehook evaluation helpers.
pub mod finalize;

#[cfg(test)]
mod tests;

use crate::config::{StepHookEngine, StepPrehookConfig, WorkflowFinalizeRule};
use anyhow::Result;
use cel_interpreter::Program;

// Public API re-exports
pub use cel::{
    evaluate_convergence_expression, evaluate_finalize_rule_expression,
    evaluate_step_prehook_expression, evaluate_webhook_filter,
};
pub use finalize::{
    emit_item_finalize_event, emit_step_prehook_event, evaluate_step_prehook,
    resolve_workflow_finalize_outcome,
};

/// Validates all command rules on an agent: CEL syntax and `{prompt}` placeholder.
pub fn validate_agent_command_rules(
    agent_id: &str,
    rules: &[crate::config::AgentCommandRule],
) -> Result<()> {
    for (i, rule) in rules.iter().enumerate() {
        let expression = rule.when.trim();
        if expression.is_empty() {
            anyhow::bail!("agent '{agent_id}' command_rules[{i}].when cannot be empty");
        }
        let compiled = std::panic::catch_unwind(|| Program::compile(expression)).map_err(|_| {
            anyhow::anyhow!("agent '{agent_id}' command_rules[{i}].when caused CEL parser panic")
        })?;
        compiled.map_err(|err| {
            anyhow::anyhow!("agent '{agent_id}' command_rules[{i}].when is invalid CEL: {err}")
        })?;
        if !rule.command.contains("{prompt}") {
            anyhow::bail!(
                "agent '{agent_id}' command_rules[{i}].command must contain {{prompt}} placeholder"
            );
        }
    }
    Ok(())
}

/// Validates one step prehook expression and its engine configuration.
pub fn validate_step_prehook(
    prehook: &StepPrehookConfig,
    workflow_id: &str,
    step_type: &str,
) -> Result<()> {
    let expression = prehook.when.trim();
    if expression.is_empty() {
        anyhow::bail!("workflow '{workflow_id}' step '{step_type}' prehook.when cannot be empty");
    }
    match prehook.engine {
        StepHookEngine::Cel => {
            let compiled =
                std::panic::catch_unwind(|| Program::compile(expression)).map_err(|_| {
                    anyhow::anyhow!(
                        "workflow '{workflow_id}' step '{step_type}' prehook.when caused CEL parser panic"
                    )
                })?;
            compiled.map_err(|err| {
                anyhow::anyhow!(
                    "workflow '{workflow_id}' step '{step_type}' prehook.when is invalid CEL: {err}"
                )
            })?;
        }
    }
    Ok(())
}

/// Validates one workflow finalize rule expression and metadata.
pub fn validate_workflow_finalize_rule(
    rule: &WorkflowFinalizeRule,
    workflow_id: &str,
) -> Result<()> {
    if rule.id.trim().is_empty() {
        anyhow::bail!("workflow '{workflow_id}' has finalize rule with empty id");
    }
    if rule.status.trim().is_empty() {
        anyhow::bail!(
            "workflow '{}' finalize rule '{}' has empty status",
            workflow_id,
            rule.id
        );
    }
    let expression = rule.when.trim();
    if expression.is_empty() {
        anyhow::bail!(
            "workflow '{}' finalize rule '{}' has empty when",
            workflow_id,
            rule.id
        );
    }
    match rule.engine {
        StepHookEngine::Cel => {
            let compiled =
                std::panic::catch_unwind(|| Program::compile(expression)).map_err(|_| {
                    anyhow::anyhow!(
                        "workflow '{}' finalize rule '{}' caused CEL parser panic",
                        workflow_id,
                        rule.id
                    )
                })?;
            compiled.map_err(|err| {
                anyhow::anyhow!(
                    "workflow '{}' finalize rule '{}' invalid CEL: {}",
                    workflow_id,
                    rule.id,
                    err
                )
            })?;
        }
    }
    Ok(())
}
