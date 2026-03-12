use super::common::AgentLookup;
use crate::config::{resolve_step_semantic_kind, StepSemanticKind, WorkflowStepConfig};
use anyhow::Result;
use std::collections::HashSet;

/// Validate the step loop: duplicate IDs, semantic kind, agent capability, prehook.
pub(super) fn validate_workflow_steps<A: AgentLookup>(
    steps: &[WorkflowStepConfig],
    workflow_id: &str,
    agents: &A,
) -> Result<usize> {
    let mut enabled_count = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if matches!(
            semantic,
            StepSemanticKind::Builtin { ref name } if name == "ticket_scan"
        ) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let is_self_contained = matches!(
            semantic,
            StepSemanticKind::Builtin { .. } | StepSemanticKind::Command | StepSemanticKind::Chain
        );
        if !is_self_contained && !agents.has_capability(key) {
            anyhow::bail!(
                "no agent supports capability for step '{}' used by workflow '{}'",
                key,
                workflow_id
            );
        }
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{}' has no enabled steps", workflow_id);
    }
    Ok(enabled_count)
}
