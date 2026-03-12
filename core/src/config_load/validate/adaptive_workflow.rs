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
                "workflow '{}' adaptive planner is enabled but adaptive.planner_agent is missing",
                workflow_id
            )
        })?;

    let agent = agents.get_agent(planner_agent).ok_or_else(|| {
        anyhow::anyhow!(
            "workflow '{}' adaptive planner references unknown agent '{}'",
            workflow_id,
            planner_agent
        )
    })?;

    if !agent.supports_capability("adaptive_plan") {
        anyhow::bail!(
            "workflow '{}' adaptive planner agent '{}' must support capability 'adaptive_plan'",
            workflow_id,
            planner_agent
        );
    }

    Ok(())
}
