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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorkflowConfig;
    use orchestrator_config::dynamic_step::DynamicStepConfig;

    fn make_dynamic_step(id: &str, trigger: Option<&str>) -> DynamicStepConfig {
        DynamicStepConfig {
            id: id.to_string(),
            description: None,
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            trigger: trigger.map(String::from),
            priority: 0,
            max_runs: None,
        }
    }

    #[test]
    fn empty_dynamic_steps_ok() {
        let workflow = WorkflowConfig::default();
        let result = validate_dynamic_steps(&workflow, "test-wf");
        assert!(result.is_ok());
    }

    #[test]
    fn dynamic_step_no_trigger_ok() {
        let workflow = WorkflowConfig {
            dynamic_steps: vec![make_dynamic_step("ds1", None)],
            ..Default::default()
        };
        let result = validate_dynamic_steps(&workflow, "test-wf");
        assert!(result.is_ok());
    }

    #[test]
    fn dynamic_step_valid_cel_trigger_ok() {
        let workflow = WorkflowConfig {
            dynamic_steps: vec![make_dynamic_step("ds1", Some("qa_failed == true"))],
            ..Default::default()
        };
        let result = validate_dynamic_steps(&workflow, "test-wf");
        assert!(result.is_ok());
    }

    #[test]
    fn dynamic_step_invalid_cel_trigger_err() {
        let workflow = WorkflowConfig {
            dynamic_steps: vec![make_dynamic_step("ds1", Some("invalid @#$ expression"))],
            ..Default::default()
        };
        let result = validate_dynamic_steps(&workflow, "test-wf");
        assert!(result.is_err());
    }
}
