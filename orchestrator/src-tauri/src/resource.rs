use crate::cli_types::{
    AgentGroupSpec, AgentSpec, AgentTemplatesSpec, OrchestratorResource, ResourceKind,
    ResourceMetadata, ResourceSpec, WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec,
    WorkflowLoopSpec, WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec, WorkspaceSpec,
};
use crate::{
    AgentConfig, AgentGroupConfig, AgentTemplates, LoopMode, OrchestratorConfig, StepHookEngine,
    StepPrehookConfig, WorkflowConfig, WorkflowFinalizeConfig, WorkflowFinalizeRule,
    WorkflowLoopConfig, WorkflowStepConfig, WorkflowStepType, WorkspaceConfig,
};
use anyhow::{anyhow, Result};
use serde::Serialize;

const API_VERSION: &str = "orchestrator.dev/v1";

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
}

#[derive(Debug, Clone)]
pub struct WorkspaceResource {
    pub metadata: ResourceMetadata,
    pub spec: WorkspaceSpec,
}

#[derive(Debug, Clone)]
pub struct AgentResource {
    pub metadata: ResourceMetadata,
    pub spec: AgentSpec,
}

#[derive(Debug, Clone)]
pub struct AgentGroupResource {
    pub metadata: ResourceMetadata,
    pub spec: AgentGroupSpec,
}

#[derive(Debug, Clone)]
pub struct WorkflowResource {
    pub metadata: ResourceMetadata,
    pub spec: WorkflowSpec,
}

#[derive(Debug, Clone)]
pub enum RegisteredResource {
    Workspace(WorkspaceResource),
    Agent(AgentResource),
    AgentGroup(AgentGroupResource),
    Workflow(WorkflowResource),
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRegistration {
    pub kind: ResourceKind,
    pub build: fn(OrchestratorResource) -> Result<RegisteredResource>,
}

pub fn resource_registry() -> [ResourceRegistration; 4] {
    [
        ResourceRegistration {
            kind: ResourceKind::Workspace,
            build: build_workspace,
        },
        ResourceRegistration {
            kind: ResourceKind::Agent,
            build: build_agent,
        },
        ResourceRegistration {
            kind: ResourceKind::AgentGroup,
            build: build_agent_group,
        },
        ResourceRegistration {
            kind: ResourceKind::Workflow,
            build: build_workflow,
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

impl Resource for RegisteredResource {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Agent(_) => ResourceKind::Agent,
            Self::AgentGroup(_) => ResourceKind::AgentGroup,
            Self::Workflow(_) => ResourceKind::Workflow,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Workspace(resource) => &resource.metadata.name,
            Self::Agent(resource) => &resource.metadata.name,
            Self::AgentGroup(resource) => &resource.metadata.name,
            Self::Workflow(resource) => &resource.metadata.name,
        }
    }

    fn validate(&self) -> Result<()> {
        if self.name().trim().is_empty() {
            return Err(anyhow!("metadata.name cannot be empty"));
        }

        match self {
            Self::Workspace(resource) => {
                if resource.spec.root_path.trim().is_empty() {
                    return Err(anyhow!("workspace.spec.root_path cannot be empty"));
                }
                if resource.spec.ticket_dir.trim().is_empty() {
                    return Err(anyhow!("workspace.spec.ticket_dir cannot be empty"));
                }
            }
            Self::Agent(resource) => {
                let templates = &resource.spec.templates;
                if templates.init_once.is_none()
                    && templates.qa.is_none()
                    && templates.fix.is_none()
                    && templates.retest.is_none()
                    && templates.loop_guard.is_none()
                {
                    return Err(anyhow!(
                        "agent.spec.templates must define at least one template"
                    ));
                }
            }
            Self::AgentGroup(resource) => {
                if resource.spec.agents.is_empty() {
                    return Err(anyhow!("agent_group.spec.agents cannot be empty"));
                }
            }
            Self::Workflow(resource) => {
                if resource.spec.steps.is_empty() {
                    return Err(anyhow!("workflow.spec.steps cannot be empty"));
                }
            }
        }

        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        match self {
            Self::Workspace(resource) => {
                let incoming = workspace_spec_to_config(&resource.spec);
                apply_to_map(&mut config.workspaces, &resource.metadata.name, incoming)
            }
            Self::Agent(resource) => {
                let incoming = agent_spec_to_config(&resource.spec);
                apply_to_map(&mut config.agents, &resource.metadata.name, incoming)
            }
            Self::AgentGroup(resource) => {
                let incoming = agent_group_spec_to_config(&resource.spec);
                apply_to_map(&mut config.agent_groups, &resource.metadata.name, incoming)
            }
            Self::Workflow(resource) => {
                let incoming = workflow_spec_to_config(&resource.spec);
                apply_to_map(&mut config.workflows, &resource.metadata.name, incoming)
            }
        }
    }

    fn to_yaml(&self) -> Result<String> {
        let manifest = match self {
            Self::Workspace(resource) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workspace,
                metadata: resource.metadata.clone(),
                spec: ResourceSpec::Workspace(resource.spec.clone()),
            },
            Self::Agent(resource) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Agent,
                metadata: resource.metadata.clone(),
                spec: ResourceSpec::Agent(resource.spec.clone()),
            },
            Self::AgentGroup(resource) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::AgentGroup,
                metadata: resource.metadata.clone(),
                spec: ResourceSpec::AgentGroup(resource.spec.clone()),
            },
            Self::Workflow(resource) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workflow,
                metadata: resource.metadata.clone(),
                spec: ResourceSpec::Workflow(resource.spec.clone()),
            },
        };
        Ok(serde_yaml::to_string(&manifest)?)
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        if let Some(workspace) = config.workspaces.get(name) {
            return Some(Self::Workspace(WorkspaceResource {
                metadata: ResourceMetadata {
                    name: name.to_string(),
                    labels: None,
                    annotations: None,
                },
                spec: workspace_config_to_spec(workspace),
            }));
        }
        if let Some(agent) = config.agents.get(name) {
            return Some(Self::Agent(AgentResource {
                metadata: ResourceMetadata {
                    name: name.to_string(),
                    labels: None,
                    annotations: None,
                },
                spec: agent_config_to_spec(agent),
            }));
        }
        if let Some(agent_group) = config.agent_groups.get(name) {
            return Some(Self::AgentGroup(AgentGroupResource {
                metadata: ResourceMetadata {
                    name: name.to_string(),
                    labels: None,
                    annotations: None,
                },
                spec: agent_group_config_to_spec(agent_group),
            }));
        }
        if let Some(workflow) = config.workflows.get(name) {
            return Some(Self::Workflow(WorkflowResource {
                metadata: ResourceMetadata {
                    name: name.to_string(),
                    labels: None,
                    annotations: None,
                },
                spec: workflow_config_to_spec(workflow),
            }));
        }
        None
    }
}

fn apply_to_map<T: Clone + Serialize>(
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

fn serializes_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

fn build_workspace(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Workspace {
        return Err(anyhow!("resource kind/spec mismatch for Workspace"));
    }
    match spec {
        ResourceSpec::Workspace(spec) => Ok(RegisteredResource::Workspace(WorkspaceResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Workspace")),
    }
}

fn build_agent(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Agent {
        return Err(anyhow!("resource kind/spec mismatch for Agent"));
    }
    match spec {
        ResourceSpec::Agent(spec) => {
            Ok(RegisteredResource::Agent(AgentResource { metadata, spec }))
        }
        _ => Err(anyhow!("resource kind/spec mismatch for Agent")),
    }
}

fn build_agent_group(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::AgentGroup {
        return Err(anyhow!("resource kind/spec mismatch for AgentGroup"));
    }
    match spec {
        ResourceSpec::AgentGroup(spec) => Ok(RegisteredResource::AgentGroup(AgentGroupResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for AgentGroup")),
    }
}

fn build_workflow(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Workflow {
        return Err(anyhow!("resource kind/spec mismatch for Workflow"));
    }
    match spec {
        ResourceSpec::Workflow(spec) => Ok(RegisteredResource::Workflow(WorkflowResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Workflow")),
    }
}

fn workspace_spec_to_config(spec: &WorkspaceSpec) -> WorkspaceConfig {
    WorkspaceConfig {
        root_path: spec.root_path.clone(),
        qa_targets: spec.qa_targets.clone(),
        ticket_dir: spec.ticket_dir.clone(),
    }
}

fn workspace_config_to_spec(config: &WorkspaceConfig) -> WorkspaceSpec {
    WorkspaceSpec {
        root_path: config.root_path.clone(),
        qa_targets: config.qa_targets.clone(),
        ticket_dir: config.ticket_dir.clone(),
    }
}

fn agent_spec_to_config(spec: &AgentSpec) -> AgentConfig {
    AgentConfig {
        templates: AgentTemplates {
            init_once: spec.templates.init_once.clone(),
            qa: spec.templates.qa.clone(),
            fix: spec.templates.fix.clone(),
            retest: spec.templates.retest.clone(),
            loop_guard: spec.templates.loop_guard.clone(),
        },
    }
}

fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        templates: AgentTemplatesSpec {
            init_once: config.templates.init_once.clone(),
            qa: config.templates.qa.clone(),
            fix: config.templates.fix.clone(),
            retest: config.templates.retest.clone(),
            loop_guard: config.templates.loop_guard.clone(),
        },
    }
}

fn agent_group_spec_to_config(spec: &AgentGroupSpec) -> AgentGroupConfig {
    AgentGroupConfig {
        agents: spec.agents.clone(),
    }
}

fn agent_group_config_to_spec(config: &AgentGroupConfig) -> AgentGroupSpec {
    AgentGroupSpec {
        agents: config.agents.clone(),
    }
}

fn workflow_spec_to_config(spec: &WorkflowSpec) -> WorkflowConfig {
    let steps = spec
        .steps
        .iter()
        .map(|step| WorkflowStepConfig {
            id: step.id.clone(),
            step_type: parse_workflow_step_type(&step.step_type).unwrap_or(WorkflowStepType::Qa),
            enabled: step.enabled,
            agent_group_id: step.agent_group_id.clone(),
            prehook: step.prehook.as_ref().map(|prehook| StepPrehookConfig {
                engine: StepHookEngine::Cel,
                when: prehook.when.clone(),
                reason: prehook.reason.clone(),
                ui: None,
            }),
        })
        .collect();

    let loop_policy = WorkflowLoopConfig {
        mode: parse_loop_mode(&spec.loop_policy.mode),
        guard: crate::WorkflowLoopGuardConfig {
            max_cycles: spec.loop_policy.max_cycles,
            ..Default::default()
        },
    };

    let finalize = WorkflowFinalizeConfig {
        rules: spec
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRule {
                id: rule.id.clone(),
                engine: StepHookEngine::Cel,
                when: rule.when.clone(),
                status: rule.status.clone(),
                reason: rule.reason.clone(),
            })
            .collect(),
    };

    WorkflowConfig {
        steps,
        loop_policy,
        finalize,
        qa: None,
        fix: None,
        retest: None,
    }
}

fn workflow_config_to_spec(config: &WorkflowConfig) -> WorkflowSpec {
    let steps = config
        .steps
        .iter()
        .map(|step| WorkflowStepSpec {
            id: step.id.clone(),
            step_type: step.step_type.as_str().to_string(),
            enabled: step.enabled,
            agent_group_id: step.agent_group_id.clone(),
            prehook: step.prehook.as_ref().map(|prehook| WorkflowPrehookSpec {
                when: prehook.when.clone(),
                reason: prehook.reason.clone(),
            }),
        })
        .collect();

    let loop_policy = WorkflowLoopSpec {
        mode: loop_mode_as_str(&config.loop_policy.mode).to_string(),
        max_cycles: config.loop_policy.guard.max_cycles,
    };

    let finalize = WorkflowFinalizeSpec {
        rules: config
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRuleSpec {
                id: rule.id.clone(),
                when: rule.when.clone(),
                status: rule.status.clone(),
                reason: rule.reason.clone(),
            })
            .collect(),
    };

    WorkflowSpec {
        steps,
        loop_policy,
        finalize,
    }
}

fn parse_workflow_step_type(value: &str) -> Result<WorkflowStepType> {
    match value {
        "init_once" => Ok(WorkflowStepType::InitOnce),
        "qa" => Ok(WorkflowStepType::Qa),
        "ticket_scan" => Ok(WorkflowStepType::TicketScan),
        "fix" => Ok(WorkflowStepType::Fix),
        "retest" => Ok(WorkflowStepType::Retest),
        _ => Err(anyhow!("unknown workflow step type: {}", value)),
    }
}

fn parse_loop_mode(value: &str) -> LoopMode {
    match value {
        "infinite" => LoopMode::Infinite,
        _ => LoopMode::Once,
    }
}

fn loop_mode_as_str(mode: &LoopMode) -> &'static str {
    match mode {
        LoopMode::Once => "once",
        LoopMode::Infinite => "infinite",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    fn workspace_manifest(name: &str, root_path: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: name.to_string(),
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: root_path.to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
            }),
        }
    }

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
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Agent(AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    qa: Some("run".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                },
            }),
        };

        let error = dispatch_resource(resource).expect_err("dispatch should fail");
        assert!(error.to_string().contains("mismatch"));
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
        assert!(yaml.contains("apiVersion: orchestrator.dev/v1"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-ws"));
    }

    #[test]
    fn resource_trait_get_from_reads_existing_config() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let active = crate::read_active_config(&state).expect("state should be readable");
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
            let active = crate::read_active_config(&state).expect("state should be readable");
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
            let active = crate::read_active_config(&state).expect("state should be readable");
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
            let active = crate::read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let initial = dispatch_resource(workspace_manifest("change-ws", "workspace/v1"))
            .expect("dispatch should succeed");
        assert_eq!(initial.apply(&mut config), ApplyResult::Created);

        let updated = dispatch_resource(workspace_manifest("change-ws", "workspace/v2"))
            .expect("dispatch should succeed");
        assert_eq!(updated.apply(&mut config), ApplyResult::Configured);
    }
}
