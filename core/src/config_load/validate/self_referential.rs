use crate::config::WorkflowConfig;
use crate::self_referential_policy::{
    evaluate_self_referential_policy, format_blocking_policy_error,
};
use anyhow::Result;
use tracing::warn;

/// Validate safety configuration for self-referential workspaces.
pub fn validate_self_referential_safety(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    workspace_id: &str,
    workspace_is_self_referential: bool,
) -> Result<()> {
    let evaluation = evaluate_self_referential_policy(
        workflow,
        workflow_id,
        workspace_id,
        workspace_is_self_referential,
    )?;
    for diagnostic in evaluation
        .diagnostics
        .iter()
        .filter(|diagnostic| !diagnostic.blocking)
    {
        warn!(
            workspace_id,
            rule_id = diagnostic.rule_id,
            "{}",
            diagnostic.message
        );
    }
    if evaluation.has_blocking_errors() {
        anyhow::bail!(format_blocking_policy_error(&evaluation));
    }
    Ok(())
}
