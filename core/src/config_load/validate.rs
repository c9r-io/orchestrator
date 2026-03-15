mod adaptive_workflow;
mod agent_env;
mod common;
mod dynamic_steps;
mod execution_profiles;
mod finalize_rules;
mod loop_policy;
mod probe;
mod root_path;
mod self_referential;
#[cfg(test)]
mod tests;
mod workflow_steps;

pub use agent_env::{validate_agent_env_store_refs, validate_agent_env_store_refs_for_project};
pub use root_path::ensure_within_root;
pub use self_referential::validate_self_referential_safety;
pub use workflow_steps::collect_step_warnings;

use crate::config::{OrchestratorConfig, WorkflowConfig, WorkflowSafetyProfile};
use anyhow::Result;
use std::collections::HashMap;

/// Validates a workflow against the globally configured projects and agents.
pub fn validate_workflow_config(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    validate_workflow_config_for_project(config, workflow, workflow_id, None)
}

/// Project-scoped workflow validation. `project_id` of `None` defaults to the
/// default project.
pub fn validate_workflow_config_for_project(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
    project_id: Option<&str>,
) -> Result<()> {
    let pid = config.effective_project_id(project_id);
    let project = config
        .projects
        .get(pid)
        .ok_or_else(|| anyhow::anyhow!("project '{}' not found", pid))?;
    let project_agents = &project.agents;
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }
    validate_probe_workflow_shape(workflow, workflow_id)?;
    validate_execution_profiles_for_project(config, workflow, workflow_id, pid)?;

    workflow_steps::validate_workflow_steps(&workflow.steps, workflow_id, project_agents)?;
    finalize_rules::validate_finalize_rules(workflow, workflow_id)?;
    dynamic_steps::validate_dynamic_steps(workflow, workflow_id)?;
    loop_policy::validate_loop_policy(workflow, workflow_id, project_agents)?;
    adaptive_workflow::validate_adaptive_workflow(workflow, workflow_id, project_agents)?;

    let self_referential_workspaces: Vec<_> = project
        .workspaces
        .iter()
        .filter(|(_, workspace)| workspace.self_referential)
        .collect();

    if workflow.safety.profile == WorkflowSafetyProfile::SelfReferentialProbe {
        if self_referential_workspaces.is_empty() {
            validate_self_referential_safety(workflow, workflow_id, "__unbound__", false)?;
        } else {
            for (workspace_id, _) in &self_referential_workspaces {
                validate_self_referential_safety(workflow, workflow_id, workspace_id, true)?;
            }
        }
    } else {
        for (workspace_id, _) in self_referential_workspaces {
            validate_self_referential_safety(workflow, workflow_id, workspace_id, true)?;
        }
    }
    Ok(())
}

pub(crate) use execution_profiles::validate_execution_profiles_for_project;
pub(crate) use probe::validate_probe_workflow_shape;

pub(crate) fn validate_workflow_config_with_agents(
    all_agents: &HashMap<String, &crate::config::AgentConfig>,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }
    validate_probe_workflow_shape(workflow, workflow_id)?;

    workflow_steps::validate_workflow_steps(&workflow.steps, workflow_id, all_agents)?;
    finalize_rules::validate_finalize_rules(workflow, workflow_id)?;
    loop_policy::validate_loop_policy(workflow, workflow_id, all_agents)?;
    adaptive_workflow::validate_adaptive_workflow(workflow, workflow_id, all_agents)?;
    Ok(())
}

// Legacy entry points kept for test coverage; delegate to the generic helper.
#[cfg(test)]
fn validate_adaptive_workflow_config(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    all_agents: &HashMap<String, crate::config::AgentConfig>,
) -> Result<()> {
    adaptive_workflow::validate_adaptive_workflow(workflow, workflow_id, all_agents)
}

#[cfg(test)]
fn validate_adaptive_workflow_config_refs(
    workflow: &WorkflowConfig,
    workflow_id: &str,
    all_agents: &HashMap<String, &crate::config::AgentConfig>,
) -> Result<()> {
    adaptive_workflow::validate_adaptive_workflow(workflow, workflow_id, all_agents)
}
