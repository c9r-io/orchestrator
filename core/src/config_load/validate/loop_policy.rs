use super::common::AgentLookup;
use crate::config::{LoopMode, WorkflowConfig};
use anyhow::Result;

/// Validate loop policy: max_cycles, fixed mode, guard agent.
pub(super) fn validate_loop_policy<A: AgentLookup>(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    agents: &A,
) -> Result<()> {
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles {
        if max_cycles == 0 {
            anyhow::bail!(
                "workflow '{}' loop.guard.max_cycles must be > 0",
                workflow_id
            );
        }
    }
    if matches!(workflow.loop_policy.mode, LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!(
            "workflow '{}' loop.mode=fixed requires guard.max_cycles > 0",
            workflow_id
        );
    }
    if workflow.loop_policy.guard.enabled
        && !matches!(workflow.loop_policy.mode, LoopMode::Once)
        && !agents.has_capability("loop_guard")
    {
        anyhow::bail!(
            "workflow '{}' loop.guard enabled but no agent supports loop_guard capability",
            workflow_id
        );
    }
    Ok(())
}
