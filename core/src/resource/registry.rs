use crate::cli_types::{OrchestratorResource, ResourceKind};
use crate::config::OrchestratorConfig;
use anyhow::{anyhow, Result};

use super::{
    agent, env_store, project, runtime_policy, secret_store, step_template, workflow,
    workspace, ApplyResult, Resource,
};
use super::{
    AgentResource, EnvStoreResource, ProjectResource, RuntimePolicyResource, SecretStoreResource,
    StepTemplateResource, WorkflowResource, WorkspaceResource,
};

#[derive(Debug, Clone)]
pub enum RegisteredResource {
    Workspace(WorkspaceResource),
    Agent(Box<AgentResource>),
    Workflow(WorkflowResource),
    Project(ProjectResource),
    RuntimePolicy(RuntimePolicyResource),
    StepTemplate(StepTemplateResource),
    EnvStore(EnvStoreResource),
    SecretStore(SecretStoreResource),
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRegistration {
    pub kind: ResourceKind,
    pub build: fn(OrchestratorResource) -> Result<RegisteredResource>,
}

pub fn resource_registry() -> [ResourceRegistration; 8] {
    [
        ResourceRegistration {
            kind: ResourceKind::Workspace,
            build: workspace::build_workspace,
        },
        ResourceRegistration {
            kind: ResourceKind::Agent,
            build: agent::build_agent,
        },
        ResourceRegistration {
            kind: ResourceKind::Workflow,
            build: workflow::build_workflow,
        },
        ResourceRegistration {
            kind: ResourceKind::Project,
            build: project::build_project,
        },
        ResourceRegistration {
            kind: ResourceKind::RuntimePolicy,
            build: runtime_policy::build_runtime_policy,
        },
        ResourceRegistration {
            kind: ResourceKind::StepTemplate,
            build: step_template::build_step_template,
        },
        ResourceRegistration {
            kind: ResourceKind::EnvStore,
            build: env_store::build_env_store,
        },
        ResourceRegistration {
            kind: ResourceKind::SecretStore,
            build: secret_store::build_secret_store,
        },
    ]
}

impl RegisteredResource {
    /// Return the metadata.project field if present on this resource
    pub fn metadata_project(&self) -> Option<&str> {
        let meta = match self {
            Self::Workspace(r) => &r.metadata,
            Self::Agent(r) => &r.metadata,
            Self::Workflow(r) => &r.metadata,
            Self::Project(r) => &r.metadata,
            Self::RuntimePolicy(r) => &r.metadata,
            Self::StepTemplate(r) => &r.metadata,
            Self::EnvStore(r) => &r.metadata,
            Self::SecretStore(r) => &r.metadata,
        };
        meta.project.as_deref()
    }
}

pub fn dispatch_resource(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let kind = resource.kind;
    if let Some(registration) = resource_registry().iter().find(|entry| entry.kind == kind) {
        return (registration.build)(resource);
    }
    Err(anyhow!("unsupported resource kind"))
}

// ── RegisteredResource impl ───────────────────────────────────────────────────

impl Resource for RegisteredResource {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Agent(_) => ResourceKind::Agent,
            Self::Workflow(_) => ResourceKind::Workflow,
            Self::Project(_) => ResourceKind::Project,
            Self::RuntimePolicy(_) => ResourceKind::RuntimePolicy,
            Self::StepTemplate(_) => ResourceKind::StepTemplate,
            Self::EnvStore(_) => ResourceKind::EnvStore,
            Self::SecretStore(_) => ResourceKind::SecretStore,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Workspace(resource) => &resource.metadata.name,
            Self::Agent(resource) => &resource.metadata.name,
            Self::Workflow(resource) => &resource.metadata.name,
            Self::Project(resource) => &resource.metadata.name,
            Self::RuntimePolicy(resource) => &resource.metadata.name,
            Self::StepTemplate(resource) => &resource.metadata.name,
            Self::EnvStore(resource) => &resource.metadata.name,
            Self::SecretStore(resource) => &resource.metadata.name,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Workspace(resource) => resource.validate(),
            Self::Agent(resource) => resource.validate(),
            Self::Workflow(resource) => resource.validate(),
            Self::Project(resource) => resource.validate(),
            Self::RuntimePolicy(resource) => resource.validate(),
            Self::StepTemplate(resource) => resource.validate(),
            Self::EnvStore(resource) => resource.validate(),
            Self::SecretStore(resource) => resource.validate(),
        }
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        match self {
            Self::Workspace(resource) => resource.apply(config),
            Self::Agent(resource) => resource.apply(config),
            Self::Workflow(resource) => resource.apply(config),
            Self::Project(resource) => resource.apply(config),
            Self::RuntimePolicy(resource) => resource.apply(config),
            Self::StepTemplate(resource) => resource.apply(config),
            Self::EnvStore(resource) => resource.apply(config),
            Self::SecretStore(resource) => resource.apply(config),
        }
    }

    fn to_yaml(&self) -> Result<String> {
        match self {
            Self::Workspace(resource) => resource.to_yaml(),
            Self::Agent(resource) => resource.to_yaml(),
            Self::Workflow(resource) => resource.to_yaml(),
            Self::Project(resource) => resource.to_yaml(),
            Self::RuntimePolicy(resource) => resource.to_yaml(),
            Self::StepTemplate(resource) => resource.to_yaml(),
            Self::EnvStore(resource) => resource.to_yaml(),
            Self::SecretStore(resource) => resource.to_yaml(),
        }
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        if let Some(workspace) = WorkspaceResource::get_from(config, name) {
            return Some(Self::Workspace(workspace));
        }
        if let Some(agent) = AgentResource::get_from(config, name) {
            return Some(Self::Agent(Box::new(agent)));
        }
        if let Some(workflow) = WorkflowResource::get_from(config, name) {
            return Some(Self::Workflow(workflow));
        }
        if let Some(project) = ProjectResource::get_from(config, name) {
            return Some(Self::Project(project));
        }
        if let Some(step_template) = StepTemplateResource::get_from(config, name) {
            return Some(Self::StepTemplate(step_template));
        }
        if name == "runtime" {
            if let Some(runtime_policy) = RuntimePolicyResource::get_from(config, name) {
                return Some(Self::RuntimePolicy(runtime_policy));
            }
        }
        if let Some(env_store) = EnvStoreResource::get_from(config, name) {
            return Some(Self::EnvStore(env_store));
        }
        if let Some(secret_store) = SecretStoreResource::get_from(config, name) {
            return Some(Self::SecretStore(secret_store));
        }
        None
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        // Try each builtin kind in turn
        if WorkspaceResource::delete_from(config, name) {
            return true;
        }
        if AgentResource::delete_from(config, name) {
            return true;
        }
        if WorkflowResource::delete_from(config, name) {
            return true;
        }
        if ProjectResource::delete_from(config, name) {
            return true;
        }
        if StepTemplateResource::delete_from(config, name) {
            return true;
        }
        if EnvStoreResource::delete_from(config, name) {
            return true;
        }
        if SecretStoreResource::delete_from(config, name) {
            return true;
        }
        false
    }
}
