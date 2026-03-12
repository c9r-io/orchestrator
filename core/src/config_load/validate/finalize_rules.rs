use crate::config::WorkflowConfig;
use anyhow::Result;

/// Validate finalize rules.
pub(super) fn validate_finalize_rules(workflow: &WorkflowConfig, workflow_id: &str) -> Result<()> {
    for rule in &workflow.finalize.rules {
        crate::prehook::validate_workflow_finalize_rule(rule, workflow_id)?;
    }
    Ok(())
}
