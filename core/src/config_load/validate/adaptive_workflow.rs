use super::common::AgentLookup;
use crate::config::WorkflowConfig;
use anyhow::Result;

/// Validate adaptive workflow config generically over agent map type.
pub(super) fn validate_adaptive_workflow<A: AgentLookup>(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    agents: &A,
) -> Result<()> {
    let Some(adaptive) = workflow.adaptive.as_ref() else {
        return Ok(());
    };
    if !adaptive.enabled {
        return Ok(());
    }

    let planner_agent = adaptive
        .planner_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "workflow '{workflow_id}' adaptive planner is enabled but adaptive.planner_agent is missing"
            )
        })?;

    let agent = agents.get_agent(planner_agent).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow '{workflow_id}' adaptive planner references unknown agent '{planner_agent}'"
        )
    })?;

    if !agent.supports_capability("adaptive_plan") {
        anyhow::bail!(
            "workflow '{workflow_id}' adaptive planner agent '{planner_agent}' must support capability 'adaptive_plan'"
        );
    }

    Ok(())
}
