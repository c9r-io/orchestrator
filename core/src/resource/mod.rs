use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceMetadata, ResourceSpec};
use crate::config::OrchestratorConfig;
use anyhow::{anyhow, Result};
use serde::Serialize;

pub(crate) const API_VERSION: &str = "orchestrator.dev/v2";

// ── Submodules ────────────────────────────────────────────────────────────────

mod agent;
mod defaults;
mod export;
mod parse;
mod project;
mod runtime_policy;
mod step_template;
mod workflow;
mod workspace;

// ── Re-exports ────────────────────────────────────────────────────────────────

pub use agent::AgentResource;
pub use defaults::DefaultsResource;
pub use export::{export_manifest_documents, export_manifest_resources};
pub use parse::{delete_resource_by_kind, kind_as_str, parse_resources_from_yaml};
pub use project::ProjectResource;
pub use runtime_policy::RuntimePolicyResource;
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
    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult;
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
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRegistration {
    pub kind: ResourceKind,
    pub build: fn(OrchestratorResource) -> Result<RegisteredResource>,
}

pub fn resource_registry() -> [ResourceRegistration; 7] {
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
    ]
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
    Ok(serde_yaml::to_string(&manifest)?)
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
        }
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        match self {
            Self::Workspace(resource) => resource.apply(config),
            Self::Agent(resource) => resource.apply(config),
            Self::Workflow(resource) => resource.apply(config),
            Self::Project(resource) => resource.apply(config),
            Self::Defaults(resource) => resource.apply(config),
            Self::RuntimePolicy(resource) => resource.apply(config),
            Self::StepTemplate(resource) => resource.apply(config),
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
        None
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        if config.workspaces.remove(name).is_some() {
            config.resource_meta.workspaces.remove(name);
            return true;
        }
        if config.agents.remove(name).is_some() {
            config.resource_meta.agents.remove(name);
            return true;
        }
        if config.workflows.remove(name).is_some() {
            config.resource_meta.workflows.remove(name);
            return true;
        }
        if config.projects.remove(name).is_some() {
            return true;
        }
        if config.step_templates.remove(name).is_some() {
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
            })),
        };

        let error = dispatch_resource(resource).expect_err("dispatch should fail");
        assert!(error.to_string().contains("mismatch"));
    }

    #[test]
    fn resource_registry_has_seven_entries() {
        let registry = resource_registry();
        assert_eq!(registry.len(), 7);
        let kinds: Vec<ResourceKind> = registry.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::StepTemplate));
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
        let result = resource.apply(&mut config);

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
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);
        assert_eq!(resource.apply(&mut config), ApplyResult::Unchanged);
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
        assert_eq!(initial.apply(&mut config), ApplyResult::Created);

        let updated = dispatch_resource(workspace_manifest("change-ws", "workspace/v2"))
            .expect("dispatch should succeed");
        assert_eq!(updated.apply(&mut config), ApplyResult::Configured);
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
        ws.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-ws"));
        assert!(!config.workspaces.contains_key("rd-ws"));
    }

    #[test]
    fn registered_resource_delete_from_removes_agent() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("rd-ag", "cmd")).expect("dispatch delete agent");
        ag.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-ag"));
        assert!(!config.agents.contains_key("rd-ag"));
    }

    #[test]
    fn registered_resource_delete_from_removes_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("rd-wf")).expect("dispatch delete workflow");
        wf.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-wf"));
        assert!(!config.workflows.contains_key("rd-wf"));
    }

    #[test]
    fn registered_resource_delete_from_removes_project() {
        let mut config = make_config();
        let pr =
            dispatch_resource(project_manifest("rd-pr", "d")).expect("dispatch delete project");
        pr.apply(&mut config);
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
}

// ── Shared test fixtures ──────────────────────────────────────────────────────

#[cfg(test)]
pub(super) mod test_fixtures {
    use crate::cli_types::{
        AgentSpec, DefaultsSpec, OrchestratorResource, ProjectSpec,
        ResourceKind, ResourceMetadata, ResourceSpec, ResumeSpec, RunnerSpec, RuntimePolicySpec,
        SafetySpec, StepTemplateSpec, WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowSpec,
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
