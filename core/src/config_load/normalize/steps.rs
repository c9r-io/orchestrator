use crate::config::{
    normalize_step_execution_mode, CaptureDecl, CaptureSource, PostAction, WorkflowStepConfig,
};
use anyhow::Result;

/// Apply sensible default behavior to well-known step types when the user
/// hasn't configured explicit captures or collect_artifacts.
pub(crate) fn apply_default_step_behavior(step: &mut WorkflowStepConfig) {
    let key = step
        .builtin
        .as_deref()
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    let has_capture = |var: &str| step.behavior.captures.iter().any(|c| c.var == var);
    let has_post_action = |pa: &PostAction| step.behavior.post_actions.iter().any(|a| a == pa);

    match key {
        "qa" | "qa_testing" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("qa_failed") {
                step.behavior.captures.push(CaptureDecl {
                    var: "qa_failed".to_string(),
                    source: CaptureSource::FailedFlag,
                });
            }
            if !has_post_action(&PostAction::CreateTicket) {
                step.behavior.post_actions.push(PostAction::CreateTicket);
            }
        }
        "fix" | "ticket_fix" => {
            if !has_capture("fix_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "fix_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        "retest" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("retest_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "retest_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        _ => {}
    }
}

pub(crate) fn normalize_step_execution_mode_recursive(step: &mut WorkflowStepConfig) -> Result<()> {
    normalize_step_execution_mode(step).map_err(|e| anyhow::anyhow!(e))?;
    for chain_step in &mut step.chain_steps {
        normalize_step_execution_mode_recursive(chain_step)?;
    }
    Ok(())
}
