use crate::config::{
    ExecutionProfileMode, OrchestratorConfig, StepSemanticKind, WorkflowConfig,
    resolve_step_semantic_kind,
};
use anyhow::Result;

pub(crate) fn validate_execution_profiles_for_project(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
    project_id: &str,
) -> Result<()> {
    let project = config
        .projects
        .get(project_id)
        .ok_or_else(|| anyhow::anyhow!("project '{}' not found", project_id))?;
    for step in &workflow.steps {
        let Some(profile_name) = step.execution_profile.as_deref() else {
            continue;
        };
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if !matches!(semantic, StepSemanticKind::Agent { .. }) {
            anyhow::bail!(
                "workflow '{}' step '{}' execution_profile is only supported on agent steps",
                workflow_id,
                step.id
            );
        }
        let profile = project
            .execution_profiles
            .get(profile_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "workflow '{}' step '{}' references unknown execution profile '{}'",
                    workflow_id,
                    step.id,
                    profile_name
                )
            })?;
        if profile.mode == ExecutionProfileMode::Host
            && (!profile.writable_paths.is_empty()
                || !profile.network_allowlist.is_empty()
                || profile.max_memory_mb.is_some()
                || profile.max_cpu_seconds.is_some()
                || profile.max_processes.is_some()
                || profile.max_open_files.is_some())
        {
            anyhow::bail!(
                "workflow '{}' step '{}' uses host execution profile '{}' with sandbox-only fields",
                workflow_id,
                step.id,
                profile_name
            );
        }
    }
    Ok(())
}
