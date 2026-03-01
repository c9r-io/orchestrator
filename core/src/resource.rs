use crate::cli_types::{
    AgentMetadataSpec, AgentSelectionSpec, AgentSpec, AgentTemplatesSpec, DefaultsSpec,
    DynamicStepSpec, OrchestratorResource, ProjectSpec, ResourceKind, ResourceMetadata,
    ResourceSpec, ResumeSpec, RunnerSpec, RuntimePolicySpec, SafetySpec, WorkflowFinalizeRuleSpec,
    WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowPrehookSpec, WorkflowSpec, WorkflowStepSpec,
    WorkspaceSpec,
};
use crate::config::{
    AgentConfig, AgentMetadata, AgentSelectionConfig, ConfigDefaults, CostPreference, LoopMode,
    OrchestratorConfig, ProjectConfig, ResumeConfig, RunnerConfig, RunnerExecutorKind,
    RunnerPolicy, StepHookEngine, StepPrehookConfig, StepPrehookUiConfig, WorkflowConfig,
    WorkflowFinalizeConfig, WorkflowFinalizeRule, WorkflowLoopConfig, WorkflowLoopGuardConfig,
    StepBehavior, StepScope, WorkflowStepConfig, WorkspaceConfig,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const API_VERSION: &str = "orchestrator.dev/v2";

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
pub struct ProjectResource {
    pub metadata: ResourceMetadata,
    pub spec: ProjectSpec,
}

#[derive(Debug, Clone)]
pub struct DefaultsResource {
    pub metadata: ResourceMetadata,
    pub spec: DefaultsSpec,
}

#[derive(Debug, Clone)]
pub struct RuntimePolicyResource {
    pub metadata: ResourceMetadata,
    pub spec: RuntimePolicySpec,
}

#[derive(Debug, Clone)]
pub enum RegisteredResource {
    Workspace(WorkspaceResource),
    Agent(Box<AgentResource>),
    Workflow(WorkflowResource),
    Project(ProjectResource),
    Defaults(DefaultsResource),
    RuntimePolicy(RuntimePolicyResource),
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceRegistration {
    pub kind: ResourceKind,
    pub build: fn(OrchestratorResource) -> Result<RegisteredResource>,
}

pub fn resource_registry() -> [ResourceRegistration; 6] {
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
        ResourceRegistration {
            kind: ResourceKind::Project,
            build: build_project,
        },
        ResourceRegistration {
            kind: ResourceKind::Defaults,
            build: build_defaults,
        },
        ResourceRegistration {
            kind: ResourceKind::RuntimePolicy,
            build: build_runtime_policy,
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
        project: None,
        labels: None,
        annotations: None,
    }
}

fn metadata_from_parts(
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
        let result = apply_to_map(&mut config.workspaces, self.name(), incoming);
        config.resource_meta.workspaces.insert(
            self.name().to_string(),
            crate::config::ResourceStoredMetadata {
                labels: self.metadata.labels.clone(),
                annotations: self.metadata.annotations.clone(),
            },
        );
        result
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
            metadata: match config.resource_meta.workspaces.get(name) {
                Some(stored) => metadata_from_parts(
                    name,
                    None,
                    stored.labels.clone(),
                    stored.annotations.clone(),
                ),
                None => metadata_with_name(name),
            },
            spec: workspace_config_to_spec(workspace),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        let removed = config.workspaces.remove(name).is_some();
        if removed {
            config.resource_meta.workspaces.remove(name);
        }
        removed
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
        let all_templates: Vec<Option<&str>> = vec![
            templates.init_once.as_deref(),
            templates.qa.as_deref(),
            templates.plan.as_deref(),
            templates.fix.as_deref(),
            templates.retest.as_deref(),
            templates.loop_guard.as_deref(),
            templates.ticket_scan.as_deref(),
            templates.build.as_deref(),
            templates.test.as_deref(),
            templates.lint.as_deref(),
            templates.implement.as_deref(),
            templates.review.as_deref(),
            templates.git_ops.as_deref(),
        ];
        let has_named_template = all_templates.iter().any(|t| t.is_some());
        let has_extra_template = !templates.extra.is_empty();
        if !has_named_template && !has_extra_template {
            return Err(anyhow!(
                "agent.spec.templates must define at least one template"
            ));
        }
        for value in &all_templates {
            if matches!(value, Some(raw) if raw.trim().is_empty()) {
                return Err(anyhow!(
                    "agent.spec.templates entries cannot be empty strings"
                ));
            }
        }
        for (name, value) in &templates.extra {
            if value.trim().is_empty() {
                return Err(anyhow!(
                    "agent.spec.templates.{} cannot be an empty string",
                    name
                ));
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = agent_spec_to_config(&self.spec);
        let result = apply_to_map(&mut config.agents, self.name(), incoming);
        config.resource_meta.agents.insert(
            self.name().to_string(),
            crate::config::ResourceStoredMetadata {
                labels: self.metadata.labels.clone(),
                annotations: self.metadata.annotations.clone(),
            },
        );
        result
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Agent,
            &self.metadata,
            ResourceSpec::Agent(Box::new(self.spec.clone())),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.agents.get(name).map(|agent| Self {
            metadata: match config.resource_meta.agents.get(name) {
                Some(stored) => metadata_from_parts(
                    name,
                    None,
                    stored.labels.clone(),
                    stored.annotations.clone(),
                ),
                None => metadata_with_name(name),
            },
            spec: agent_config_to_spec(agent),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        let removed = config.agents.remove(name).is_some();
        if removed {
            config.resource_meta.agents.remove(name);
        }
        removed
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
        for step in &self.spec.steps {
            crate::config::validate_step_type(&step.step_type).map_err(|e| anyhow!(e))?;
        }
        let loop_mode = parse_loop_mode(&self.spec.loop_policy.mode)?;
        if matches!(loop_mode, LoopMode::Fixed) {
            match self.spec.loop_policy.max_cycles {
                None | Some(0) => {
                    return Err(anyhow!("workflow loop.mode=fixed requires max_cycles > 0"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = workflow_spec_to_config(&self.spec)
            .expect("validated workflow spec must be convertible");
        let result = apply_to_map(&mut config.workflows, self.name(), incoming);
        config.resource_meta.workflows.insert(
            self.name().to_string(),
            crate::config::ResourceStoredMetadata {
                labels: self.metadata.labels.clone(),
                annotations: self.metadata.annotations.clone(),
            },
        );
        result
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
            metadata: match config.resource_meta.workflows.get(name) {
                Some(stored) => metadata_from_parts(
                    name,
                    None,
                    stored.labels.clone(),
                    stored.annotations.clone(),
                ),
                None => metadata_with_name(name),
            },
            spec: workflow_config_to_spec(workflow),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        let removed = config.workflows.remove(name).is_some();
        if removed {
            config.resource_meta.workflows.remove(name);
        }
        removed
    }
}

impl Resource for ProjectResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Project
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = ProjectConfig {
            description: self.spec.description.clone(),
            workspaces: std::collections::HashMap::new(),
            agents: std::collections::HashMap::new(),
            workflows: std::collections::HashMap::new(),
        };
        match config.projects.get(self.name()) {
            None => {
                config.projects.insert(self.name().to_string(), incoming);
                ApplyResult::Created
            }
            Some(existing) => {
                if existing.description == incoming.description {
                    ApplyResult::Unchanged
                } else {
                    let mut next = existing.clone();
                    next.description = incoming.description;
                    config.projects.insert(self.name().to_string(), next);
                    ApplyResult::Configured
                }
            }
        }
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Project,
            &self.metadata,
            ResourceSpec::Project(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.projects.get(name).map(|project| Self {
            metadata: metadata_with_name(name),
            spec: ProjectSpec {
                description: project.description.clone(),
            },
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        config.projects.remove(name).is_some()
    }
}

impl Resource for DefaultsResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Defaults
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = ConfigDefaults {
            project: self.spec.project.clone(),
            workspace: self.spec.workspace.clone(),
            workflow: self.spec.workflow.clone(),
        };
        if serializes_equal(&config.defaults, &incoming) {
            ApplyResult::Unchanged
        } else {
            config.defaults = incoming;
            ApplyResult::Configured
        }
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::Defaults,
            &self.metadata,
            ResourceSpec::Defaults(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, _name: &str) -> Option<Self> {
        Some(Self {
            metadata: metadata_with_name("defaults"),
            spec: DefaultsSpec {
                project: config.defaults.project.clone(),
                workspace: config.defaults.workspace.clone(),
                workflow: config.defaults.workflow.clone(),
            },
        })
    }

    fn delete_from(_config: &mut OrchestratorConfig, _name: &str) -> bool {
        false
    }
}

impl Resource for RuntimePolicyResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::RuntimePolicy
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        validate_resource_name(self.name())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming_runner = runner_spec_to_config(&self.spec.runner);
        let incoming_resume = ResumeConfig {
            auto: self.spec.resume.auto,
        };
        if serializes_equal(&config.runner, &incoming_runner)
            && serializes_equal(&config.resume, &incoming_resume)
        {
            return ApplyResult::Unchanged;
        }
        config.runner = incoming_runner;
        config.resume = incoming_resume;
        ApplyResult::Configured
    }

    fn to_yaml(&self) -> Result<String> {
        manifest_yaml(
            ResourceKind::RuntimePolicy,
            &self.metadata,
            ResourceSpec::RuntimePolicy(self.spec.clone()),
        )
    }

    fn get_from(config: &OrchestratorConfig, _name: &str) -> Option<Self> {
        Some(Self {
            metadata: metadata_with_name("runtime"),
            spec: RuntimePolicySpec {
                runner: runner_config_to_spec(&config.runner),
                resume: ResumeSpec {
                    auto: config.resume.auto,
                },
            },
        })
    }

    fn delete_from(_config: &mut OrchestratorConfig, _name: &str) -> bool {
        false
    }
}

impl Resource for RegisteredResource {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Agent(_) => ResourceKind::Agent,
            Self::Workflow(_) => ResourceKind::Workflow,
            Self::Project(_) => ResourceKind::Project,
            Self::Defaults(_) => ResourceKind::Defaults,
            Self::RuntimePolicy(_) => ResourceKind::RuntimePolicy,
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
        false
    }
}

pub fn delete_resource_by_kind(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
) -> Result<bool> {
    match kind {
        "ws" | "workspace" => Ok(WorkspaceResource::delete_from(config, name)),
        "agent" => Ok(AgentResource::delete_from(config, name)),
        "wf" | "workflow" => Ok(WorkflowResource::delete_from(config, name)),
        "project" => Ok(ProjectResource::delete_from(config, name)),
        "defaults" => Ok(DefaultsResource::delete_from(config, name)),
        "runtimepolicy" | "runtime-policy" => Ok(RuntimePolicyResource::delete_from(config, name)),
        _ => Err(anyhow!(
            "unknown resource type: {} (supported: workspace, agent, workflow, project, defaults, runtimepolicy)",
            kind
        )),
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
        ResourceSpec::Agent(spec) => Ok(RegisteredResource::Agent(Box::new(AgentResource {
            metadata,
            spec: *spec,
        }))),
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

fn build_project(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Project {
        return Err(anyhow!("resource kind/spec mismatch for Project"));
    }
    match spec {
        ResourceSpec::Project(spec) => Ok(RegisteredResource::Project(ProjectResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Project")),
    }
}

fn build_defaults(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Defaults {
        return Err(anyhow!("resource kind/spec mismatch for Defaults"));
    }
    match spec {
        ResourceSpec::Defaults(spec) => Ok(RegisteredResource::Defaults(DefaultsResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Defaults")),
    }
}

fn build_runtime_policy(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::RuntimePolicy {
        return Err(anyhow!("resource kind/spec mismatch for RuntimePolicy"));
    }
    match spec {
        ResourceSpec::RuntimePolicy(spec) => {
            Ok(RegisteredResource::RuntimePolicy(RuntimePolicyResource {
                metadata,
                spec,
            }))
        }
        _ => Err(anyhow!("resource kind/spec mismatch for RuntimePolicy")),
    }
}

fn workspace_spec_to_config(spec: &WorkspaceSpec) -> WorkspaceConfig {
    WorkspaceConfig {
        root_path: spec.root_path.clone(),
        qa_targets: spec.qa_targets.clone(),
        ticket_dir: spec.ticket_dir.clone(),
        self_referential: spec.self_referential,
    }
}

fn workspace_config_to_spec(config: &WorkspaceConfig) -> WorkspaceSpec {
    WorkspaceSpec {
        root_path: config.root_path.clone(),
        qa_targets: config.qa_targets.clone(),
        ticket_dir: config.ticket_dir.clone(),
        self_referential: config.self_referential,
    }
}

fn agent_spec_to_config(spec: &AgentSpec) -> AgentConfig {
    let named_templates: Vec<(&str, &Option<String>)> = vec![
        ("init_once", &spec.templates.init_once),
        ("plan", &spec.templates.plan),
        ("qa", &spec.templates.qa),
        ("ticket_scan", &spec.templates.ticket_scan),
        ("fix", &spec.templates.fix),
        ("retest", &spec.templates.retest),
        ("loop_guard", &spec.templates.loop_guard),
        ("build", &spec.templates.build),
        ("test", &spec.templates.test),
        ("lint", &spec.templates.lint),
        ("implement", &spec.templates.implement),
        ("review", &spec.templates.review),
        ("git_ops", &spec.templates.git_ops),
    ];

    let template_capabilities: Vec<String> = named_templates
        .iter()
        .filter_map(|(name, opt)| opt.as_ref().map(|_| name.to_string()))
        .collect();

    let mut capabilities = spec.capabilities.clone().unwrap_or_default();
    for cap in template_capabilities {
        if !capabilities.contains(&cap) {
            capabilities.push(cap);
        }
    }

    let mut templates = std::collections::HashMap::new();
    for (name, opt) in &named_templates {
        if let Some(t) = opt {
            templates.insert(name.to_string(), t.clone());
        }
    }
    // Include extra/custom templates (qa_doc_gen, qa_testing, ticket_fix, etc.)
    for (name, t) in &spec.templates.extra {
        if !templates.contains_key(name) {
            templates.insert(name.clone(), t.clone());
            if !capabilities.contains(name) {
                capabilities.push(name.clone());
            }
        }
    }

    AgentConfig {
        metadata: AgentMetadata {
            name: String::new(),
            description: spec
                .metadata
                .as_ref()
                .and_then(|m| m.description.clone()),
            version: None,
            cost: spec.metadata.as_ref().and_then(|m| m.cost),
        },
        capabilities,
        templates,
        selection: spec
            .selection
            .as_ref()
            .map(|selection| AgentSelectionConfig {
                strategy: selection.strategy,
                weights: selection.weights.clone(),
            })
            .unwrap_or_default(),
    }
}

fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        templates: AgentTemplatesSpec {
            init_once: config.templates.get("init_once").cloned(),
            plan: config.templates.get("plan").cloned(),
            qa: config.templates.get("qa").cloned(),
            ticket_scan: config.templates.get("ticket_scan").cloned(),
            fix: config.templates.get("fix").cloned(),
            retest: config.templates.get("retest").cloned(),
            loop_guard: config.templates.get("loop_guard").cloned(),
            build: config.templates.get("build").cloned(),
            test: config.templates.get("test").cloned(),
            lint: config.templates.get("lint").cloned(),
            implement: config.templates.get("implement").cloned(),
            review: config.templates.get("review").cloned(),
            git_ops: config.templates.get("git_ops").cloned(),
            extra: {
                let named_keys: std::collections::HashSet<&str> = [
                    "init_once",
                    "plan",
                    "qa",
                    "ticket_scan",
                    "fix",
                    "retest",
                    "loop_guard",
                    "build",
                    "test",
                    "lint",
                    "implement",
                    "review",
                    "git_ops",
                ]
                .into_iter()
                .collect();
                config
                    .templates
                    .iter()
                    .filter(|(k, _)| !named_keys.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            },
        },
        capabilities: if config.capabilities.is_empty() {
            None
        } else {
            Some(config.capabilities.clone())
        },
        metadata: if config.metadata.description.is_none() && config.metadata.cost.is_none() {
            None
        } else {
            Some(AgentMetadataSpec {
                cost: config.metadata.cost,
                description: config.metadata.description.clone(),
            })
        },
        selection: Some(AgentSelectionSpec {
            strategy: config.selection.strategy,
            weights: config.selection.weights.clone(),
        }),
    }
}

fn workflow_spec_to_config(spec: &WorkflowSpec) -> Result<WorkflowConfig> {
    let steps = spec
        .steps
        .iter()
        .map(|step| {
            crate::config::validate_step_type(&step.step_type).map_err(|e| anyhow!(e))?;
            let is_guard = step.step_type == "loop_guard";
            let builtin = if matches!(step.step_type.as_str(), "init_once" | "loop_guard") {
                Some(step.step_type.clone())
            } else {
                None
            };
            let prehook = match step.prehook.as_ref() {
                Some(prehook) => Some(StepPrehookConfig {
                    engine: parse_hook_engine(&prehook.engine),
                    when: prehook.when.clone(),
                    reason: prehook.reason.clone(),
                    ui: prehook
                        .ui
                        .as_ref()
                        .map(|ui| serde_json::from_value::<StepPrehookUiConfig>(ui.clone()))
                        .transpose()
                        .map_err(|e| anyhow!("invalid prehook ui: {}", e))?,
                    extended: prehook.extended,
                }),
                None => None,
            };
            let scope = match step.scope.as_deref() {
                Some("task") => Some(StepScope::Task),
                Some("item") => Some(StepScope::Item),
                _ => None,
            };
            let is_builtin_type = matches!(
                step.step_type.as_str(),
                "init_once" | "loop_guard" | "ticket_scan"
            );
            let required_capability = step.required_capability.clone().or_else(|| {
                if is_builtin_type {
                    None
                } else {
                    Some(step.step_type.clone())
                }
            });
            let builtin = if is_builtin_type {
                Some(step.step_type.clone())
            } else {
                builtin
            };
            Ok(WorkflowStepConfig {
                id: step.id.clone(),
                description: None,
                required_capability,
                builtin: step.builtin.clone().or(builtin),
                enabled: step.enabled,
                repeatable: step.repeatable,
                is_guard: step.is_guard || is_guard,
                cost_preference: parse_cost_preference(step.cost_preference.as_deref())?,
                prehook,
                tty: step.tty,
                outputs: Vec::new(),
                pipe_to: None,
                command: step.command.clone(),
                chain_steps: vec![],
                scope,
                behavior: StepBehavior::default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let loop_policy = WorkflowLoopConfig {
        mode: parse_loop_mode(&spec.loop_policy.mode)?,
        guard: WorkflowLoopGuardConfig {
            max_cycles: spec.loop_policy.max_cycles,
            enabled: spec.loop_policy.enabled,
            stop_when_no_unresolved: spec.loop_policy.stop_when_no_unresolved,
            agent_template: spec.loop_policy.agent_template.clone(),
        },
    };

    let finalize = WorkflowFinalizeConfig {
        rules: spec
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRule {
                id: rule.id.clone(),
                engine: parse_hook_engine(&rule.engine),
                when: rule.when.clone(),
                status: rule.status.clone(),
                reason: rule.reason.clone(),
            })
            .collect(),
    };

    Ok(WorkflowConfig {
        steps,
        loop_policy,
        finalize,
        qa: None,
        fix: None,
        retest: None,
        dynamic_steps: spec
            .dynamic_steps
            .iter()
            .map(
                |dynamic_step| crate::dynamic_orchestration::DynamicStepConfig {
                    id: dynamic_step.id.clone(),
                    description: dynamic_step.description.clone(),
                    step_type: dynamic_step.step_type.clone(),
                    agent_id: dynamic_step.agent_id.clone(),
                    template: dynamic_step.template.clone(),
                    trigger: dynamic_step.trigger.clone(),
                    priority: dynamic_step.priority,
                    max_runs: dynamic_step.max_runs,
                },
            )
            .collect(),
        safety: crate::config::SafetyConfig {
            max_consecutive_failures: spec.safety.max_consecutive_failures,
            auto_rollback: spec.safety.auto_rollback,
            checkpoint_strategy: match spec.safety.checkpoint_strategy.as_str() {
                "git_tag" => crate::config::CheckpointStrategy::GitTag,
                "git_stash" => crate::config::CheckpointStrategy::GitStash,
                _ => crate::config::CheckpointStrategy::None,
            },
            step_timeout_secs: spec.safety.step_timeout_secs,
            binary_snapshot: spec.safety.binary_snapshot,
        },
    })
}

fn workflow_config_to_spec(config: &WorkflowConfig) -> WorkflowSpec {
    let steps = config
        .steps
        .iter()
        .map(|step| WorkflowStepSpec {
            id: step.id.clone(),
            step_type: step
                .builtin
                .clone()
                .or_else(|| step.required_capability.clone())
                .unwrap_or_else(|| step.id.clone()),
            required_capability: step.required_capability.clone(),
            builtin: step.builtin.clone(),
            enabled: step.enabled,
            repeatable: step.repeatable,
            is_guard: step.is_guard,
            cost_preference: step.cost_preference.as_ref().map(|c| match c {
                CostPreference::Performance => "performance".to_string(),
                CostPreference::Quality => "quality".to_string(),
                CostPreference::Balance => "balance".to_string(),
            }),
            prehook: step.prehook.as_ref().map(|prehook| WorkflowPrehookSpec {
                engine: hook_engine_as_str(&prehook.engine).to_string(),
                when: prehook.when.clone(),
                reason: prehook.reason.clone(),
                ui: prehook
                    .ui
                    .as_ref()
                    .map(|value| serde_json::to_value(value).unwrap_or(serde_json::Value::Null)),
                extended: prehook.extended,
            }),
            tty: step.tty,
            command: step.command.clone(),
            scope: step.scope.and_then(|s| {
                // Only serialize when it differs from default
                let default = crate::config::default_scope_for_step_id(&step.id);
                if s != default {
                    Some(match s {
                        StepScope::Task => "task".to_string(),
                        StepScope::Item => "item".to_string(),
                    })
                } else {
                    None
                }
            }),
        })
        .collect();

    let loop_policy = WorkflowLoopSpec {
        mode: loop_mode_as_str(&config.loop_policy.mode).to_string(),
        max_cycles: config.loop_policy.guard.max_cycles,
        enabled: config.loop_policy.guard.enabled,
        stop_when_no_unresolved: config.loop_policy.guard.stop_when_no_unresolved,
        agent_template: config.loop_policy.guard.agent_template.clone(),
    };

    let finalize = WorkflowFinalizeSpec {
        rules: config
            .finalize
            .rules
            .iter()
            .map(|rule| WorkflowFinalizeRuleSpec {
                id: rule.id.clone(),
                engine: hook_engine_as_str(&rule.engine).to_string(),
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
        dynamic_steps: config
            .dynamic_steps
            .iter()
            .map(|dynamic_step| DynamicStepSpec {
                id: dynamic_step.id.clone(),
                description: dynamic_step.description.clone(),
                step_type: dynamic_step.step_type.clone(),
                agent_id: dynamic_step.agent_id.clone(),
                template: dynamic_step.template.clone(),
                trigger: dynamic_step.trigger.clone(),
                priority: dynamic_step.priority,
                max_runs: dynamic_step.max_runs,
            })
            .collect(),
        safety: SafetySpec::default(),
    }
}

fn parse_hook_engine(value: &str) -> StepHookEngine {
    match value {
        "cel" => StepHookEngine::Cel,
        _ => StepHookEngine::Cel,
    }
}

fn hook_engine_as_str(value: &StepHookEngine) -> &'static str {
    match value {
        StepHookEngine::Cel => "cel",
    }
}

fn parse_cost_preference(value: Option<&str>) -> Result<Option<CostPreference>> {
    Ok(match value {
        Some("performance") => Some(CostPreference::Performance),
        Some("quality") => Some(CostPreference::Quality),
        Some("balance") => Some(CostPreference::Balance),
        Some(other) => return Err(anyhow!("unknown cost_preference '{}'", other)),
        None => None,
    })
}

fn runner_spec_to_config(spec: &RunnerSpec) -> RunnerConfig {
    RunnerConfig {
        shell: spec.shell.clone(),
        shell_arg: spec.shell_arg.clone(),
        policy: match spec.policy.as_str() {
            "allowlist" => RunnerPolicy::Allowlist,
            _ => RunnerPolicy::Legacy,
        },
        executor: match spec.executor.as_str() {
            "shell" => RunnerExecutorKind::Shell,
            _ => RunnerExecutorKind::Shell,
        },
        allowed_shells: spec.allowed_shells.clone(),
        allowed_shell_args: spec.allowed_shell_args.clone(),
        env_allowlist: spec.env_allowlist.clone(),
        redaction_patterns: spec.redaction_patterns.clone(),
    }
}

fn runner_config_to_spec(config: &RunnerConfig) -> RunnerSpec {
    RunnerSpec {
        shell: config.shell.clone(),
        shell_arg: config.shell_arg.clone(),
        policy: match config.policy {
            RunnerPolicy::Legacy => "legacy".to_string(),
            RunnerPolicy::Allowlist => "allowlist".to_string(),
        },
        executor: match config.executor {
            RunnerExecutorKind::Shell => "shell".to_string(),
        },
        allowed_shells: config.allowed_shells.clone(),
        allowed_shell_args: config.allowed_shell_args.clone(),
        env_allowlist: config.env_allowlist.clone(),
        redaction_patterns: config.redaction_patterns.clone(),
    }
}

fn parse_loop_mode(value: &str) -> Result<LoopMode> {
    value.parse::<LoopMode>().map_err(|e| anyhow!(e))
}

fn loop_mode_as_str(mode: &LoopMode) -> &'static str {
    match mode {
        LoopMode::Once => "once",
        LoopMode::Fixed => "fixed",
        LoopMode::Infinite => "infinite",
    }
}

pub fn kind_as_str(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Workspace => "workspace",
        ResourceKind::Agent => "agent",
        ResourceKind::Workflow => "workflow",
        ResourceKind::Project => "project",
        ResourceKind::Defaults => "defaults",
        ResourceKind::RuntimePolicy => "runtimepolicy",
    }
}

pub fn parse_resources_from_yaml(content: &str) -> Result<Vec<OrchestratorResource>> {
    let mut resources = Vec::new();
    for document in serde_yaml::Deserializer::from_str(content) {
        let value = serde_yaml::Value::deserialize(document)?;
        if value.is_null() {
            continue;
        }
        let resource = serde_yaml::from_value::<OrchestratorResource>(value)?;
        resources.push(resource);
    }
    Ok(resources)
}

pub fn export_manifest_resources(config: &OrchestratorConfig) -> Vec<RegisteredResource> {
    let mut resources = Vec::new();
    resources.push(RegisteredResource::RuntimePolicy(RuntimePolicyResource {
        metadata: metadata_with_name("runtime"),
        spec: RuntimePolicySpec {
            runner: runner_config_to_spec(&config.runner),
            resume: ResumeSpec {
                auto: config.resume.auto,
            },
        },
    }));
    resources.push(RegisteredResource::Defaults(DefaultsResource {
        metadata: metadata_with_name("defaults"),
        spec: DefaultsSpec {
            project: config.defaults.project.clone(),
            workspace: config.defaults.workspace.clone(),
            workflow: config.defaults.workflow.clone(),
        },
    }));
    for (name, project) in &config.projects {
        resources.push(RegisteredResource::Project(ProjectResource {
            metadata: metadata_with_name(name),
            spec: ProjectSpec {
                description: project.description.clone(),
            },
        }));
    }
    for (name, workspace) in &config.workspaces {
        let metadata = match config.resource_meta.workspaces.get(name) {
            Some(stored) => metadata_from_parts(
                name,
                None,
                stored.labels.clone(),
                stored.annotations.clone(),
            ),
            None => metadata_with_name(name),
        };
        resources.push(RegisteredResource::Workspace(WorkspaceResource {
            metadata,
            spec: workspace_config_to_spec(workspace),
        }));
    }
    for (name, agent) in &config.agents {
        let metadata = match config.resource_meta.agents.get(name) {
            Some(stored) => metadata_from_parts(
                name,
                None,
                stored.labels.clone(),
                stored.annotations.clone(),
            ),
            None => metadata_with_name(name),
        };
        resources.push(RegisteredResource::Agent(Box::new(AgentResource {
            metadata,
            spec: agent_config_to_spec(agent),
        })));
    }
    for (name, workflow) in &config.workflows {
        let metadata = match config.resource_meta.workflows.get(name) {
            Some(stored) => metadata_from_parts(
                name,
                None,
                stored.labels.clone(),
                stored.annotations.clone(),
            ),
            None => metadata_with_name(name),
        };
        resources.push(RegisteredResource::Workflow(WorkflowResource {
            metadata,
            spec: workflow_config_to_spec(workflow),
        }));
    }
    resources
}

pub fn export_manifest_documents(config: &OrchestratorConfig) -> Vec<OrchestratorResource> {
    export_manifest_resources(config)
        .into_iter()
        .map(|resource| match resource {
            RegisteredResource::Workspace(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workspace,
                metadata: item.metadata,
                spec: ResourceSpec::Workspace(item.spec),
            },
            RegisteredResource::Agent(item) => {
                let item = *item;
                OrchestratorResource {
                    api_version: API_VERSION.to_string(),
                    kind: ResourceKind::Agent,
                    metadata: item.metadata,
                    spec: ResourceSpec::Agent(Box::new(item.spec)),
                }
            }
            RegisteredResource::Workflow(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Workflow,
                metadata: item.metadata,
                spec: ResourceSpec::Workflow(item.spec),
            },
            RegisteredResource::Project(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Project,
                metadata: item.metadata,
                spec: ResourceSpec::Project(item.spec),
            },
            RegisteredResource::Defaults(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Defaults,
                metadata: item.metadata,
                spec: ResourceSpec::Defaults(item.spec),
            },
            RegisteredResource::RuntimePolicy(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::RuntimePolicy,
                metadata: item.metadata,
                spec: ResourceSpec::RuntimePolicy(item.spec),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;

    fn workspace_manifest(name: &str, root_path: &str) -> OrchestratorResource {
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

    fn agent_manifest(name: &str, qa_command: &str) -> OrchestratorResource {
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
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some(qa_command.to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            })),
        }
    }

    fn workflow_manifest(name: &str) -> OrchestratorResource {
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
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some("run".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            })),
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
    fn workspace_resource_apply() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
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
            let active = read_active_config(&state).expect("state should be readable");
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
            let active = read_active_config(&state).expect("state should be readable");
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

    // ── helper factories ──────────────────────────────────────────────

    fn project_manifest(name: &str, description: &str) -> OrchestratorResource {
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

    fn defaults_manifest(project: &str, workspace: &str, workflow: &str) -> OrchestratorResource {
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

    fn runtime_policy_manifest() -> OrchestratorResource {
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
                    policy: "legacy".to_string(),
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

    fn make_config() -> OrchestratorConfig {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let active = read_active_config(&state).expect("state should be readable");
        active.config.clone()
    }

    // ── ProjectResource tests ──────────────────────────────────────────

    #[test]
    fn project_resource_dispatch_and_kind() {
        let resource = dispatch_resource(project_manifest("my-proj", "A test project"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Project);
        assert_eq!(resource.name(), "my-proj");
    }

    #[test]
    fn project_resource_validate_accepts_valid() {
        let resource = dispatch_resource(project_manifest("valid-proj", "desc"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn project_resource_validate_rejects_empty_name() {
        let resource = dispatch_resource(project_manifest("", "desc"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_err());
    }

    #[test]
    fn project_resource_apply_created_then_unchanged() {
        let mut config = make_config();
        let resource = dispatch_resource(project_manifest("proj-a", "desc"))
            .expect("dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);
        assert_eq!(resource.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn project_resource_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(project_manifest("proj-b", "v1"))
            .expect("dispatch should succeed");
        assert_eq!(r1.apply(&mut config), ApplyResult::Created);

        let r2 = dispatch_resource(project_manifest("proj-b", "v2"))
            .expect("dispatch should succeed");
        assert_eq!(r2.apply(&mut config), ApplyResult::Configured);
    }

    #[test]
    fn project_resource_get_from_and_delete_from() {
        let mut config = make_config();
        let resource = dispatch_resource(project_manifest("proj-del", "desc"))
            .expect("dispatch should succeed");
        resource.apply(&mut config);

        let loaded = ProjectResource::get_from(&config, "proj-del");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().spec.description, Some("desc".to_string()));

        assert!(ProjectResource::delete_from(&mut config, "proj-del"));
        assert!(ProjectResource::get_from(&config, "proj-del").is_none());
    }

    #[test]
    fn project_resource_delete_returns_false_when_missing() {
        let mut config = make_config();
        assert!(!ProjectResource::delete_from(&mut config, "nonexistent"));
    }

    #[test]
    fn project_resource_to_yaml() {
        let resource = dispatch_resource(project_manifest("yaml-proj", "desc"))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: Project"));
        assert!(yaml.contains("yaml-proj"));
    }

    // ── DefaultsResource tests ──────────────────────────────────────────

    #[test]
    fn defaults_resource_dispatch_and_kind() {
        let resource = dispatch_resource(defaults_manifest("p", "w", "f"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Defaults);
        assert_eq!(resource.name(), "defaults");
    }

    #[test]
    fn defaults_resource_apply_unchanged_when_same() {
        let mut config = make_config();
        let r1 = dispatch_resource(defaults_manifest("p", "w", "f"))
            .expect("dispatch should succeed");
        r1.apply(&mut config);
        // Apply same again -> unchanged
        assert_eq!(r1.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn defaults_resource_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(defaults_manifest("p1", "w1", "f1"))
            .expect("dispatch should succeed");
        r1.apply(&mut config);

        let r2 = dispatch_resource(defaults_manifest("p2", "w2", "f2"))
            .expect("dispatch should succeed");
        assert_eq!(r2.apply(&mut config), ApplyResult::Configured);
    }

    #[test]
    fn defaults_resource_get_from_always_returns_some() {
        let config = make_config();
        let loaded = DefaultsResource::get_from(&config, "anything");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().metadata.name, "defaults");
    }

    #[test]
    fn defaults_resource_delete_returns_false() {
        let mut config = make_config();
        assert!(!DefaultsResource::delete_from(&mut config, "defaults"));
    }

    #[test]
    fn defaults_resource_to_yaml() {
        let resource = dispatch_resource(defaults_manifest("proj", "", ""))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: Defaults"));
        assert!(yaml.contains("proj"));
    }

    // ── RuntimePolicyResource tests ─────────────────────────────────────

    #[test]
    fn runtime_policy_dispatch_and_kind() {
        let resource = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::RuntimePolicy);
        assert_eq!(resource.name(), "runtime");
    }

    #[test]
    fn runtime_policy_apply_unchanged_when_same() {
        let mut config = make_config();
        let r1 = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch should succeed");
        r1.apply(&mut config);
        assert_eq!(r1.apply(&mut config), ApplyResult::Unchanged);
    }

    #[test]
    fn runtime_policy_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch should succeed");
        r1.apply(&mut config);

        // Change the runner policy
        let mut manifest = runtime_policy_manifest();
        if let ResourceSpec::RuntimePolicy(ref mut spec) = manifest.spec {
            spec.runner.policy = "allowlist".to_string();
        }
        let r2 = dispatch_resource(manifest).expect("dispatch should succeed");
        assert_eq!(r2.apply(&mut config), ApplyResult::Configured);
    }

    #[test]
    fn runtime_policy_get_from_always_returns_some() {
        let config = make_config();
        let loaded = RuntimePolicyResource::get_from(&config, "runtime");
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().metadata.name, "runtime");
    }

    #[test]
    fn runtime_policy_delete_returns_false() {
        let mut config = make_config();
        assert!(!RuntimePolicyResource::delete_from(&mut config, "runtime"));
    }

    #[test]
    fn runtime_policy_to_yaml() {
        let resource = dispatch_resource(runtime_policy_manifest())
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: RuntimePolicy"));
        assert!(yaml.contains("/bin/bash"));
    }

    // ── delete_resource_by_kind tests ────────────────────────────────

    #[test]
    fn delete_resource_by_kind_workspace() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("del-ws", "workspace/del"))
            .expect("dispatch should succeed");
        ws.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "workspace", "del-ws").unwrap());
        assert!(!config.workspaces.contains_key("del-ws"));
    }

    #[test]
    fn delete_resource_by_kind_ws_alias() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("del-ws2", "workspace/del2"))
            .expect("dispatch should succeed");
        ws.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "ws", "del-ws2").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_agent() {
        let mut config = make_config();
        let agent = dispatch_resource(agent_manifest("del-agent", "cargo test"))
            .expect("dispatch should succeed");
        agent.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "agent", "del-agent").unwrap());
        assert!(!config.agents.contains_key("del-agent"));
    }

    #[test]
    fn delete_resource_by_kind_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf"))
            .expect("dispatch should succeed");
        wf.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "workflow", "del-wf").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_wf_alias() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("del-wf2"))
            .expect("dispatch should succeed");
        wf.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "wf", "del-wf2").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_project() {
        let mut config = make_config();
        let proj = dispatch_resource(project_manifest("del-proj", "desc"))
            .expect("dispatch should succeed");
        proj.apply(&mut config);
        assert!(delete_resource_by_kind(&mut config, "project", "del-proj").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_defaults() {
        let mut config = make_config();
        assert!(!delete_resource_by_kind(&mut config, "defaults", "defaults").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_runtime_policy() {
        let mut config = make_config();
        assert!(!delete_resource_by_kind(&mut config, "runtimepolicy", "runtime").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_runtime_policy_alias() {
        let mut config = make_config();
        assert!(!delete_resource_by_kind(&mut config, "runtime-policy", "runtime").unwrap());
    }

    #[test]
    fn delete_resource_by_kind_rejects_unknown() {
        let mut config = make_config();
        let err = delete_resource_by_kind(&mut config, "foobar", "x").unwrap_err();
        assert!(err.to_string().contains("unknown resource type"));
    }

    // ── RegisteredResource dispatch delegation ─────────────────────────

    #[test]
    fn registered_resource_kind_name_for_all_variants() {
        let ws = dispatch_resource(workspace_manifest("rr-ws", "workspace/rr")).unwrap();
        assert_eq!(ws.kind(), ResourceKind::Workspace);
        assert_eq!(ws.name(), "rr-ws");

        let ag = dispatch_resource(agent_manifest("rr-ag", "cmd")).unwrap();
        assert_eq!(ag.kind(), ResourceKind::Agent);
        assert_eq!(ag.name(), "rr-ag");

        let wf = dispatch_resource(workflow_manifest("rr-wf")).unwrap();
        assert_eq!(wf.kind(), ResourceKind::Workflow);
        assert_eq!(wf.name(), "rr-wf");

        let pr = dispatch_resource(project_manifest("rr-pr", "d")).unwrap();
        assert_eq!(pr.kind(), ResourceKind::Project);
        assert_eq!(pr.name(), "rr-pr");

        let df = dispatch_resource(defaults_manifest("", "", "")).unwrap();
        assert_eq!(df.kind(), ResourceKind::Defaults);
        assert_eq!(df.name(), "defaults");

        let rp = dispatch_resource(runtime_policy_manifest()).unwrap();
        assert_eq!(rp.kind(), ResourceKind::RuntimePolicy);
        assert_eq!(rp.name(), "runtime");
    }

    #[test]
    fn registered_resource_validate_delegates() {
        // Test that validate dispatches correctly for each variant
        let ws = dispatch_resource(workspace_manifest("v-ws", "workspace/v")).unwrap();
        assert!(ws.validate().is_ok());

        let ag = dispatch_resource(agent_manifest("v-ag", "cmd")).unwrap();
        assert!(ag.validate().is_ok());

        let wf = dispatch_resource(workflow_manifest("v-wf")).unwrap();
        assert!(wf.validate().is_ok());

        let pr = dispatch_resource(project_manifest("v-pr", "d")).unwrap();
        assert!(pr.validate().is_ok());

        let df = dispatch_resource(defaults_manifest("", "", "")).unwrap();
        assert!(df.validate().is_ok());

        let rp = dispatch_resource(runtime_policy_manifest()).unwrap();
        assert!(rp.validate().is_ok());
    }

    #[test]
    fn registered_resource_to_yaml_delegates() {
        let ws = dispatch_resource(workspace_manifest("ty-ws", "workspace/ty")).unwrap();
        assert!(ws.to_yaml().unwrap().contains("Workspace"));

        let ag = dispatch_resource(agent_manifest("ty-ag", "cmd")).unwrap();
        assert!(ag.to_yaml().unwrap().contains("Agent"));

        let wf = dispatch_resource(workflow_manifest("ty-wf")).unwrap();
        assert!(wf.to_yaml().unwrap().contains("Workflow"));

        let pr = dispatch_resource(project_manifest("ty-pr", "d")).unwrap();
        assert!(pr.to_yaml().unwrap().contains("Project"));

        let df = dispatch_resource(defaults_manifest("", "", "")).unwrap();
        assert!(df.to_yaml().unwrap().contains("Defaults"));

        let rp = dispatch_resource(runtime_policy_manifest()).unwrap();
        assert!(rp.to_yaml().unwrap().contains("RuntimePolicy"));
    }

    #[test]
    fn registered_resource_get_from_finds_defaults_and_runtime() {
        let config = make_config();
        let defaults = RegisteredResource::get_from(&config, "defaults");
        assert!(defaults.is_some());
        assert_eq!(defaults.unwrap().kind(), ResourceKind::Defaults);

        let runtime = RegisteredResource::get_from(&config, "runtime");
        assert!(runtime.is_some());
        assert_eq!(runtime.unwrap().kind(), ResourceKind::RuntimePolicy);
    }

    #[test]
    fn registered_resource_get_from_returns_none_for_unknown() {
        let config = make_config();
        assert!(RegisteredResource::get_from(&config, "no-such-resource-xyz").is_none());
    }

    #[test]
    fn registered_resource_delete_from_removes_workspace() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("rd-ws", "workspace/rd")).unwrap();
        ws.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-ws"));
        assert!(!config.workspaces.contains_key("rd-ws"));
    }

    #[test]
    fn registered_resource_delete_from_removes_agent() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("rd-ag", "cmd")).unwrap();
        ag.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-ag"));
        assert!(!config.agents.contains_key("rd-ag"));
    }

    #[test]
    fn registered_resource_delete_from_removes_workflow() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("rd-wf")).unwrap();
        wf.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-wf"));
        assert!(!config.workflows.contains_key("rd-wf"));
    }

    #[test]
    fn registered_resource_delete_from_removes_project() {
        let mut config = make_config();
        let pr = dispatch_resource(project_manifest("rd-pr", "d")).unwrap();
        pr.apply(&mut config);
        assert!(RegisteredResource::delete_from(&mut config, "rd-pr"));
        assert!(!config.projects.contains_key("rd-pr"));
    }

    #[test]
    fn registered_resource_delete_from_returns_false_for_unknown() {
        let mut config = make_config();
        assert!(!RegisteredResource::delete_from(&mut config, "no-such-thing"));
    }

    // ── Validation edge cases ──────────────────────────────────────────

    #[test]
    fn workspace_validate_rejects_empty_root_path() {
        let ws = WorkspaceResource {
            metadata: metadata_with_name("ws-no-root"),
            spec: WorkspaceSpec {
                root_path: "  ".to_string(),
                qa_targets: vec![],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        };
        let err = ws.validate().unwrap_err();
        assert!(err.to_string().contains("root_path"));
    }

    #[test]
    fn workspace_validate_rejects_empty_ticket_dir() {
        let ws = WorkspaceResource {
            metadata: metadata_with_name("ws-no-ticket"),
            spec: WorkspaceSpec {
                root_path: "/some/path".to_string(),
                qa_targets: vec![],
                ticket_dir: "  ".to_string(),
                self_referential: false,
            },
        };
        let err = ws.validate().unwrap_err();
        assert!(err.to_string().contains("ticket_dir"));
    }

    #[test]
    fn agent_validate_rejects_empty_string_template() {
        let agent = AgentResource {
            metadata: metadata_with_name("ag-empty-tmpl"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: Some("  ".to_string()), // empty string
                    qa: Some("valid".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let err = agent.validate().unwrap_err();
        assert!(err.to_string().contains("cannot be empty strings"));
    }

    #[test]
    fn agent_validate_rejects_empty_extra_template() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("custom".to_string(), "  ".to_string());
        let agent = AgentResource {
            metadata: metadata_with_name("ag-empty-extra"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some("valid".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra,
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let err = agent.validate().unwrap_err();
        assert!(err.to_string().contains("cannot be an empty string"));
    }

    #[test]
    fn agent_validate_accepts_extra_only_templates() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("qa_doc_gen".to_string(), "do qa doc gen".to_string());
        let agent = AgentResource {
            metadata: metadata_with_name("ag-extra-only"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: None,
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra,
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        assert!(agent.validate().is_ok());
    }

    #[test]
    fn workflow_validate_rejects_empty_step_id() {
        let wf = WorkflowResource {
            metadata: metadata_with_name("wf-empty-id"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "  ".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
        assert!(err.to_string().contains("id cannot be empty"));
    }

    #[test]
    fn workflow_validate_rejects_empty_step_type() {
        let wf = WorkflowResource {
            metadata: metadata_with_name("wf-empty-type"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "step1".to_string(),
                    step_type: "  ".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
        assert!(err.to_string().contains("type cannot be empty"));
    }

    #[test]
    fn workflow_validate_rejects_fixed_without_max_cycles() {
        let wf = WorkflowResource {
            metadata: metadata_with_name("wf-fixed-no-max"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
        assert!(err.to_string().contains("max_cycles > 0"));
    }

    #[test]
    fn workflow_validate_rejects_fixed_with_zero_max_cycles() {
        let wf = WorkflowResource {
            metadata: metadata_with_name("wf-fixed-zero"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: Some(0),
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            },
        };
        let err = wf.validate().unwrap_err();
        assert!(err.to_string().contains("max_cycles > 0"));
    }

    #[test]
    fn workflow_validate_accepts_fixed_with_valid_max_cycles() {
        let wf = WorkflowResource {
            metadata: metadata_with_name("wf-fixed-ok"),
            spec: WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "fixed".to_string(),
                    max_cycles: Some(3),
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            },
        };
        assert!(wf.validate().is_ok());
    }

    // ── parse_cost_preference tests ─────────────────────────────────

    #[test]
    fn parse_cost_preference_all_variants() {
        assert_eq!(parse_cost_preference(Some("performance")).unwrap(), Some(CostPreference::Performance));
        assert_eq!(parse_cost_preference(Some("quality")).unwrap(), Some(CostPreference::Quality));
        assert_eq!(parse_cost_preference(Some("balance")).unwrap(), Some(CostPreference::Balance));
        assert_eq!(parse_cost_preference(None).unwrap(), None);
    }

    #[test]
    fn parse_cost_preference_rejects_unknown() {
        let err = parse_cost_preference(Some("turbo")).unwrap_err();
        assert!(err.to_string().contains("unknown cost_preference"));
    }

    // ── kind_as_str tests ───────────────────────────────────────────

    #[test]
    fn kind_as_str_all_variants() {
        assert_eq!(kind_as_str(ResourceKind::Workspace), "workspace");
        assert_eq!(kind_as_str(ResourceKind::Agent), "agent");
        assert_eq!(kind_as_str(ResourceKind::Workflow), "workflow");
        assert_eq!(kind_as_str(ResourceKind::Project), "project");
        assert_eq!(kind_as_str(ResourceKind::Defaults), "defaults");
        assert_eq!(kind_as_str(ResourceKind::RuntimePolicy), "runtimepolicy");
    }

    // ── parse_resources_from_yaml tests ─────────────────────────────

    #[test]
    fn parse_resources_from_yaml_single_document() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: test-proj
spec:
  description: A project
"#;
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].kind, ResourceKind::Project);
        assert_eq!(resources[0].metadata.name, "test-proj");
    }

    #[test]
    fn parse_resources_from_yaml_multi_document() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: proj-1
spec:
  description: first
---
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: proj-2
spec:
  description: second
"#;
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].metadata.name, "proj-1");
        assert_eq!(resources[1].metadata.name, "proj-2");
    }

    #[test]
    fn parse_resources_from_yaml_skips_null_documents() {
        let yaml = "---\n---\napiVersion: orchestrator.dev/v2\nkind: Project\nmetadata:\n  name: p\nspec:\n  description: d\n";
        let resources = parse_resources_from_yaml(yaml).expect("should parse");
        assert_eq!(resources.len(), 1);
    }

    // ── build_* error paths ─────────────────────────────────────────

    #[test]
    fn build_agent_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Agent,
            metadata: metadata_with_name("bad"),
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn build_workflow_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: metadata_with_name("bad"),
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn build_project_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Project,
            metadata: metadata_with_name("bad"),
            spec: ResourceSpec::Defaults(DefaultsSpec {
                project: String::new(),
                workspace: String::new(),
                workflow: String::new(),
            }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn build_defaults_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Defaults,
            metadata: metadata_with_name("bad"),
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    #[test]
    fn build_runtime_policy_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::RuntimePolicy,
            metadata: metadata_with_name("bad"),
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).unwrap_err();
        assert!(err.to_string().contains("mismatch"));
    }

    // ── resource_registry tests ─────────────────────────────────────

    #[test]
    fn resource_registry_has_six_entries() {
        let registry = resource_registry();
        assert_eq!(registry.len(), 6);
        let kinds: Vec<ResourceKind> = registry.iter().map(|r| r.kind).collect();
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
    }

    // ── agent_spec_to_config / agent_config_to_spec roundtrip ───────

    #[test]
    fn agent_spec_config_roundtrip_with_extra_templates() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("qa_doc_gen".to_string(), "gen docs".to_string());
        extra.insert("qa_testing".to_string(), "run qa testing".to_string());

        let spec = AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: None,
                plan: Some("plan template".to_string()),
                qa: Some("qa template".to_string()),
                fix: None,
                retest: None,
                loop_guard: None,
                ticket_scan: None,
                build: None,
                test: None,
                lint: None,
                implement: Some("implement template".to_string()),
                review: None,
                git_ops: None,
                extra,
            },
            capabilities: Some(vec!["plan".to_string(), "custom_cap".to_string()]),
            metadata: Some(AgentMetadataSpec {
                cost: Some(2),
                description: Some("A test agent".to_string()),
            }),
            selection: Some(AgentSelectionSpec {
                strategy: Default::default(),
                weights: None,
            }),
        };

        let config = agent_spec_to_config(&spec);
        // Check extra templates are in config
        assert!(config.templates.contains_key("qa_doc_gen"));
        assert!(config.templates.contains_key("qa_testing"));
        assert!(config.templates.contains_key("plan"));
        assert!(config.templates.contains_key("implement"));
        // Check capabilities include both explicit and template-derived
        assert!(config.capabilities.contains(&"plan".to_string()));
        assert!(config.capabilities.contains(&"custom_cap".to_string()));
        assert!(config.capabilities.contains(&"qa".to_string()));
        assert!(config.capabilities.contains(&"qa_doc_gen".to_string()));

        // Roundtrip back to spec
        let roundtripped = agent_config_to_spec(&config);
        assert_eq!(roundtripped.templates.plan, Some("plan template".to_string()));
        assert_eq!(roundtripped.templates.qa, Some("qa template".to_string()));
        assert_eq!(roundtripped.templates.implement, Some("implement template".to_string()));
        assert!(roundtripped.templates.extra.contains_key("qa_doc_gen"));
        assert!(roundtripped.templates.extra.contains_key("qa_testing"));
        assert!(roundtripped.capabilities.is_some());
        // Metadata (cost, description) is now preserved through the roundtrip.
        let rt_meta = roundtripped.metadata.expect("metadata should be preserved");
        assert_eq!(rt_meta.cost, Some(2));
        assert_eq!(rt_meta.description, Some("A test agent".to_string()));
    }

    #[test]
    fn agent_config_to_spec_empty_capabilities_becomes_none() {
        let config = AgentConfig {
            metadata: AgentMetadata::default(),
            capabilities: vec![],
            templates: std::collections::HashMap::new(),
            selection: AgentSelectionConfig::default(),
        };
        let spec = agent_config_to_spec(&config);
        assert!(spec.capabilities.is_none());
    }

    #[test]
    fn agent_config_to_spec_no_metadata_becomes_none() {
        let config = AgentConfig {
            metadata: AgentMetadata {
                name: String::new(),
                description: None,
                version: None,
                cost: None,
            },
            capabilities: vec![],
            templates: std::collections::HashMap::new(),
            selection: AgentSelectionConfig::default(),
        };
        let spec = agent_config_to_spec(&config);
        assert!(spec.metadata.is_none());
    }

    // ── runner_spec_to_config / runner_config_to_spec roundtrip ──────

    #[test]
    fn runner_spec_config_roundtrip() {
        let spec = RunnerSpec {
            shell: "/bin/zsh".to_string(),
            shell_arg: "-c".to_string(),
            policy: "allowlist".to_string(),
            executor: "shell".to_string(),
            allowed_shells: vec!["/bin/bash".to_string()],
            allowed_shell_args: vec!["-c".to_string()],
            env_allowlist: vec!["PATH".to_string()],
            redaction_patterns: vec!["SECRET_.*".to_string()],
        };

        let config = runner_spec_to_config(&spec);
        assert_eq!(config.shell, "/bin/zsh");
        assert!(matches!(config.policy, RunnerPolicy::Allowlist));
        assert!(matches!(config.executor, RunnerExecutorKind::Shell));
        assert_eq!(config.allowed_shells, vec!["/bin/bash".to_string()]);
        assert_eq!(config.env_allowlist, vec!["PATH".to_string()]);

        let roundtripped = runner_config_to_spec(&config);
        assert_eq!(roundtripped.shell, "/bin/zsh");
        assert_eq!(roundtripped.policy, "allowlist");
        assert_eq!(roundtripped.executor, "shell");
        assert_eq!(roundtripped.allowed_shells, vec!["/bin/bash".to_string()]);
    }

    #[test]
    fn runner_spec_legacy_policy() {
        let spec = RunnerSpec {
            shell: "/bin/sh".to_string(),
            shell_arg: "-c".to_string(),
            policy: "legacy".to_string(),
            executor: "shell".to_string(),
            allowed_shells: vec![],
            allowed_shell_args: vec![],
            env_allowlist: vec![],
            redaction_patterns: vec![],
        };
        let config = runner_spec_to_config(&spec);
        assert!(matches!(config.policy, RunnerPolicy::Legacy));
        let back = runner_config_to_spec(&config);
        assert_eq!(back.policy, "legacy");
    }

    // ── workspace_spec_to_config / workspace_config_to_spec roundtrip

    #[test]
    fn workspace_spec_config_roundtrip() {
        let spec = WorkspaceSpec {
            root_path: "/my/project".to_string(),
            qa_targets: vec!["src".to_string(), "tests".to_string()],
            ticket_dir: "docs/tickets".to_string(),
            self_referential: true,
        };
        let config = workspace_spec_to_config(&spec);
        assert_eq!(config.root_path, "/my/project");
        assert_eq!(config.qa_targets, vec!["src", "tests"]);
        assert_eq!(config.ticket_dir, "docs/tickets");
        assert!(config.self_referential);

        let back = workspace_config_to_spec(&config);
        assert_eq!(back.root_path, "/my/project");
        assert_eq!(back.qa_targets, vec!["src", "tests"]);
        assert!(back.self_referential);
    }

    // ── workflow_spec_to_config conversion details ──────────────────

    #[test]
    fn workflow_spec_to_config_with_prehook_and_cost() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: Some("qa".to_string()),
                builtin: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: Some("quality".to_string()),
                prehook: Some(WorkflowPrehookSpec {
                    engine: "cel".to_string(),
                    when: "is_last_cycle".to_string(),
                    reason: Some("only run on last cycle".to_string()),
                    ui: None,
                    extended: false,
                }),
                tty: true,
                command: Some("cargo test".to_string()),
                scope: Some("task".to_string()),
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "fixed".to_string(),
                max_cycles: Some(2),
                enabled: true,
                stop_when_no_unresolved: false,
                agent_template: Some("guard_template".to_string()),
            },
            finalize: WorkflowFinalizeSpec {
                rules: vec![WorkflowFinalizeRuleSpec {
                    id: "rule1".to_string(),
                    engine: "cel".to_string(),
                    when: "qa_exit_code == 0".to_string(),
                    status: "passed".to_string(),
                    reason: Some("QA passed".to_string()),
                }],
            },
            dynamic_steps: vec![DynamicStepSpec {
                id: "dyn1".to_string(),
                description: Some("dynamic step".to_string()),
                step_type: "qa".to_string(),
                agent_id: Some("agent1".to_string()),
                template: Some("tmpl".to_string()),
                trigger: Some("always".to_string()),
                priority: 10,
                max_runs: Some(3),
            }],
            safety: SafetySpec::default(),
        };

        let config = workflow_spec_to_config(&spec).expect("should convert");
        assert_eq!(config.steps.len(), 1);
        let step = &config.steps[0];
        assert_eq!(step.id, "qa");
        assert_eq!(step.cost_preference, Some(CostPreference::Quality));
        assert!(step.prehook.is_some());
        let prehook = step.prehook.as_ref().unwrap();
        assert_eq!(prehook.when, "is_last_cycle");
        assert_eq!(prehook.reason.as_deref(), Some("only run on last cycle"));
        assert!(step.tty);
        assert_eq!(step.command.as_deref(), Some("cargo test"));
        assert_eq!(step.scope, Some(StepScope::Task));

        // Loop config
        assert!(matches!(config.loop_policy.mode, LoopMode::Fixed));
        assert_eq!(config.loop_policy.guard.max_cycles, Some(2));
        assert!(!config.loop_policy.guard.stop_when_no_unresolved);
        assert_eq!(config.loop_policy.guard.agent_template.as_deref(), Some("guard_template"));

        // Finalize
        assert_eq!(config.finalize.rules.len(), 1);
        assert_eq!(config.finalize.rules[0].id, "rule1");
        assert_eq!(config.finalize.rules[0].status, "passed");

        // Dynamic steps
        assert_eq!(config.dynamic_steps.len(), 1);
        assert_eq!(config.dynamic_steps[0].id, "dyn1");
        assert_eq!(config.dynamic_steps[0].priority, 10);
        assert_eq!(config.dynamic_steps[0].max_runs, Some(3));
    }

    #[test]
    fn workflow_spec_to_config_init_once_sets_builtin() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "init".to_string(),
                step_type: "init_once".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: None,
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert_eq!(config.steps[0].builtin.as_deref(), Some("init_once"));
    }

    #[test]
    fn workflow_spec_to_config_loop_guard_sets_is_guard_and_builtin() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "guard".to_string(),
                step_type: "loop_guard".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false, // should be set to true by conversion
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: None,
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert!(config.steps[0].is_guard);
        assert_eq!(config.steps[0].builtin.as_deref(), Some("loop_guard"));
    }

    #[test]
    fn workflow_spec_to_config_scope_item() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: Some("item".to_string()),
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert_eq!(config.steps[0].scope, Some(StepScope::Item));
    }

    #[test]
    fn workflow_spec_to_config_safety_git_tag() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: None,
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 5,
                auto_rollback: true,
                checkpoint_strategy: "git_tag".to_string(),
                step_timeout_secs: Some(600),
                binary_snapshot: true,
            },
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert_eq!(config.safety.max_consecutive_failures, 5);
        assert!(config.safety.auto_rollback);
        assert!(matches!(config.safety.checkpoint_strategy, crate::config::CheckpointStrategy::GitTag));
        assert_eq!(config.safety.step_timeout_secs, Some(600));
        assert!(config.safety.binary_snapshot);
    }

    #[test]
    fn workflow_spec_to_config_safety_git_stash() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: None,
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 3,
                auto_rollback: false,
                checkpoint_strategy: "git_stash".to_string(),
                step_timeout_secs: None,
                binary_snapshot: false,
            },
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert!(matches!(config.safety.checkpoint_strategy, crate::config::CheckpointStrategy::GitStash));
    }

    #[test]
    fn workflow_spec_to_config_safety_none_strategy() {
        let spec = WorkflowSpec {
            steps: vec![WorkflowStepSpec {
                id: "qa".to_string(),
                step_type: "qa".to_string(),
                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                command: None,
                scope: None,
            }],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec {
                max_consecutive_failures: 3,
                auto_rollback: false,
                checkpoint_strategy: "unknown_strat".to_string(),
                step_timeout_secs: None,
                binary_snapshot: false,
            },
        };
        let config = workflow_spec_to_config(&spec).unwrap();
        assert!(matches!(config.safety.checkpoint_strategy, crate::config::CheckpointStrategy::None));
    }

    // ── workflow_config_to_spec conversion details ──────────────────

    #[test]
    fn workflow_config_to_spec_cost_preference_mapping() {
        let config = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "perf".to_string(),
                    description: None,

                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Performance),
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "qual".to_string(),
                    description: None,

                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Quality),
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "bal".to_string(),
                    description: None,

                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: Some(CostPreference::Balance),
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.steps[0].cost_preference.as_deref(), Some("performance"));
        assert_eq!(spec.steps[1].cost_preference.as_deref(), Some("quality"));
        assert_eq!(spec.steps[2].cost_preference.as_deref(), Some("balance"));
    }

    #[test]
    fn workflow_config_to_spec_dynamic_steps_roundtrip() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa".to_string(),
                description: None,

                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Infinite,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: false,
                    stop_when_no_unresolved: false,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![crate::dynamic_orchestration::DynamicStepConfig {
                id: "ds1".to_string(),
                description: Some("dynamic".to_string()),
                step_type: "implement".to_string(),
                agent_id: Some("agent-x".to_string()),
                template: Some("tmpl-x".to_string()),
                trigger: Some("on_failure".to_string()),
                priority: 5,
                max_runs: Some(2),
            }],
            safety: crate::config::SafetyConfig::default(),
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.loop_policy.mode, "infinite");
        assert_eq!(spec.dynamic_steps.len(), 1);
        assert_eq!(spec.dynamic_steps[0].id, "ds1");
        assert_eq!(spec.dynamic_steps[0].priority, 5);
        assert_eq!(spec.dynamic_steps[0].max_runs, Some(2));
    }

    // ── export_manifest_resources / export_manifest_documents ────────

    #[test]
    fn export_manifest_resources_includes_all_resource_types() {
        let mut config = make_config();
        // Add one of each
        let ws = dispatch_resource(workspace_manifest("exp-ws", "workspace/exp")).unwrap();
        ws.apply(&mut config);
        let ag = dispatch_resource(agent_manifest("exp-ag", "cmd")).unwrap();
        ag.apply(&mut config);
        let wf = dispatch_resource(workflow_manifest("exp-wf")).unwrap();
        wf.apply(&mut config);
        let pr = dispatch_resource(project_manifest("exp-pr", "d")).unwrap();
        pr.apply(&mut config);

        let resources = export_manifest_resources(&config);
        let kinds: Vec<ResourceKind> = resources.iter().map(|r| r.kind()).collect();
        assert!(kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(kinds.contains(&ResourceKind::Defaults));
        assert!(kinds.contains(&ResourceKind::Workspace));
        assert!(kinds.contains(&ResourceKind::Agent));
        assert!(kinds.contains(&ResourceKind::Workflow));
        assert!(kinds.contains(&ResourceKind::Project));
    }

    #[test]
    fn export_manifest_documents_produces_orchestrator_resources() {
        let mut config = make_config();
        let ws = dispatch_resource(workspace_manifest("doc-ws", "workspace/doc")).unwrap();
        ws.apply(&mut config);

        let docs = export_manifest_documents(&config);
        assert!(!docs.is_empty());
        for doc in &docs {
            assert_eq!(doc.api_version, "orchestrator.dev/v2");
        }
        let doc_kinds: Vec<ResourceKind> = docs.iter().map(|d| d.kind).collect();
        assert!(doc_kinds.contains(&ResourceKind::RuntimePolicy));
        assert!(doc_kinds.contains(&ResourceKind::Defaults));
        assert!(doc_kinds.contains(&ResourceKind::Workspace));
    }

    #[test]
    fn export_manifest_resources_preserves_labels_annotations() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "labeled-ws".to_string(),
                project: None,
                labels: Some([("env".to_string(), "prod".to_string())].into()),
                annotations: Some([("team".to_string(), "infra".to_string())].into()),
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/labeled".to_string(),
                qa_targets: vec![],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let exported = export_manifest_resources(&config);
        let ws = exported.iter().find(|r| r.name() == "labeled-ws");
        assert!(ws.is_some());
        // Verify via get_from
        let loaded = WorkspaceResource::get_from(&config, "labeled-ws").unwrap();
        assert_eq!(loaded.metadata.labels.as_ref().unwrap().get("env").unwrap(), "prod");
        assert_eq!(loaded.metadata.annotations.as_ref().unwrap().get("team").unwrap(), "infra");
    }

    // ── hook_engine / parse_hook_engine ──────────────────────────────

    #[test]
    fn parse_hook_engine_defaults_to_cel() {
        assert!(matches!(parse_hook_engine("cel"), StepHookEngine::Cel));
        assert!(matches!(parse_hook_engine("unknown"), StepHookEngine::Cel));
        assert!(matches!(parse_hook_engine(""), StepHookEngine::Cel));
    }

    #[test]
    fn hook_engine_as_str_returns_cel() {
        assert_eq!(hook_engine_as_str(&StepHookEngine::Cel), "cel");
    }

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

    // ── workspace get_from with/without stored metadata ─────────────

    #[test]
    fn workspace_get_from_without_stored_metadata() {
        let mut config = make_config();
        // Insert workspace directly without resource_meta
        config.workspaces.insert("bare-ws".to_string(), WorkspaceConfig {
            root_path: "/bare".to_string(),
            qa_targets: vec![],
            ticket_dir: "t".to_string(),
            self_referential: false,
        });
        let loaded = WorkspaceResource::get_from(&config, "bare-ws").unwrap();
        assert_eq!(loaded.metadata.name, "bare-ws");
        assert!(loaded.metadata.labels.is_none());
    }

    // ── agent get_from with/without stored metadata ─────────────────

    #[test]
    fn agent_get_from_without_stored_metadata() {
        let mut config = make_config();
        config.agents.insert("bare-ag".to_string(), AgentConfig {
            metadata: AgentMetadata::default(),
            capabilities: vec!["qa".to_string()],
            templates: [("qa".to_string(), "run qa".to_string())].into(),
            selection: AgentSelectionConfig::default(),
        });
        let loaded = AgentResource::get_from(&config, "bare-ag").unwrap();
        assert_eq!(loaded.metadata.name, "bare-ag");
        assert!(loaded.metadata.labels.is_none());
    }

    // ── workflow get_from returns None for missing ───────────────────

    #[test]
    fn workflow_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(WorkflowResource::get_from(&config, "nonexistent-wf").is_none());
    }

    #[test]
    fn agent_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(AgentResource::get_from(&config, "nonexistent-ag").is_none());
    }

    #[test]
    fn workspace_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(WorkspaceResource::get_from(&config, "nonexistent-ws").is_none());
    }

    // ── workspace/agent delete with metadata cleanup ────────────────

    #[test]
    fn workspace_delete_cleans_up_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "meta-ws".to_string(),
                project: None,
                labels: Some([("k".to_string(), "v".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/meta".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);
        assert!(config.resource_meta.workspaces.contains_key("meta-ws"));

        WorkspaceResource::delete_from(&mut config, "meta-ws");
        assert!(!config.resource_meta.workspaces.contains_key("meta-ws"));
    }

    #[test]
    fn agent_delete_cleans_up_metadata() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("meta-ag", "cmd")).unwrap();
        ag.apply(&mut config);
        assert!(config.resource_meta.agents.contains_key("meta-ag"));

        AgentResource::delete_from(&mut config, "meta-ag");
        assert!(!config.resource_meta.agents.contains_key("meta-ag"));
    }

    #[test]
    fn workflow_delete_cleans_up_metadata() {
        let mut config = make_config();
        let wf = dispatch_resource(workflow_manifest("meta-wf")).unwrap();
        wf.apply(&mut config);
        assert!(config.resource_meta.workflows.contains_key("meta-wf"));

        WorkflowResource::delete_from(&mut config, "meta-wf");
        assert!(!config.resource_meta.workflows.contains_key("meta-wf"));
    }

    // ── workflow_config_to_spec prehook roundtrip ───────────────────

    #[test]
    fn workflow_config_to_spec_prehook_roundtrip() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa_testing".to_string(),
                description: None,

                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: Some(StepPrehookConfig {
                    engine: StepHookEngine::Cel,
                    when: "is_last_cycle".to_string(),
                    reason: Some("deferred".to_string()),
                    ui: None,
                    extended: true,
                }),
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };
        let spec = workflow_config_to_spec(&config);
        let prehook = spec.steps[0].prehook.as_ref().unwrap();
        assert_eq!(prehook.engine, "cel");
        assert_eq!(prehook.when, "is_last_cycle");
        assert_eq!(prehook.reason.as_deref(), Some("deferred"));
        assert!(prehook.extended);
    }

    // ── workflow_config_to_spec finalize rules roundtrip ─────────────

    #[test]
    fn workflow_config_to_spec_finalize_rules() {
        let config = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "qa".to_string(),
                description: None,

                required_capability: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
            },
            finalize: WorkflowFinalizeConfig {
                rules: vec![
                    WorkflowFinalizeRule {
                        id: "r1".to_string(),
                        engine: StepHookEngine::Cel,
                        when: "qa_exit == 0".to_string(),
                        status: "passed".to_string(),
                        reason: Some("passed QA".to_string()),
                    },
                    WorkflowFinalizeRule {
                        id: "r2".to_string(),
                        engine: StepHookEngine::Cel,
                        when: "true".to_string(),
                        status: "failed".to_string(),
                        reason: None,
                    },
                ],
            },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };
        let spec = workflow_config_to_spec(&config);
        assert_eq!(spec.finalize.rules.len(), 2);
        assert_eq!(spec.finalize.rules[0].id, "r1");
        assert_eq!(spec.finalize.rules[0].engine, "cel");
        assert_eq!(spec.finalize.rules[0].reason.as_deref(), Some("passed QA"));
        assert_eq!(spec.finalize.rules[1].id, "r2");
        assert!(spec.finalize.rules[1].reason.is_none());
    }

    // ── dispatch_resource unsupported kind ───────────────────────────
    // (This tests the fallback error in dispatch_resource, but since all 6 kinds
    //  are registered, we test the mismatch paths via build_* instead.)

    // ── workspace apply stores metadata ─────────────────────────────

    #[test]
    fn workspace_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workspace,
            metadata: ResourceMetadata {
                name: "store-meta-ws".to_string(),
                project: None,
                labels: Some([("env".to_string(), "staging".to_string())].into()),
                annotations: Some([("note".to_string(), "test".to_string())].into()),
            },
            spec: ResourceSpec::Workspace(WorkspaceSpec {
                root_path: "/store-meta".to_string(),
                qa_targets: vec![],
                ticket_dir: "t".to_string(),
                self_referential: false,
            }),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let stored = config.resource_meta.workspaces.get("store-meta-ws").unwrap();
        assert_eq!(stored.labels.as_ref().unwrap().get("env").unwrap(), "staging");
        assert_eq!(stored.annotations.as_ref().unwrap().get("note").unwrap(), "test");
    }

    #[test]
    fn agent_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Agent,
            metadata: ResourceMetadata {
                name: "store-meta-ag".to_string(),
                project: None,
                labels: Some([("tier".to_string(), "primary".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Agent(Box::new(AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some("run".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            })),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let stored = config.resource_meta.agents.get("store-meta-ag").unwrap();
        assert_eq!(stored.labels.as_ref().unwrap().get("tier").unwrap(), "primary");
    }

    #[test]
    fn workflow_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Workflow,
            metadata: ResourceMetadata {
                name: "store-meta-wf".to_string(),
                project: None,
                labels: Some([("version".to_string(), "v2".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Workflow(WorkflowSpec {
                steps: vec![WorkflowStepSpec {
                    id: "qa".to_string(),
                    step_type: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    command: None,
                    scope: None,
                }],
                loop_policy: WorkflowLoopSpec {
                    mode: "once".to_string(),
                    max_cycles: None,
                    enabled: true,
                    stop_when_no_unresolved: true,
                    agent_template: None,
                },
                finalize: WorkflowFinalizeSpec { rules: vec![] },
                dynamic_steps: vec![],
                safety: SafetySpec::default(),
            }),
        };
        let rr = dispatch_resource(resource).unwrap();
        rr.apply(&mut config);

        let stored = config.resource_meta.workflows.get("store-meta-wf").unwrap();
        assert_eq!(stored.labels.as_ref().unwrap().get("version").unwrap(), "v2");
    }
}

#[test]
fn parse_loop_mode_infinite() {
    let mode = parse_loop_mode("infinite").expect("infinite should parse");
    match mode {
        LoopMode::Infinite => (), // pass
        _ => panic!("expected Infinite"),
    }
}

#[test]
fn parse_loop_mode_fixed() {
    let mode = parse_loop_mode("fixed").expect("fixed should parse");
    assert!(matches!(mode, LoopMode::Fixed));
}

#[test]
fn parse_loop_mode_rejects_invalid() {
    assert!(matches!(parse_loop_mode("once"), Ok(LoopMode::Once)));
    assert!(matches!(parse_loop_mode("fixed"), Ok(LoopMode::Fixed)));
    let invalid = parse_loop_mode("anything_else").expect_err("invalid mode should fail");
    assert!(invalid.to_string().contains("unknown loop mode"));
}

#[test]
fn loop_mode_as_str_returns_correct_values() {
    assert_eq!(loop_mode_as_str(&LoopMode::Once), "once");
    assert_eq!(loop_mode_as_str(&LoopMode::Fixed), "fixed");
    assert_eq!(loop_mode_as_str(&LoopMode::Infinite), "infinite");
}

#[test]
fn agent_validation_rejects_empty_templates() {
    let agent = AgentResource {
        metadata: ResourceMetadata {
            name: "test-agent".to_string(),
            project: None,
            labels: None,
            annotations: None,
        },
        spec: AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: None,
                plan: None,
                qa: None,
                fix: None,
                retest: None,
                loop_guard: None,
                ticket_scan: None,
                build: None,
                test: None,
                lint: None,
                implement: None,
                review: None,
                git_ops: None,
                extra: std::collections::HashMap::new(),
            },
            capabilities: None,
            metadata: None,
            selection: None,
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
            project: None,
            labels: None,
            annotations: None,
        },
        spec: WorkflowSpec {
            steps: vec![],
            loop_policy: WorkflowLoopSpec {
                mode: "once".to_string(),
                max_cycles: None,
                enabled: true,
                stop_when_no_unresolved: true,
                agent_template: None,
            },
            finalize: WorkflowFinalizeSpec { rules: vec![] },
            dynamic_steps: vec![],
            safety: SafetySpec::default(),
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
            project: None,
            labels: Some([("env".to_string(), "test".to_string())].into()),
            annotations: Some([("desc".to_string(), "test workspace".to_string())].into()),
        },
        spec: WorkspaceSpec {
            root_path: "/path/to/workspace".to_string(),
            qa_targets: vec!["docs/qa".to_string(), "tests".to_string()],
            ticket_dir: "tickets".to_string(),
            self_referential: false,
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
            project: None,
            labels: None,
            annotations: None,
        },
        spec: AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: Some("init".to_string()),
                plan: None,
                qa: Some("test".to_string()),
                fix: Some("fix".to_string()),
                retest: Some("retest".to_string()),
                loop_guard: Some("guard".to_string()),
                ticket_scan: None,
                build: None,
                test: None,
                lint: None,
                implement: None,
                review: None,
                git_ops: None,
                extra: std::collections::HashMap::new(),
            },
            capabilities: None,
            metadata: None,
            selection: None,
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
