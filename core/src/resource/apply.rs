use crate::cli_types::ResourceMetadata;
use crate::config::OrchestratorConfig;
use anyhow::Result;

use super::helpers::apply_to_store;
use super::registry::RegisteredResource;
use super::{ApplyResult, Resource};

/// Helper: clone metadata with project set to the target project scope.
fn scoped_metadata(metadata: &ResourceMetadata, project: &str) -> ResourceMetadata {
    let mut m = metadata.clone();
    m.project = Some(project.to_string());
    m
}

/// Apply a resource into a specific project scope.
/// Builtin resources are routed to `config.projects[project].<kind>` via
/// `apply_to_store` which stores the full CR (including labels/annotations)
/// in the resource_store and reconciles back to the config snapshot.
pub fn apply_to_project(
    resource: &RegisteredResource,
    config: &mut OrchestratorConfig,
    project: &str,
) -> Result<ApplyResult> {
    // Ensure the target project entry exists before apply_to_store runs.
    config.ensure_project(Some(project));

    match resource {
        RegisteredResource::Agent(agent) => {
            let metadata = scoped_metadata(&agent.metadata, project);
            Ok(apply_to_store(
                config,
                "Agent",
                agent.name(),
                &metadata,
                serde_json::to_value(&agent.spec)?,
            ))
        }
        RegisteredResource::Workflow(workflow) => {
            let metadata = scoped_metadata(&workflow.metadata, project);
            Ok(apply_to_store(
                config,
                "Workflow",
                workflow.name(),
                &metadata,
                serde_json::to_value(&workflow.spec)?,
            ))
        }
        RegisteredResource::Workspace(ws) => {
            let metadata = scoped_metadata(&ws.metadata, project);
            Ok(apply_to_store(
                config,
                "Workspace",
                ws.name(),
                &metadata,
                serde_json::to_value(&ws.spec)?,
            ))
        }
        RegisteredResource::StepTemplate(template) => {
            let metadata = scoped_metadata(&template.metadata, project);
            Ok(apply_to_store(
                config,
                "StepTemplate",
                template.name(),
                &metadata,
                serde_json::to_value(&template.spec)?,
            ))
        }
        RegisteredResource::EnvStore(store) => {
            let metadata = scoped_metadata(&store.metadata, project);
            Ok(apply_to_store(
                config,
                "EnvStore",
                store.name(),
                &metadata,
                serde_json::to_value(&store.spec)?,
            ))
        }
        RegisteredResource::ExecutionProfile(profile) => {
            let metadata = scoped_metadata(&profile.metadata, project);
            Ok(apply_to_store(
                config,
                "ExecutionProfile",
                profile.name(),
                &metadata,
                serde_json::to_value(&profile.spec)?,
            ))
        }
        RegisteredResource::SecretStore(store) => {
            let metadata = scoped_metadata(&store.metadata, project);
            Ok(apply_to_store(
                config,
                "SecretStore",
                store.name(),
                &metadata,
                serde_json::to_value(&store.spec)?,
            ))
        }
        RegisteredResource::RuntimePolicy(rp) => {
            let metadata = scoped_metadata(&rp.metadata, project);
            Ok(apply_to_store(
                config,
                "RuntimePolicy",
                rp.name(),
                &metadata,
                serde_json::to_value(&rp.spec)?,
            ))
        }
        _ => resource.apply(config),
    }
}
