use crate::config::OrchestratorConfig;
use anyhow::Result;

use super::helpers::apply_to_map;
use super::registry::RegisteredResource;
use super::{agent, workflow, workspace, ApplyResult, Resource};

/// Apply a resource into a specific project scope.
/// Builtin resources are routed to `config.projects[project].<kind>`.
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
            step_templates: Default::default(),
            env_stores: Default::default(),
            execution_profiles: Default::default(),
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
        RegisteredResource::StepTemplate(template) => {
            let incoming = crate::config::StepTemplateConfig {
                prompt: template.spec.prompt.clone(),
                description: template.spec.description.clone(),
            };
            Ok(apply_to_map(
                &mut project_entry.step_templates,
                template.name(),
                incoming,
            ))
        }
        RegisteredResource::EnvStore(store) => {
            let incoming = crate::config::EnvStoreConfig {
                data: store.spec.data.clone(),
                sensitive: false,
            };
            Ok(apply_to_map(
                &mut project_entry.env_stores,
                store.name(),
                incoming,
            ))
        }
        RegisteredResource::ExecutionProfile(profile) => Ok(apply_to_map(
            &mut project_entry.execution_profiles,
            profile.name(),
            crate::resource::execution_profile::execution_profile_spec_to_config(&profile.spec),
        )),
        RegisteredResource::SecretStore(store) => {
            let incoming = crate::config::EnvStoreConfig {
                data: store.spec.data.clone(),
                sensitive: true,
            };
            Ok(apply_to_map(
                &mut project_entry.env_stores,
                store.name(),
                incoming,
            ))
        }
        _ => resource.apply(config),
    }
}
