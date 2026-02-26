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
    WorkflowStepConfig, WorkflowStepType, WorkspaceConfig,
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
    Agent(AgentResource),
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
            ResourceSpec::Agent(self.spec.clone()),
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
            parse_workflow_step_type(&step.step_type)?;
        }
        parse_loop_mode(&self.spec.loop_policy.mode)?;
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
            return Some(Self::Agent(agent));
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
        metadata: AgentMetadata::default(),
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
                    "init_once", "plan", "qa", "ticket_scan", "fix", "retest",
                    "loop_guard", "build", "test", "lint", "implement", "review", "git_ops",
                ].into_iter().collect();
                config.templates.iter()
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
            let step_type = parse_workflow_step_type(&step.step_type)?;
            let is_guard = step_type == WorkflowStepType::LoopGuard;
            let builtin = if matches!(
                step_type,
                WorkflowStepType::InitOnce | WorkflowStepType::LoopGuard
            ) {
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
            Ok(WorkflowStepConfig {
                id: step.id.clone(),
                description: None,
                step_type: Some(step_type),
                required_capability: step.required_capability.clone(),
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
                .step_type
                .as_ref()
                .map(|t| t.as_str().to_string())
                .unwrap_or_default(),
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

fn parse_workflow_step_type(value: &str) -> Result<WorkflowStepType> {
    value.parse::<WorkflowStepType>().map_err(|e| anyhow!(e))
}

fn parse_loop_mode(value: &str) -> Result<LoopMode> {
    value.parse::<LoopMode>().map_err(|e| anyhow!(e))
}

fn loop_mode_as_str(mode: &LoopMode) -> &'static str {
    match mode {
        LoopMode::Once => "once",
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
        resources.push(RegisteredResource::Agent(AgentResource {
            metadata,
            spec: agent_config_to_spec(agent),
        }));
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
            RegisteredResource::Agent(item) => OrchestratorResource {
                api_version: API_VERSION.to_string(),
                kind: ResourceKind::Agent,
                metadata: item.metadata,
                spec: ResourceSpec::Agent(item.spec),
            },
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
            spec: ResourceSpec::Agent(AgentSpec {
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
            }),
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
            spec: ResourceSpec::Agent(AgentSpec {
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
        parse_workflow_step_type("plan").unwrap(),
        WorkflowStepType::Plan
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
    let mode = parse_loop_mode("infinite").expect("infinite should parse");
    match mode {
        LoopMode::Infinite => (), // pass
        _ => panic!("expected Infinite"),
    }
}

#[test]
fn parse_loop_mode_rejects_invalid() {
    assert!(matches!(parse_loop_mode("once"), Ok(LoopMode::Once)));
    let invalid = parse_loop_mode("anything_else").expect_err("invalid mode should fail");
    assert!(invalid.to_string().contains("unknown loop mode"));
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
