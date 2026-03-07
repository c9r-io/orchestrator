use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceMetadata, ResourceSpec};
use crate::config::OrchestratorConfig;
use anyhow::{anyhow, Result};
use serde::Serialize;

pub(crate) const API_VERSION: &str = "orchestrator.dev/v2";

// ── Submodules ────────────────────────────────────────────────────────────────

pub(crate) mod agent;
mod defaults;
mod env_store;
mod export;
mod parse;
mod project;
pub(crate) mod runtime_policy;
mod secret_store;
mod step_template;
pub(crate) mod workflow;
pub(crate) mod workspace;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use agent::AgentResource;
pub use defaults::DefaultsResource;
pub use env_store::EnvStoreResource;
pub use export::{export_crd_documents, export_manifest_documents, export_manifest_resources};
pub use parse::{
    delete_resource_by_kind, kind_as_str, parse_manifests_from_yaml, parse_resources_from_yaml,
};
pub use project::ProjectResource;
pub use runtime_policy::RuntimePolicyResource;
pub use secret_store::SecretStoreResource;
pub use step_template::StepTemplateResource;
pub use workflow::WorkflowResource;
pub use workspace::WorkspaceResource;

// ── Core types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    Created,
    Configured,
    Unchanged,
}

pub trait Resource: Sized {
    fn kind(&self) -> ResourceKind;
    fn name(&self) -> &str;
    fn validate(&self) -> Result<()>;
    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult>;
    fn to_yaml(&self) -> Result<String>;
    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self>;
    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool;
}

#[derive(Debug, Clone)]
pub enum RegisteredResource {
    Workspace(WorkspaceResource),
    Agent(Box<AgentResource>),
    Workflow(WorkflowResource),
    Project(ProjectResource),
    Defaults(DefaultsResource),
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

pub fn resource_registry() -> [ResourceRegistration; 9] {
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
            kind: ResourceKind::Defaults,
            build: defaults::build_defaults,
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
            Self::Defaults(r) => &r.metadata,
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

// ── Shared helpers (pub(crate) so submodules can use them via super::) ────────

pub(crate) fn validate_resource_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("metadata.name cannot be empty"));
    }
    Ok(())
}

pub(crate) fn metadata_with_name(name: &str) -> ResourceMetadata {
    ResourceMetadata {
        name: name.to_string(),
        project: None,
        labels: None,
        annotations: None,
    }
}

#[allow(dead_code)]
pub(crate) fn metadata_from_parts(
    name: &str,
    project: Option<String>,
    labels: Option<std::collections::HashMap<String, String>>,
    annotations: Option<std::collections::HashMap<String, String>>,
) -> ResourceMetadata {
    ResourceMetadata {
        name: name.to_string(),
        project,
        labels,
        annotations,
    }
}

/// Read resource metadata from the ResourceStore, falling back to name-only.
pub fn metadata_from_store(
    config: &OrchestratorConfig,
    kind: &str,
    name: &str,
) -> ResourceMetadata {
    match config.resource_store.get(kind, name) {
        Some(cr) => cr.metadata.clone(),
        None => metadata_with_name(name),
    }
}

pub(crate) fn manifest_yaml(
    kind: ResourceKind,
    metadata: &ResourceMetadata,
    spec: ResourceSpec,
) -> Result<String> {
    let manifest = OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind,
        metadata: metadata.clone(),
        spec,
    };
    Ok(serde_yml::to_string(&manifest)?)
}

pub(crate) fn apply_to_map<T: Clone + Serialize>(
    map: &mut std::collections::HashMap<String, T>,
    name: &str,
    incoming: T,
) -> ApplyResult {
    match map.get(name) {
        None => {
            map.insert(name.to_string(), incoming);
            ApplyResult::Created
        }
        Some(existing) => {
            if serializes_equal(existing, &incoming) {
                ApplyResult::Unchanged
            } else {
                map.insert(name.to_string(), incoming);
                ApplyResult::Configured
            }
        }
    }
}

pub(crate) fn serializes_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

/// Apply a builtin resource to the unified ResourceStore, then write back
/// the single affected entry to the legacy config field.
pub(crate) fn apply_to_store(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
    metadata: &ResourceMetadata,
    spec: serde_json::Value,
) -> ApplyResult {
    use crate::crd::types::CustomResource;

    let now = chrono::Utc::now().to_rfc3339();

    // If the store doesn't have this entry yet but the legacy field does,
    // seed the store from the legacy field so that put() can correctly
    // detect Unchanged vs Configured (instead of always returning Created).
    if config.resource_store.get(kind, name).is_none() {
        crate::crd::writeback::seed_store_from_legacy(config, kind, name, &now);
    }

    // Preserve generation and created_at from existing CR if updating
    let (generation, created_at) = match config.resource_store.get(kind, name) {
        Some(existing) => (existing.generation + 1, existing.created_at.clone()),
        None => (1, now.clone()),
    };

    let cr = CustomResource {
        kind: kind.to_string(),
        api_version: "orchestrator.dev/v2".to_string(),
        metadata: metadata.clone(),
        spec,
        generation,
        created_at,
        updated_at: now,
    };
    let result = config.resource_store.put(cr);
    // Targeted writeback: only update the specific entry, not the whole map
    crate::crd::writeback::write_back_single(config, kind, name);
    result
}

/// Delete a builtin resource from the unified ResourceStore, then remove
/// the single affected entry from the legacy config field.
pub(crate) fn delete_from_store(config: &mut OrchestratorConfig, kind: &str, name: &str) -> bool {
    // If the store doesn't have this entry yet but the legacy field does,
    // seed it first so that remove() returns Some and we actually delete it.
    if config.resource_store.get(kind, name).is_none() {
        let now = chrono::Utc::now().to_rfc3339();
        crate::crd::writeback::seed_store_from_legacy(config, kind, name, &now);
    }

    let removed = config.resource_store.remove(kind, name).is_some();
    if removed {
        crate::crd::writeback::remove_from_legacy(config, kind, name);
    }
    removed
}

// ── Project-scoped apply ──────────────────────────────────────────────────────

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

// ── RegisteredResource impl ───────────────────────────────────────────────────

impl Resource for RegisteredResource {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Agent(_) => ResourceKind::Agent,
            Self::Workflow(_) => ResourceKind::Workflow,
            Self::Project(_) => ResourceKind::Project,
            Self::Defaults(_) => ResourceKind::Defaults,
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
            Self::Defaults(resource) => &resource.metadata.name,
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
            Self::Defaults(resource) => resource.validate(),
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
            Self::Defaults(resource) => resource.apply(config),
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
            Self::Defaults(resource) => resource.to_yaml(),
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
        if name == "defaults" {
            if let Some(defaults) = DefaultsResource::get_from(config, name) {
                return Some(Self::Defaults(defaults));
            }
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{AgentSpec, ResourceSpec, WorkspaceSpec};
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    use super::test_fixtures::{
        agent_manifest, defaults_manifest, make_config, project_manifest, runtime_policy_manifest,
        workflow_manifest, workspace_manifest,
    };

    #[test]
    fn resource_dispatch_maps_workspace_manifest() {
        let resource = dispatch_resource(workspace_manifest("dispatch-ws", "workspace/dispatch"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Workspace);
        assert_eq!(resource.name(), "dispatch-ws");
    }

    #[test]
    fn resource_dispatch_rejects_mismatched_spec_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Agent(Box::new(AgentSpec {
                command: "echo {prompt}".to_string(),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            })),
        };

        let error = dispatch_resource(resource).expect_err("dispatch should fail");
        assert!(error.to_string().contains("mismatch"));
    }

    #[test]
    fn resource_registry_has_nine_entries() {
        let registry = resource_registry();
        assert_eq!(registry.len(), 9);
        let kinds: Vec<ResourceKind> = registry.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::StepTemplate));
        assert!(kinds.contains(&ResourceKind::EnvStore));
        assert!(kinds.contains(&ResourceKind::SecretStore));
    }

    #[test]
    fn resource_trait_validate_rejects_empty_name() {
        let resource = dispatch_resource(workspace_manifest("", "workspace/invalid"))
            .expect("dispatch should succeed");
        let result = resource.validate();
        assert!(result.is_err());
    }

    #[test]
    fn resource_trait_to_yaml_serializes_manifest_shape() {
        let resource = dispatch_resource(workspace_manifest("yaml-ws", "workspace/yaml"))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("yaml serialization should work");
        assert!(yaml.contains("apiVersion: orchestrator.dev/v2"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-ws"));
    }

    #[test]
    fn resource_trait_get_from_reads_existing_config() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let active = read_active_config(&state).expect("state should be readable");
        let resource = RegisteredResource::get_from(&active.config, "default")
            .expect("default workspace should exist");
        assert_eq!(resource.kind(), ResourceKind::Workspace);
        assert_eq!(resource.name(), "default");
    }

    #[test]
    fn apply_result_created_when_missing() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workspace_manifest("fresh-ws", "workspace/fresh"))
            .expect("dispatch should succeed");
        let result = resource.apply(&mut config).expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.workspaces.contains_key("fresh-ws"));
    }

    #[test]
    fn apply_result_unchanged_for_identical_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workspace_manifest("same-ws", "workspace/same"))
            .expect("dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn apply_result_configured_when_resource_changes() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let initial = dispatch_resource(workspace_manifest("change-ws", "workspace/v1"))
            .expect("dispatch should succeed");
        assert_eq!(
            initial.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let updated = dispatch_resource(workspace_manifest("change-ws", "workspace/v2"))
            .expect("dispatch should succeed");
        assert_eq!(
            updated.apply(&mut config).expect("apply"),
            ApplyResult::Configured
        );
    }

    // ── RegisteredResource dispatch delegation ─────────────────────────

    #[test]
    fn registered_resource_kind_name_for_all_variants() {
        let ws =
            dispatch_resource(workspace_manifest("rr-ws", "workspace/rr")).expect("dispatch ws");
        assert_eq!(ws.kind(), ResourceKind::Workspace);
        assert_eq!(ws.name(), "rr-ws");

        let ag = dispatch_resource(agent_manifest("rr-ag", "cmd")).expect("dispatch agent");
        assert_eq!(ag.kind(), ResourceKind::Agent);
        assert_eq!(ag.name(), "rr-ag");

        let wf = dispatch_resource(workflow_manifest("rr-wf")).expect("dispatch workflow");
        assert_eq!(wf.kind(), ResourceKind::Workflow);
        assert_eq!(wf.name(), "rr-wf");

        let pr = dispatch_resource(project_manifest("rr-pr", "d")).expect("dispatch project");
        assert_eq!(pr.kind(), ResourceKind::Project);
        assert_eq!(pr.name(), "rr-pr");

        let df = dispatch_resource(defaults_manifest("", "", "")).expect("dispatch defaults");
        assert_eq!(df.kind(), ResourceKind::Defaults);
        assert_eq!(df.name(), "defaults");

        let rp = dispatch_resource(runtime_policy_manifest()).expect("dispatch runtime policy");
        assert_eq!(rp.kind(), ResourceKind::RuntimePolicy);
        assert_eq!(rp.name(), "runtime");
    }

    #[test]
    fn registered_resource_validate_delegates() {
        let ws = dispatch_resource(workspace_manifest("v-ws", "workspace/v"))
            .expect("dispatch validation ws");
        assert!(ws.validate().is_ok());

        let ag =
            dispatch_resource(agent_manifest("v-ag", "cmd")).expect("dispatch validation agent");
        assert!(ag.validate().is_ok());

        let wf =
            dispatch_resource(workflow_manifest("v-wf")).expect("dispatch validation workflow");
        assert!(wf.validate().is_ok());

        let pr =
            dispatch_resource(project_manifest("v-pr", "d")).expect("dispatch validation project");
        assert!(pr.validate().is_ok());

        let df =
            dispatch_resource(defaults_manifest("", "", "")).expect("dispatch validation defaults");
        assert!(df.validate().is_ok());

        let rp = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch validation runtime policy");
        assert!(rp.validate().is_ok());
    }

    #[test]
    fn registered_resource_to_yaml_delegates() {
        let ws = dispatch_resource(workspace_manifest("ty-ws", "workspace/ty"))
            .expect("dispatch yaml ws");
        assert!(ws
            .to_yaml()
            .expect("serialize workspace yaml")
            .contains("Workspace"));

        let ag = dispatch_resource(agent_manifest("ty-ag", "cmd")).expect("dispatch yaml agent");
        assert!(ag
            .to_yaml()
            .expect("serialize agent yaml")
            .contains("Agent"));

        let wf = dispatch_resource(workflow_manifest("ty-wf")).expect("dispatch yaml workflow");
        assert!(wf
            .to_yaml()
            .expect("serialize workflow yaml")
            .contains("Workflow"));

        let pr = dispatch_resource(project_manifest("ty-pr", "d")).expect("dispatch yaml project");
        assert!(pr
            .to_yaml()
            .expect("serialize project yaml")
            .contains("Project"));

        let df = dispatch_resource(defaults_manifest("", "", "")).expect("dispatch yaml defaults");
        assert!(df
            .to_yaml()
            .expect("serialize defaults yaml")
            .contains("Defaults"));

        let rp =
            dispatch_resource(runtime_policy_manifest()).expect("dispatch yaml runtime policy");
        assert!(rp
            .to_yaml()
            .expect("serialize runtime policy yaml")
            .contains("RuntimePolicy"));
    }

    #[test]
    fn registered_resource_get_from_finds_defaults_and_runtime() {
        let config = make_config();
        let defaults = RegisteredResource::get_from(&config, "defaults");
        assert!(defaults.is_some());
        assert_eq!(
            defaults.expect("defaults resource should exist").kind(),
            ResourceKind::Defaults
        );

        let runtime = RegisteredResource::get_from(&config, "runtime");
        assert!(runtime.is_some());
        assert_eq!(
            runtime.expect("runtime policy should exist").kind(),
            ResourceKind::RuntimePolicy
        );
    }

    #[test]
    fn registered_resource_get_from_returns_none_for_unknown() {
        let config = make_config();
        assert!(RegisteredResource::get_from(&config, "no-such-resource-xyz").is_none());
    }

    #[test]
    fn registered_resource_delete_from_removes_workspace() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("rd-ws", "workspace/rd"))
            .expect("dispatch delete ws");
        ws.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-ws"));
        assert!(!config.workspaces.contains_key("rd-ws"));
    }

    #[test]
    fn registered_resource_delete_from_removes_agent() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("rd-ag", "cmd")).expect("dispatch delete agent");
        ag.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-ag"));
        assert!(!config.agents.contains_key("rd-ag"));
    }

    #[test]
    fn registered_resource_delete_from_removes_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("rd-wf")).expect("dispatch delete workflow");
        wf.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-wf"));
        assert!(!config.workflows.contains_key("rd-wf"));
    }

    #[test]
    fn registered_resource_delete_from_removes_project() {
        let mut config = make_config();
        let pr =
            dispatch_resource(project_manifest("rd-pr", "d")).expect("dispatch delete project");
        pr.apply(&mut config).expect("apply");
        assert!(RegisteredResource::delete_from(&mut config, "rd-pr"));
        assert!(!config.projects.contains_key("rd-pr"));
    }

    #[test]
    fn registered_resource_delete_from_returns_false_for_unknown() {
        let mut config = make_config();
        assert!(!RegisteredResource::delete_from(
            &mut config,
            "no-such-thing"
        ));
    }

    // ── resource_registry tests ─────────────────────────────────────

    // Moved to resource_registry_has_seven_entries above

    // ── metadata helpers ────────────────────────────────────────────

    #[test]
    fn metadata_with_name_creates_minimal_metadata() {
        let meta = metadata_with_name("test");
        assert_eq!(meta.name, "test");
        assert!(meta.project.is_none());
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }

    #[test]
    fn metadata_from_parts_creates_full_metadata() {
        let labels = Some([("k".to_string(), "v".to_string())].into());
        let annotations = Some([("a".to_string(), "b".to_string())].into());
        let meta = metadata_from_parts("test", Some("proj".to_string()), labels, annotations);
        assert_eq!(meta.name, "test");
        assert_eq!(meta.project.as_deref(), Some("proj"));
        assert!(meta.labels.is_some());
        assert!(meta.annotations.is_some());
    }

    // ── validate_resource_name ──────────────────────────────────────

    #[test]
    fn validate_resource_name_accepts_valid() {
        assert!(validate_resource_name("valid-name").is_ok());
        assert!(validate_resource_name("a").is_ok());
    }

    #[test]
    fn validate_resource_name_rejects_empty() {
        assert!(validate_resource_name("").is_err());
        assert!(validate_resource_name("  ").is_err());
    }

    // ── serializes_equal ────────────────────────────────────────────

    #[test]
    fn serializes_equal_compares_by_json_value() {
        assert!(serializes_equal(&42, &42));
        assert!(!serializes_equal(&42, &43));
        assert!(serializes_equal(&"hello", &"hello"));
        assert!(!serializes_equal(&"hello", &"world"));
    }

    // ── resource_to_yaml ─────────────────────────────────────────────

    #[test]
    fn resource_to_yaml() {
        let workspace = WorkspaceResource {
            metadata: ResourceMetadata {
                name: "yaml-roundtrip".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: WorkspaceSpec {
                root_path: "workspace/yaml-roundtrip".to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
            },
        };

        let yaml = workspace
            .to_yaml()
            .expect("workspace yaml should serialize");
        assert!(yaml.contains("apiVersion: orchestrator.dev/v2"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-roundtrip"));
        assert!(yaml.contains("root_path: workspace/yaml-roundtrip"));
    }

    // ── apply_to_store ──────────────────────────────────────────────

    #[test]
    fn apply_to_store_returns_created_for_new_resource() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/new".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-new");
        let result = apply_to_store(&mut config, "Workspace", "ws-new", &meta, ws.to_cr_spec());
        assert_eq!(result, ApplyResult::Created);
        assert!(config.resource_store.get("Workspace", "ws-new").is_some());
        assert!(
            config.workspaces.contains_key("ws-new"),
            "legacy field updated"
        );
    }

    #[test]
    fn apply_to_store_returns_unchanged_for_identical() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/same".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-same");
        let spec = ws.to_cr_spec();
        apply_to_store(&mut config, "Workspace", "ws-same", &meta, spec.clone());
        let result = apply_to_store(&mut config, "Workspace", "ws-same", &meta, spec);
        assert_eq!(result, ApplyResult::Unchanged);
    }

    #[test]
    fn apply_to_store_returns_configured_for_changed() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws1 = crate::config::WorkspaceConfig {
            root_path: "/v1".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/v2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-chg");
        apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws1.to_cr_spec());
        let result = apply_to_store(&mut config, "Workspace", "ws-chg", &meta, ws2.to_cr_spec());
        assert_eq!(result, ApplyResult::Configured);
        assert_eq!(config.workspaces.get("ws-chg").unwrap().root_path, "/v2");
    }

    #[test]
    fn apply_to_store_seeds_from_legacy_for_correct_change_detection() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        // Pre-populate legacy field without going through store
        config.workspaces.insert(
            "legacy-ws".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/legacy".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        assert!(config
            .resource_store
            .get("Workspace", "legacy-ws")
            .is_none());

        // Apply the identical resource — should return Unchanged because seed detects it
        let ws = crate::config::WorkspaceConfig {
            root_path: "/legacy".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("legacy-ws");
        let result = apply_to_store(
            &mut config,
            "Workspace",
            "legacy-ws",
            &meta,
            ws.to_cr_spec(),
        );
        assert_eq!(
            result,
            ApplyResult::Unchanged,
            "should seed from legacy and detect no change"
        );
    }

    #[test]
    fn apply_to_store_increments_generation() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/g".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-gen");
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws.to_cr_spec());
        let gen1 = config
            .resource_store
            .get("Workspace", "ws-gen")
            .unwrap()
            .generation;

        let ws2 = crate::config::WorkspaceConfig {
            root_path: "/g2".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        apply_to_store(&mut config, "Workspace", "ws-gen", &meta, ws2.to_cr_spec());
        let gen2 = config
            .resource_store
            .get("Workspace", "ws-gen")
            .unwrap()
            .generation;
        assert!(gen2 > gen1, "generation should increment on update");
    }

    // ── delete_from_store ───────────────────────────────────────────

    #[test]
    fn delete_from_store_removes_from_both_store_and_legacy() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/d".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_with_name("ws-del");
        apply_to_store(&mut config, "Workspace", "ws-del", &meta, ws.to_cr_spec());
        assert!(config.workspaces.contains_key("ws-del"));

        let removed = delete_from_store(&mut config, "Workspace", "ws-del");
        assert!(removed);
        assert!(config.resource_store.get("Workspace", "ws-del").is_none());
        assert!(!config.workspaces.contains_key("ws-del"));
    }

    #[test]
    fn delete_from_store_seeds_from_legacy_and_removes() {
        let mut config = OrchestratorConfig::default();
        // Only in legacy, not in store
        config.workspaces.insert(
            "legacy-del".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/ld".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            },
        );
        let removed = delete_from_store(&mut config, "Workspace", "legacy-del");
        assert!(removed, "should seed from legacy then remove");
        assert!(!config.workspaces.contains_key("legacy-del"));
    }

    #[test]
    fn delete_from_store_returns_false_for_missing() {
        let mut config = OrchestratorConfig::default();
        let removed = delete_from_store(&mut config, "Workspace", "no-such");
        assert!(!removed);
    }

    // ── metadata_from_store ─────────────────────────────────────────

    #[test]
    fn metadata_from_store_returns_cr_metadata() {
        use crate::crd::projection::CrdProjectable;
        let mut config = OrchestratorConfig::default();
        let ws = crate::config::WorkspaceConfig {
            root_path: "/m".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        };
        let meta = metadata_from_parts(
            "ws-meta",
            None,
            Some([("env".to_string(), "prod".to_string())].into()),
            Some([("note".to_string(), "hi".to_string())].into()),
        );
        apply_to_store(&mut config, "Workspace", "ws-meta", &meta, ws.to_cr_spec());

        let loaded = metadata_from_store(&config, "Workspace", "ws-meta");
        assert_eq!(loaded.labels.as_ref().unwrap().get("env").unwrap(), "prod");
        assert_eq!(
            loaded.annotations.as_ref().unwrap().get("note").unwrap(),
            "hi"
        );
    }

    #[test]
    fn metadata_from_store_falls_back_to_name_only() {
        let config = OrchestratorConfig::default();
        let meta = metadata_from_store(&config, "Workspace", "missing");
        assert_eq!(meta.name, "missing");
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }
}

// ── Shared test fixtures ──────────────────────────────────────────────────────

#[cfg(test)]
pub(super) mod test_fixtures {
    use crate::cli_types::{
        AgentSpec, DefaultsSpec, OrchestratorResource, ProjectSpec, ResourceKind, ResourceMetadata,
        ResourceSpec, ResumeSpec, RunnerSpec, RuntimePolicySpec, SafetySpec, StepTemplateSpec,
        WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowSpec,
        WorkflowStepSpec, WorkspaceSpec,
    };
    use crate::config::OrchestratorConfig;
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    use super::API_VERSION;

    pub fn make_config() -> OrchestratorConfig {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let active = read_active_config(&state).expect("state should be readable");
        active.config.clone()
    }

    pub fn workspace_manifest(name: &str, root_path: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: root_path.to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
            }),
        }
    }

    pub fn agent_manifest(name: &str, command: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Agent,
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Agent(Box::new(AgentSpec {
                command: command.to_string(),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            })),
        }
    }

    pub fn workflow_manifest(name: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workflow(WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    template: None,
                    builtin: None,
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                    max_parallel: None,
                    timeout_secs: None,
                    behavior: Default::default(),
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: Some(3),
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec {
                    rules: vec![WorkflowFinalizeRuleSpec {
                        id: "qa-passed".to_string(),
                        engine: "cel".to_string(),
                        when: "qa_exit_code == 0".to_string(),
                        status: "qa_passed".to_string(),
                        reason: Some("qa succeeded".to_string()),
                    }],
                },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
                max_parallel: None,
            }),
        }
    }

    pub fn project_manifest(name: &str, description: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Project,
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Project(ProjectSpec {
                description: Some(description.to_string()),
            }),
        }
    }

    pub fn defaults_manifest(
        project: &str,
        workspace: &str,
        workflow: &str,
    ) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Defaults,
            metadata: ResourceMetadata {
                name: "defaults".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Defaults(DefaultsSpec {
                project: project.to_string(),
                workspace: workspace.to_string(),
                workflow: workflow.to_string(),
            }),
        }
    }

    pub fn step_template_manifest(name: &str, prompt: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::StepTemplate,
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::StepTemplate(StepTemplateSpec {
                prompt: prompt.to_string(),
                description: None,
            }),
        }
    }

    pub fn runtime_policy_manifest() -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::RuntimePolicy,
            metadata: ResourceMetadata {
                name: "runtime".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::RuntimePolicy(RuntimePolicySpec {
                runner: RunnerSpec {
                    shell: "/bin/bash".to_string(),
                    shell_arg: "-lc".to_string(),
                    policy: "unsafe".to_string(),
                    executor: "shell".to_string(),
                    allowed_shells: vec![],
                    allowed_shell_args: vec![],
                    env_allowlist: vec![],
                    redaction_patterns: vec![],
                },
                resume: ResumeSpec { auto: false },
            }),
        }
    }
}

#[cfg(test)]
mod apply_to_project_tests {
    use super::test_fixtures::{
        agent_manifest, make_config, workflow_manifest, workspace_manifest,
    };
    use super::*;

    #[test]
    fn apply_to_project_routes_agent_to_project_scope() {
        let mut config = make_config();
        let resource =
            dispatch_resource(agent_manifest("proj-ag", "echo test")).expect("dispatch agent");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects.contains_key("my-qa"));
        assert!(config.projects["my-qa"].agents.contains_key("proj-ag"));
        // Should NOT be in global agents
        assert!(!config.agents.contains_key("proj-ag"));
    }

    #[test]
    fn apply_to_project_routes_workspace_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workspace_manifest("proj-ws", "workspace/proj"))
            .expect("dispatch ws");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workspaces.contains_key("proj-ws"));
        // Should NOT be in global workspaces
        assert!(!config.workspaces.contains_key("proj-ws"));
    }

    #[test]
    fn apply_to_project_routes_workflow_to_project_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(workflow_manifest("proj-wf")).expect("dispatch wf");
        let result = apply_to_project(&resource, &mut config, "my-qa").expect("apply");

        assert_eq!(result, ApplyResult::Created);
        assert!(config.projects["my-qa"].workflows.contains_key("proj-wf"));
        // Should NOT be in global workflows
        assert!(!config.workflows.contains_key("proj-wf"));
    }

    #[test]
    fn apply_to_project_auto_creates_project_entry() {
        let mut config = make_config();
        assert!(!config.projects.contains_key("auto-proj"));

        let resource =
            dispatch_resource(agent_manifest("auto-ag", "echo auto")).expect("dispatch agent");
        apply_to_project(&resource, &mut config, "auto-proj").expect("apply");

        assert!(config.projects.contains_key("auto-proj"));
    }

    #[test]
    fn apply_to_project_returns_unchanged_for_identical() {
        let mut config = make_config();
        let resource =
            dispatch_resource(agent_manifest("dup-ag", "echo dup")).expect("dispatch agent");

        assert_eq!(
            apply_to_project(&resource, &mut config, "dup-proj").expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            apply_to_project(&resource, &mut config, "dup-proj").expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn apply_to_project_singleton_defaults_goes_to_global() {
        use super::test_fixtures::defaults_manifest;
        let mut config = make_config();
        let resource =
            dispatch_resource(defaults_manifest("p", "w", "f")).expect("dispatch defaults");
        // Singletons fall through to global apply
        let result = apply_to_project(&resource, &mut config, "proj-singleton").expect("apply");
        assert!(matches!(
            result,
            ApplyResult::Created | ApplyResult::Configured | ApplyResult::Unchanged
        ));
        // Defaults were applied to global config (project field updated)
        assert_eq!(config.defaults.project, "p");
    }
}
