use crate::config::{WorkflowConfig, WorkflowSafetyProfile};
use crate::self_referential_policy::{
    evaluate_self_referential_policy, format_blocking_policy_error,
};
use anyhow::Result;

pub(crate) fn validate_probe_workflow_shape(
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.safety.profile != WorkflowSafetyProfile::SelfReferentialProbe {
        return Ok(());
    }
    let evaluation = evaluate_self_referential_policy(workflow, workflow_id, "__probe__", true)?;
    if evaluation.has_blocking_errors() {
        anyhow::bail!(format_blocking_policy_error(&evaluation));
    }
    Ok(())
}
