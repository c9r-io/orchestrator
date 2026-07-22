use super::common::AgentLookup;
use crate::config::{LoopMode, StepHookEngine, WorkflowConfig};
use anyhow::Result;
use cel_interpreter::Program;

/// Validate loop policy: max_cycles, fixed mode, guard agent, convergence_expr.
pub(super) fn validate_loop_policy<A: AgentLookup>(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    agents: &A,
) -> Result<()> {
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles
        && max_cycles == 0
    {
        anyhow::bail!("workflow '{workflow_id}' loop.guard.max_cycles must be > 0");
    }
    if matches!(workflow.loop_policy.mode, LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!("workflow '{workflow_id}' loop.mode=fixed requires guard.max_cycles > 0");
    }
    // Only require an agent with loop_guard capability when the guard is
    // enabled, the loop is not `once`, AND no workflow step already provides a
    // builtin loop_guard (which runs internally without agent dispatch).
    let has_builtin_guard = workflow
        .steps
        .iter()
        .any(|s| s.builtin.as_deref() == Some("loop_guard"));
    if workflow.loop_policy.guard.enabled
        && !matches!(workflow.loop_policy.mode, LoopMode::Once)
        && !has_builtin_guard
        && !agents.has_capability("loop_guard")
    {
        anyhow::bail!(
            "workflow '{workflow_id}' loop.guard enabled but no builtin loop_guard step or agent with loop_guard capability found"
        );
    }
    // Validate convergence_expr CEL expressions at config load time.
    if let Some(exprs) = &workflow.loop_policy.convergence_expr {
        for (i, entry) in exprs.iter().enumerate() {
            let expression = entry.when.trim();
            if expression.is_empty() {
                anyhow::bail!(
                    "workflow '{workflow_id}' convergence_expr[{i}] has empty 'when' expression"
                );
            }
            match entry.engine {
                StepHookEngine::Cel => {
                    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
                        .map_err(|_| {
                            anyhow::anyhow!(
                                "workflow '{workflow_id}' convergence_expr[{i}] caused CEL parser panic"
                            )
                        })?;
                    compiled.map_err(|err| {
                        anyhow::anyhow!(
                            "workflow '{workflow_id}' convergence_expr[{i}] invalid CEL: {err}"
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}
