use crate::cli_types::{
    AgentMetadataSpec, AgentSpec, AgentTemplatesSpec, OrchestratorResource, ResourceKind,
    ResourceMetadata, ResourceSpec, WorkflowFinalizeRuleSpec, WorkflowFinalizeSpec,
    WorkflowLoopSpec, WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec, WorkspaceSpec,
};
use crate::{
    AgentConfig, AgentMetadata, AgentPreference, AgentSelectionConfig, LoopMode,
    OrchestratorConfig, StepHookEngine, StepPrehookConfig, WorkflowConfig, WorkflowFinalizeConfig,
    WorkflowFinalizeRule, WorkflowLoopConfig, WorkflowStepConfig, WorkflowStepType,
    WorkspaceConfig,
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
pub struct WorkflowResource {
    pub metadata: ResourceMetadata,
    pub spec: WorkflowSpec,
}

#[derive(Debug, Clone)]
pub enum RegisteredResource {
    Workspace(WorkspaceResource),
    Agent(AgentResource),
    Workflow(WorkflowResource),
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRegistration {
    pub kind: ResourceKind,
    pub build: fn(OrchestratorResource) -> Result<RegisteredResource>,
}

pub fn resource_registry() -> [ResourceRegistration; 3] {
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

fn validate_resource_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("metadata.name cannot be empty"));
    }
    Ok(())
}

fn metadata_with_name(name: &str) -> ResourceMetadata {
    ResourceMetadata {
        name: name.to_string(),
        labels: None,
        annotations: None,
    }
}

fn manifest_yaml(
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

impl Resource for WorkspaceResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Workspace
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())?;
        if self.spec.root_path.trim().is_empty() {
            return Err(anyhow!("workspace.spec.root_path cannot be empty"));
        }
        if self.spec.ticket_dir.trim().is_empty() {
            return Err(anyhow!("workspace.spec.ticket_dir cannot be empty"));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = workspace_spec_to_config(&self.spec);
        apply_to_map(&mut config.workspaces, self.name(), incoming)
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Workspace,
            &self.metadata,
            ResourceSpec::Workspace(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.workspaces.get(name).map(|workspace| Self {
            metadata: metadata_with_name(name),
            spec: workspace_config_to_spec(workspace),
        })
    }
}

impl Resource for AgentResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Agent
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())?;
        let templates = &self.spec.templates;
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
        for value in [
            templates.init_once.as_deref(),
            templates.qa.as_deref(),
            templates.fix.as_deref(),
            templates.retest.as_deref(),
            templates.loop_guard.as_deref(),
        ] {
            if matches!(value, Some(raw) if raw.trim().is_empty()) {
                return Err(anyhow!(
                    "agent.spec.templates entries cannot be empty strings"
                ));
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = agent_spec_to_config(&self.spec);
        apply_to_map(&mut config.agents, self.name(), incoming)
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Agent,
            &self.metadata,
            ResourceSpec::Agent(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.agents.get(name).map(|agent| Self {
            metadata: metadata_with_name(name),
            spec: agent_config_to_spec(agent),
        })
    }
}

impl Resource for WorkflowResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Workflow
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())?;
        if self.spec.steps.is_empty() {
            return Err(anyhow!("workflow.spec.steps cannot be empty"));
        }
        if self.spec.steps.iter().any(|step| step.id.trim().is_empty()) {
            return Err(anyhow!("workflow.spec.steps[].id cannot be empty"));
        }
        if self
            .spec
            .steps
            .iter()
            .any(|step| step.step_type.trim().is_empty())
        {
            return Err(anyhow!("workflow.spec.steps[].type cannot be empty"));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = workflow_spec_to_config(&self.spec);
        apply_to_map(&mut config.workflows, self.name(), incoming)
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Workflow,
            &self.metadata,
            ResourceSpec::Workflow(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.workflows.get(name).map(|workflow| Self {
            metadata: metadata_with_name(name),
            spec: workflow_config_to_spec(workflow),
        })
    }
}

impl Resource for RegisteredResource {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Agent(_) => ResourceKind::Agent,
            Self::Workflow(_) => ResourceKind::Workflow,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Workspace(resource) => &resource.metadata.name,
            Self::Agent(resource) => &resource.metadata.name,
            Self::Workflow(resource) => &resource.metadata.name,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Workspace(resource) => resource.validate(),
            Self::Agent(resource) => resource.validate(),
            Self::Workflow(resource) => resource.validate(),
        }
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        match self {
            Self::Workspace(resource) => resource.apply(config),
            Self::Agent(resource) => resource.apply(config),
            Self::Workflow(resource) => resource.apply(config),
        }
    }

    fn to_yaml(&self) -> Result<String> {
        match self {
            Self::Workspace(resource) => resource.to_yaml(),
            Self::Agent(resource) => resource.to_yaml(),
            Self::Workflow(resource) => resource.to_yaml(),
        }
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        if let Some(workspace) = WorkspaceResource::get_from(config, name) {
            return Some(Self::Workspace(workspace));
        }
        if let Some(agent) = AgentResource::get_from(config, name) {
            return Some(Self::Agent(agent));
        }
        if let Some(workflow) = WorkflowResource::get_from(config, name) {
            return Some(Self::Workflow(workflow));
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
    let capabilities = vec![
        spec.templates.init_once.as_ref().map(|_| "init_once"),
        spec.templates.qa.as_ref().map(|_| "qa"),
        spec.templates.fix.as_ref().map(|_| "fix"),
        spec.templates.retest.as_ref().map(|_| "retest"),
        spec.templates.loop_guard.as_ref().map(|_| "loop_guard"),
    ]
    .into_iter()
    .flatten()
    .map(|s| s.to_string())
    .collect();

    let mut templates = std::collections::HashMap::new();
    if let Some(t) = &spec.templates.init_once {
        templates.insert("init_once".to_string(), t.clone());
    }
    if let Some(t) = &spec.templates.qa {
        templates.insert("qa".to_string(), t.clone());
    }
    if let Some(t) = &spec.templates.fix {
        templates.insert("fix".to_string(), t.clone());
    }
    if let Some(t) = &spec.templates.retest {
        templates.insert("retest".to_string(), t.clone());
    }
    if let Some(t) = &spec.templates.loop_guard {
        templates.insert("loop_guard".to_string(), t.clone());
    }

    AgentConfig {
        metadata: AgentMetadata::default(),
        capabilities,
        templates,
        preference: AgentPreference::default(),
        selection: AgentSelectionConfig::default(),
    }
}

fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        templates: AgentTemplatesSpec {
            init_once: config.templates.get("init_once").cloned(),
            qa: config.templates.get("qa").cloned(),
            fix: config.templates.get("fix").cloned(),
            retest: config.templates.get("retest").cloned(),
            loop_guard: config.templates.get("loop_guard").cloned(),
        },
        capabilities: if config.capabilities.is_empty() {
            None
        } else {
            Some(config.capabilities.clone())
        },
        metadata: if config.metadata.description.is_empty() && config.metadata.cost.is_none() {
            None
        } else {
            Some(AgentMetadataSpec {
                cost: config.metadata.cost,
                description: if config.metadata.description.is_empty() {
                    None
                } else {
                    Some(config.metadata.description.clone())
                },
            })
        },
    }
}

fn workflow_spec_to_config(spec: &WorkflowSpec) -> WorkflowConfig {
    let steps = spec
        .steps
        .iter()
        .map(|step| WorkflowStepConfig {
            id: step.id.clone(),
            description: None,
            step_type: Some(
                parse_workflow_step_type(&step.step_type).unwrap_or(WorkflowStepType::Qa),
            ),
            required_capability: None,
            builtin: None,
            enabled: step.enabled,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
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
            step_type: step
                .step_type
                .as_ref()
                .map(|t| t.as_str().to_string())
                .unwrap_or_default(),
            enabled: step.enabled,
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

    fn agent_manifest(name: &str, qa_command: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Agent,
            metadata: ResourceMetadata {
                name: name.to_string(),
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Agent(AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    qa: Some(qa_command.to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                },
                capabilities: None,
                metadata: None,
            }),
        }
    }

    fn workflow_manifest(name: &str) -> OrchestratorResource {
        OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: ResourceMetadata {
                name: name.to_string(),
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Workflow(WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    enabled: true,
                    prehook: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: Some(3),
                },
                finalize: WorkflowFinalizeSpec {
                    rules: vec![WorkflowFinalizeRuleSpec {
                        id: "qa-passed".to_string(),
                        when: "qa_exit_code == 0".to_string(),
                        status: "qa_passed".to_string(),
                        reason: Some("qa succeeded".to_string()),
                    }],
                },
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
                capabilities: None,
                metadata: None,
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
    fn workspace_resource_apply() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = crate::read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource =
            dispatch_resource(workspace_manifest("ws-roundtrip", "workspace/ws-roundtrip"))
                .expect("workspace dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = WorkspaceResource::get_from(&config, "ws-roundtrip")
            .expect("workspace should be present in config");
        assert_eq!(loaded.spec.root_path, "workspace/ws-roundtrip");
        assert_eq!(loaded.kind(), ResourceKind::Workspace);
    }

    #[test]
    fn agent_resource_apply() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = crate::read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(agent_manifest("agent-roundtrip", "cargo test"))
            .expect("agent dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = AgentResource::get_from(&config, "agent-roundtrip")
            .expect("agent should be present in config");
        assert_eq!(loaded.spec.templates.qa.as_deref(), Some("cargo test"));
        assert_eq!(loaded.kind(), ResourceKind::Agent);
    }

    #[test]
    fn workflow_resource_roundtrip() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = crate::read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(workflow_manifest("wf-roundtrip"))
            .expect("workflow dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = WorkflowResource::get_from(&config, "wf-roundtrip")
            .expect("workflow should be present in config");
        assert_eq!(loaded.spec.steps.len(), 1);
        assert_eq!(loaded.spec.steps[0].step_type, "qa");
        assert_eq!(loaded.spec.loop_policy.mode, "once");
        assert_eq!(loaded.spec.loop_policy.max_cycles, Some(3));
    }

    #[test]
    fn resource_to_yaml() {
        let workspace = WorkspaceResource {
            metadata: ResourceMetadata {
                name: "yaml-roundtrip".to_string(),
                labels: None,
                annotations: None,
            },
            spec: WorkspaceSpec {
                root_path: "workspace/yaml-roundtrip".to_string(),
                qa_targets: vec!["docs/qa".to_string()],
                ticket_dir: "docs/ticket".to_string(),
            },
        };

        let yaml = workspace
            .to_yaml()
            .expect("workspace yaml should serialize");
        assert!(yaml.contains("apiVersion: orchestrator.dev/v1"));
        assert!(yaml.contains("kind: Workspace"));
        assert!(yaml.contains("name: yaml-roundtrip"));
        assert!(yaml.contains("root_path: workspace/yaml-roundtrip"));
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

#[test]
fn parse_workflow_step_type_valid() {
    assert_eq!(
        parse_workflow_step_type("init_once").unwrap(),
        WorkflowStepType::InitOnce
    );
    assert_eq!(
        parse_workflow_step_type("qa").unwrap(),
        WorkflowStepType::Qa
    );
    assert_eq!(
        parse_workflow_step_type("ticket_scan").unwrap(),
        WorkflowStepType::TicketScan
    );
    assert_eq!(
        parse_workflow_step_type("fix").unwrap(),
        WorkflowStepType::Fix
    );
    assert_eq!(
        parse_workflow_step_type("retest").unwrap(),
        WorkflowStepType::Retest
    );
}

#[test]
fn parse_workflow_step_type_invalid() {
    let result = parse_workflow_step_type("unknown");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("unknown workflow step type"));
}

#[test]
fn parse_loop_mode_infinite() {
    let mode = parse_loop_mode("infinite");
    match mode {
        LoopMode::Infinite => (), // pass
        _ => panic!("expected Infinite"),
    }
}

#[test]
fn parse_loop_mode_default() {
    let mode1 = parse_loop_mode("once");
    let mode2 = parse_loop_mode("anything_else");
    match (mode1, mode2) {
        (LoopMode::Once, LoopMode::Once) => (), // pass
        _ => panic!("expected Once for both"),
    }
}

#[test]
fn loop_mode_as_str_returns_correct_values() {
    assert_eq!(loop_mode_as_str(&LoopMode::Once), "once");
    assert_eq!(loop_mode_as_str(&LoopMode::Infinite), "infinite");
}

#[test]
fn agent_validation_rejects_empty_templates() {
    let agent = AgentResource {
        metadata: ResourceMetadata {
            name: "test-agent".to_string(),
            labels: None,
            annotations: None,
        },
        spec: AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: None,
                qa: None,
                fix: None,
                retest: None,
                loop_guard: None,
            },
            capabilities: None,
            metadata: None,
        },
    };
    let result = agent.validate();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("at least one template"));
}

#[test]
fn workflow_validation_rejects_empty_steps() {
    let workflow = WorkflowResource {
        metadata: ResourceMetadata {
            name: "test-workflow".to_string(),
            labels: None,
            annotations: None,
        },
        spec: WorkflowSpec {
            steps: vec![],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
        },
    };
    let result = workflow.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));
}

#[test]
fn workspace_to_yaml_includes_all_fields() {
    let workspace = WorkspaceResource {
        metadata: ResourceMetadata {
            name: "full-workspace".to_string(),
            labels: Some([("env".to_string(), "test".to_string())].into()),
            annotations: Some([("desc".to_string(), "test workspace".to_string())].into()),
        },
        spec: WorkspaceSpec {
            root_path: "/path/to/workspace".to_string(),
            qa_targets: vec!["docs/qa".to_string(), "tests".to_string()],
            ticket_dir: "tickets".to_string(),
        },
    };
    let yaml = workspace.to_yaml().expect("should serialize");
    assert!(yaml.contains("full-workspace"));
    assert!(yaml.contains("/path/to/workspace"));
    assert!(yaml.contains("docs/qa"));
    assert!(yaml.contains("tickets"));
}

#[test]
fn agent_to_yaml_includes_templates() {
    let agent = AgentResource {
        metadata: ResourceMetadata {
            name: "full-agent".to_string(),
            labels: None,
            annotations: None,
        },
        spec: AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: Some("init".to_string()),
                qa: Some("test".to_string()),
                fix: Some("fix".to_string()),
                retest: Some("retest".to_string()),
                loop_guard: Some("guard".to_string()),
            },
            capabilities: None,
            metadata: None,
        },
    };
    let yaml = agent.to_yaml().expect("should serialize");
    assert!(yaml.contains("full-agent"));
    assert!(yaml.contains("init"));
    assert!(yaml.contains("test"));
    assert!(yaml.contains("fix"));
    assert!(yaml.contains("retest"));
    assert!(yaml.contains("guard"));
}
