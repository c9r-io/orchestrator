use crate::config::{CONVENTIONS, WorkflowStepConfig, normalize_step_execution_mode};
use anyhow::Result;

/// Apply sensible default behavior to well-known step types when the user
/// hasn't configured explicit captures or collect_artifacts.
///
/// Defaults are now data-driven via the convention registry
/// (`sdlc_conventions.yaml`) instead of hardcoded match arms.
pub(crate) fn apply_default_step_behavior(step: &mut WorkflowStepConfig) {
    let key = step
        .builtin
        .as_deref()
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    if let Some(conv) = CONVENTIONS.lookup(key) {
        if conv.collect_artifacts {
            step.behavior.collect_artifacts = true;
        }
        for capture in &conv.captures {
            if !step.behavior.captures.iter().any(|c| c.var == capture.var) {
                step.behavior.captures.push(capture.clone());
            }
        }
        for action in &conv.post_actions {
            if !step.behavior.post_actions.iter().any(|a| a == action) {
                step.behavior.post_actions.push(action.clone());
            }
        }
    }
}

pub(crate) fn normalize_step_execution_mode_recursive(step: &mut WorkflowStepConfig) -> Result<()> {
    normalize_step_execution_mode(step).map_err(|e| anyhow::anyhow!(e))?;
    for chain_step in &mut step.chain_steps {
        normalize_step_execution_mode_recursive(chain_step)?;
    }
    Ok(())
}
