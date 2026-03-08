use crate::config::OrchestratorConfig;
use anyhow::Result;

use super::helpers::apply_to_map;
use super::registry::RegisteredResource;
use super::{agent, workflow, workspace, ApplyResult, Resource};

/// Apply a resource into a project scope instead of global config.
/// Agent, Workflow, and Workspace resources are routed to `config.projects[project].<kind>`.
/// Other resource types fall back to global apply.
pub fn apply_to_project(
    resource: &RegisteredResource,
    config: &mut OrchestratorConfig,
    project: &str,
) -> Result<ApplyResult> {
    use crate::config::ProjectConfig;

    let project_entry = config
        .projects
        .entry(project.to_string())
        .or_insert_with(|| ProjectConfig {
            description: None,
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
        });

    match resource {
        RegisteredResource::Agent(agent) => {
            let incoming = agent::agent_spec_to_config(&agent.spec);
            Ok(apply_to_map(
                &mut project_entry.agents,
                agent.name(),
                incoming,
            ))
        }
        RegisteredResource::Workflow(workflow) => {
            let incoming = workflow::workflow_spec_to_config(&workflow.spec)?;
            Ok(apply_to_map(
                &mut project_entry.workflows,
                workflow.name(),
                incoming,
            ))
        }
        RegisteredResource::Workspace(ws) => {
            let incoming = workspace::workspace_spec_to_config(&ws.spec);
            Ok(apply_to_map(
                &mut project_entry.workspaces,
                ws.name(),
                incoming,
            ))
        }
        // Singletons and other types always go to global config
        _ => resource.apply(config),
    }
}
