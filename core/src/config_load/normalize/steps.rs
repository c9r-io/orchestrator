use crate::config::{CONVENTIONS, WorkflowStepConfig, normalize_step_execution_mode};
use anyhow::Result;

/// Apply sensible non-coordination defaults to well-known step types.
///
/// Defaults are now data-driven via the convention registry
/// (`sdlc_conventions.yaml`) instead of hardcoded match arms.
pub(crate) fn apply_default_step_behavior(step: &mut WorkflowStepConfig) {
    let key = step
        .builtin
        .as_deref()
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    if let Some(conv) = CONVENTIONS.lookup(key)
        && conv.collect_artifacts
    {
        step.behavior.collect_artifacts = true;
    }
}

pub(crate) fn normalize_step_execution_mode_recursive(step: &mut WorkflowStepConfig) -> Result<()> {
    normalize_step_execution_mode(step).map_err(|e| anyhow::anyhow!(e))?;
    for chain_step in &mut step.chain_steps {
        normalize_step_execution_mode_recursive(chain_step)?;
    }
    Ok(())
}
