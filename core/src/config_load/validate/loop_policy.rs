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
            "workflow '{}' loop.guard enabled but no builtin loop_guard step or agent with loop_guard capability found",
            workflow_id
        );
    }
    // Validate convergence_expr CEL expressions at config load time.
    if let Some(exprs) = &workflow.loop_policy.convergence_expr {
        for (i, entry) in exprs.iter().enumerate() {
            let expression = entry.when.trim();
            if expression.is_empty() {
                anyhow::bail!(
                    "workflow '{}' convergence_expr[{}] has empty 'when' expression",
                    workflow_id,
                    i
                );
            }
            match entry.engine {
                StepHookEngine::Cel => {
                    let compiled = std::panic::catch_unwind(|| Program::compile(expression))
                        .map_err(|_| {
                            anyhow::anyhow!(
                                "workflow '{}' convergence_expr[{}] caused CEL parser panic",
                                workflow_id,
                                i
                            )
                        })?;
                    compiled.map_err(|err| {
                        anyhow::anyhow!(
                            "workflow '{}' convergence_expr[{}] invalid CEL: {}",
                            workflow_id,
                            i,
                            err
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}
