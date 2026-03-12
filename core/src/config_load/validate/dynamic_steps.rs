use crate::config::{StepHookEngine, StepPrehookConfig, WorkflowConfig};
use anyhow::Result;

/// Validate dynamic step triggers.
pub(super) fn validate_dynamic_steps(workflow: &WorkflowConfig, workflow_id: &str) -> Result<()> {
    for dynamic_step in &workflow.dynamic_steps {
        if let Some(trigger) = dynamic_step.trigger.as_deref() {
            let prehook = StepPrehookConfig {
                engine: StepHookEngine::Cel,
                when: trigger.to_string(),
                reason: Some(format!("dynamic step '{}'", dynamic_step.id)),
                ui: None,
                extended: false,
            };
            crate::prehook::validate_step_prehook(&prehook, workflow_id, &dynamic_step.id)?;
        }
    }
    Ok(())
}
