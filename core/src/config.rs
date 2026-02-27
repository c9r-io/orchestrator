//! Configuration structures for the orchestrator.
//! Contains all config types: ProjectConfig, OrchestratorConfig, AgentConfig, WorkflowConfig, etc.

use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Main orchestrator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub runner: RunnerConfig,
    pub resume: ResumeConfig,
    pub defaults: ConfigDefaults,
    #[serde(default)]
    pub projects: HashMap<String, ProjectConfig>,
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspaceConfig>,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    pub workflows: HashMap<String, WorkflowConfig>,
    #[serde(default)]
    pub resource_meta: ResourceMetadataStore,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            runner: RunnerConfig::default(),
            resume: ResumeConfig { auto: false },
            defaults: ConfigDefaults {
                project: String::new(),
                workspace: String::new(),
                workflow: String::new(),
            },
            projects: HashMap::new(),
            workspaces: HashMap::new(),
            agents: HashMap::new(),
            workflows: HashMap::new(),
            resource_meta: ResourceMetadataStore::default(),
        }
    }
}

/// Persisted metadata for declarative resources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceMetadataStore {
    #[serde(default)]
    pub workspaces: HashMap<String, ResourceStoredMetadata>,
    #[serde(default)]
    pub agents: HashMap<String, ResourceStoredMetadata>,
    #[serde(default)]
    pub workflows: HashMap<String, ResourceStoredMetadata>,
}

/// Labels and annotations persisted independently from resource specs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceStoredMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

/// Project-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspaceConfig>,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    pub workflows: HashMap<String, WorkflowConfig>,
}

impl ProjectConfig {
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self {
            description: None,
            workspaces: HashMap::new(),
            agents: HashMap::new(),
            workflows: HashMap::new(),
        }
    }
}

/// Default configuration values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDefaults {
    #[serde(default = "default_project")]
    pub project: String,
    pub workspace: String,
    pub workflow: String,
}

fn default_project() -> String {
    "default".to_string()
}

/// Shell runner configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerPolicy {
    #[default]
    Legacy,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerExecutorKind {
    #[default]
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    pub shell: String,
    #[serde(default = "default_shell_arg")]
    pub shell_arg: String,
    #[serde(default)]
    pub policy: RunnerPolicy,
    #[serde(default)]
    pub executor: RunnerExecutorKind,
    #[serde(default = "default_allowed_shells")]
    pub allowed_shells: Vec<String>,
    #[serde(default = "default_allowed_shell_args")]
    pub allowed_shell_args: Vec<String>,
    #[serde(default = "default_env_allowlist")]
    pub env_allowlist: Vec<String>,
    #[serde(default = "default_redaction_patterns")]
    pub redaction_patterns: Vec<String>,
}

fn default_shell_arg() -> String {
    "-lc".to_string()
}

fn default_allowed_shells() -> Vec<String> {
    vec![
        "/bin/bash".to_string(),
        "/bin/zsh".to_string(),
        "/bin/sh".to_string(),
    ]
}

fn default_allowed_shell_args() -> Vec<String> {
    vec!["-lc".to_string(), "-c".to_string()]
}

fn default_env_allowlist() -> Vec<String> {
    vec![
        "PATH".to_string(),
        "HOME".to_string(),
        "USER".to_string(),
        "LANG".to_string(),
        "TERM".to_string(),
    ]
}

fn default_redaction_patterns() -> Vec<String> {
    vec![
        "token".to_string(),
        "password".to_string(),
        "secret".to_string(),
        "api_key".to_string(),
        "authorization".to_string(),
    ]
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            shell: "/bin/bash".to_string(),
            shell_arg: default_shell_arg(),
            policy: RunnerPolicy::Legacy,
            executor: RunnerExecutorKind::Shell,
            allowed_shells: default_allowed_shells(),
            allowed_shell_args: default_allowed_shell_args(),
            env_allowlist: default_env_allowlist(),
            redaction_patterns: default_redaction_patterns(),
        }
    }
}

/// Resume behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeConfig {
    pub auto: bool,
}

/// Workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub root_path: String,
    pub qa_targets: Vec<String>,
    pub ticket_dir: String,
    /// When true, the workspace points to the orchestrator's own source tree
    #[serde(default)]
    pub self_referential: bool,
}

/// Safety configuration for self-bootstrap and dangerous operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// Maximum consecutive failures before auto-rollback
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u32,
    /// Automatically rollback on repeated failures
    #[serde(default)]
    pub auto_rollback: bool,
    /// Strategy for creating checkpoints
    #[serde(default)]
    pub checkpoint_strategy: CheckpointStrategy,
    /// Per-step timeout in seconds (default: 1800 = 30 min)
    #[serde(default)]
    pub step_timeout_secs: Option<u64>,
    /// Snapshot the release binary at cycle start for rollback
    #[serde(default)]
    pub binary_snapshot: bool,
}

fn default_max_consecutive_failures() -> u32 {
    3
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            max_consecutive_failures: 3,
            auto_rollback: false,
            checkpoint_strategy: CheckpointStrategy::default(),
            step_timeout_secs: None,
            binary_snapshot: false,
        }
    }
}

/// Checkpoint strategy for rollback support
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStrategy {
    #[default]
    None,
    GitTag,
    GitStash,
}

/// Build error with source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildError {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
    pub level: BuildErrorLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildErrorLevel {
    Error,
    Warning,
}

/// Test failure with source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    pub test_name: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub stdout: Option<String>,
}

/// Pipeline variables passed between steps
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineVariables {
    /// Key-value store of pipeline variables
    #[serde(default)]
    pub vars: HashMap<String, String>,
    /// Build errors from the last build step
    #[serde(default)]
    pub build_errors: Vec<BuildError>,
    /// Test failures from the last test step
    #[serde(default)]
    pub test_failures: Vec<TestFailure>,
    /// Raw stdout from previous step
    #[serde(default)]
    pub prev_stdout: String,
    /// Raw stderr from previous step
    #[serde(default)]
    pub prev_stderr: String,
    /// Git diff of current cycle
    #[serde(default)]
    pub diff: String,
}

/// Agent metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: Option<String>,
    pub cost: Option<u8>,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub metadata: AgentMetadata,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub templates: HashMap<String, String>,
    #[serde(default)]
    pub selection: AgentSelectionConfig,
}

impl AgentConfig {
    pub fn new() -> Self {
        Self {
            metadata: AgentMetadata::default(),
            capabilities: Vec::new(),
            templates: HashMap::new(),
            selection: AgentSelectionConfig::default(),
        }
    }

    pub fn get_template(&self, capability: &str) -> Option<&String> {
        self.templates.get(capability)
    }

    pub fn supports_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(&capability.to_string())
    }

    #[allow(dead_code)]
    pub fn get_selection_strategy(&self) -> SelectionStrategy {
        self.selection.strategy
    }

    #[allow(dead_code)]
    pub fn get_selection_weights(&self) -> SelectionWeights {
        self.selection.weights.clone().unwrap_or_default()
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent selection configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSelectionConfig {
    #[serde(default = "default_selection_strategy")]
    pub strategy: SelectionStrategy,
    #[serde(default)]
    pub weights: Option<SelectionWeights>,
}

fn default_selection_strategy() -> SelectionStrategy {
    SelectionStrategy::CapabilityAware
}

/// Workflow step type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepType {
    InitOnce,
    Plan,
    Qa,
    TicketScan,
    Fix,
    Retest,
    LoopGuard,
    Build,
    Test,
    Lint,
    Implement,
    Review,
    GitOps,
    // AI SDLC closed-loop step types
    QaDocGen,
    QaTesting,
    TicketFix,
    DocGovernance,
    AlignTests,
    SelfTest,
}

impl WorkflowStepType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InitOnce => "init_once",
            Self::Plan => "plan",
            Self::Qa => "qa",
            Self::TicketScan => "ticket_scan",
            Self::Fix => "fix",
            Self::Retest => "retest",
            Self::LoopGuard => "loop_guard",
            Self::Build => "build",
            Self::Test => "test",
            Self::Lint => "lint",
            Self::Implement => "implement",
            Self::Review => "review",
            Self::GitOps => "git_ops",
            Self::QaDocGen => "qa_doc_gen",
            Self::QaTesting => "qa_testing",
            Self::TicketFix => "ticket_fix",
            Self::DocGovernance => "doc_governance",
            Self::AlignTests => "align_tests",
            Self::SelfTest => "self_test",
        }
    }

    /// Returns true if this step type produces structured output for pipeline variables
    #[allow(dead_code)]
    pub fn has_structured_output(&self) -> bool {
        matches!(
            self,
            Self::Build | Self::Test | Self::Lint | Self::QaTesting | Self::SelfTest
        )
    }
}

impl FromStr for WorkflowStepType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "init_once" => Ok(Self::InitOnce),
            "plan" => Ok(Self::Plan),
            "qa" => Ok(Self::Qa),
            "ticket_scan" => Ok(Self::TicketScan),
            "fix" => Ok(Self::Fix),
            "retest" => Ok(Self::Retest),
            "loop_guard" => Ok(Self::LoopGuard),
            "build" => Ok(Self::Build),
            "test" => Ok(Self::Test),
            "lint" => Ok(Self::Lint),
            "implement" => Ok(Self::Implement),
            "review" => Ok(Self::Review),
            "git_ops" => Ok(Self::GitOps),
            "qa_doc_gen" => Ok(Self::QaDocGen),
            "qa_testing" => Ok(Self::QaTesting),
            "ticket_fix" => Ok(Self::TicketFix),
            "doc_governance" => Ok(Self::DocGovernance),
            "align_tests" => Ok(Self::AlignTests),
            "self_test" => Ok(Self::SelfTest),
            _ => Err(format!("unknown workflow step type: {}", value)),
        }
    }
}

/// Step hook engine type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepHookEngine {
    #[default]
    Cel,
}

/// Prehook UI mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepPrehookUiMode {
    Visual,
    Cel,
}

/// Prehook UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepPrehookUiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<StepPrehookUiMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<serde_json::Value>,
}

/// Step prehook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepPrehookConfig {
    #[serde(default)]
    pub engine: StepHookEngine,
    pub when: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<StepPrehookUiConfig>,
    #[serde(default)]
    pub extended: bool,
}

/// Workflow finalize rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFinalizeRule {
    pub id: String,
    #[serde(default)]
    pub engine: StepHookEngine,
    pub when: String,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Workflow finalize configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowFinalizeConfig {
    #[serde(default)]
    pub rules: Vec<WorkflowFinalizeRule>,
}

/// Loop mode enumeration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    #[default]
    Once,
    Infinite,
}

impl FromStr for LoopMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "once" => Ok(Self::Once),
            "infinite" => Ok(Self::Infinite),
            _ => Err(format!(
                "unknown loop mode: {} (expected once|infinite)",
                value
            )),
        }
    }
}

/// Workflow loop guard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLoopGuardConfig {
    pub enabled: bool,
    pub stop_when_no_unresolved: bool,
    pub max_cycles: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_template: Option<String>,
}

impl Default for WorkflowLoopGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            stop_when_no_unresolved: true,
            max_cycles: None,
            agent_template: None,
        }
    }
}

/// Workflow loop configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowLoopConfig {
    pub mode: LoopMode,
    #[serde(default)]
    pub guard: WorkflowLoopGuardConfig,
}

/// Cost preference enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CostPreference {
    Performance,
    Quality,
    #[default]
    Balance,
}

/// Workflow step configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub step_type: Option<WorkflowStepType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub repeatable: bool,
    #[serde(default)]
    pub is_guard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<CostPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<StepPrehookConfig>,
    #[serde(default)]
    pub tty: bool,
    /// Named outputs this step produces (for pipeline variable passing)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// Pipe this step's output to the named step as input
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    /// Build command for builtin build/test/lint steps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Task execution step (runtime representation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionStep {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_type: Option<WorkflowStepType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub repeatable: bool,
    #[serde(default)]
    pub is_guard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<CostPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<StepPrehookConfig>,
    #[serde(default)]
    pub tty: bool,
    /// Named outputs this step produces (for pipeline variable passing)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// Pipe this step's output to the named step as input
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    /// Build command for builtin build/test/lint steps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Task execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionPlan {
    pub steps: Vec<TaskExecutionStep>,
    #[serde(rename = "loop")]
    pub loop_policy: WorkflowLoopConfig,
    #[serde(default)]
    pub finalize: WorkflowFinalizeConfig,
}

impl TaskExecutionPlan {
    pub fn step(&self, step_type: WorkflowStepType) -> Option<&TaskExecutionStep> {
        self.steps
            .iter()
            .find(|step| step.step_type.as_ref() == Some(&step_type))
    }

    #[allow(dead_code)]
    pub fn step_by_id(&self, id: &str) -> Option<&TaskExecutionStep> {
        self.steps.iter().find(|step| step.id == id)
    }
}

/// Workflow configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub steps: Vec<WorkflowStepConfig>,
    #[serde(rename = "loop", default)]
    pub loop_policy: WorkflowLoopConfig,
    #[serde(default)]
    pub finalize: WorkflowFinalizeConfig,
    #[serde(default)]
    pub qa: Option<String>,
    #[serde(default)]
    pub fix: Option<String>,
    #[serde(default)]
    pub retest: Option<String>,
    #[serde(default)]
    pub dynamic_steps: Vec<crate::dynamic_orchestration::DynamicStepConfig>,
    /// Safety configuration for self-bootstrap scenarios
    #[serde(default)]
    pub safety: SafetyConfig,
}

/// Resolved workspace (with absolute paths)
#[derive(Debug, Clone)]
pub struct ResolvedWorkspace {
    pub root_path: std::path::PathBuf,
    pub qa_targets: Vec<String>,
    pub ticket_dir: String,
}

/// Resolved project
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResolvedProject {
    pub workspaces: HashMap<String, ResolvedWorkspace>,
    pub agents: HashMap<String, AgentConfig>,
    pub workflows: HashMap<String, WorkflowConfig>,
}

/// Active configuration (runtime state)
#[derive(Debug, Clone)]
pub struct ActiveConfig {
    pub config: OrchestratorConfig,
    pub workspaces: HashMap<String, ResolvedWorkspace>,
    pub projects: HashMap<String, ResolvedProject>,
    pub default_project_id: String,
    pub default_workspace_id: String,
    pub default_workflow_id: String,
}

/// Task runtime context
#[derive(Debug, Clone)]
pub struct TaskRuntimeContext {
    pub workspace_id: String,
    pub workspace_root: std::path::PathBuf,
    pub ticket_dir: String,
    pub execution_plan: TaskExecutionPlan,
    pub current_cycle: u32,
    pub init_done: bool,
    pub dynamic_steps: Vec<crate::dynamic_orchestration::DynamicStepConfig>,
    /// Pipeline variables accumulated across steps in the current cycle
    pub pipeline_vars: PipelineVariables,
    /// Safety configuration
    pub safety: SafetyConfig,
    /// Whether the workspace is self-referential
    pub self_referential: bool,
    /// Consecutive failure counter for auto-rollback
    pub consecutive_failures: u32,
}

/// Step prehook context for evaluation
#[derive(Debug, Clone, Serialize)]
pub struct StepPrehookContext {
    pub task_id: String,
    pub task_item_id: String,
    pub cycle: u32,
    pub step: String,
    pub qa_file_path: String,
    pub item_status: String,
    pub task_status: String,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub qa_failed: bool,
    pub fix_required: bool,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_has_changes: Option<bool>,
    pub upstream_artifacts: Vec<ArtifactSummary>,
    /// Number of build errors from the last build step
    pub build_error_count: i64,
    /// Number of test failures from the last test step
    pub test_failure_count: i64,
    /// Exit code of the last build step
    pub build_exit_code: Option<i64>,
    /// Exit code of the last test step
    pub test_exit_code: Option<i64>,
    /// Exit code of the last self_test step
    pub self_test_exit_code: Option<i64>,
    /// Whether the last self_test step passed
    pub self_test_passed: bool,
}

/// Artifact summary
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactSummary {
    pub phase: String,
    pub kind: String,
    pub path: Option<String>,
}

/// Item finalize context
#[derive(Debug, Clone, Serialize)]
pub struct ItemFinalizeContext {
    pub task_id: String,
    pub task_item_id: String,
    pub cycle: u32,
    pub qa_file_path: String,
    pub item_status: String,
    pub task_status: String,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub retest_new_ticket_count: i64,
    pub qa_failed: bool,
    pub fix_required: bool,
    pub qa_enabled: bool,
    pub qa_ran: bool,
    pub qa_skipped: bool,
    pub fix_enabled: bool,
    pub fix_ran: bool,
    pub fix_success: bool,
    pub retest_enabled: bool,
    pub retest_ran: bool,
    pub retest_success: bool,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_confidence: Option<f32>,
    pub fix_quality_score: Option<f32>,
    pub total_artifacts: i64,
    pub has_ticket_artifacts: bool,
    pub has_code_change_artifacts: bool,
}

/// Workflow finalize outcome
#[derive(Debug, Clone)]
pub struct WorkflowFinalizeOutcome {
    pub rule_id: String,
    pub status: String,
    pub reason: String,
}

/// Helper to create a WorkflowStepConfig with new fields defaulted
fn step_config(
    id: &str,
    step_type: Option<WorkflowStepType>,
    required_capability: Option<&str>,
    builtin: Option<&str>,
    enabled: bool,
    repeatable: bool,
    tty: bool,
) -> WorkflowStepConfig {
    WorkflowStepConfig {
        id: id.to_string(),
        description: None,
        step_type,
        required_capability: required_capability.map(String::from),
        builtin: builtin.map(String::from),
        enabled,
        repeatable,
        is_guard: false,
        cost_preference: None,
        prehook: None,
        tty,
        outputs: Vec::new(),
        pipe_to: None,
        command: None,
    }
}

/// Default workflow steps builder
pub fn default_workflow_steps(
    qa: Option<&str>,
    ticket_scan: bool,
    fix: Option<&str>,
    retest: Option<&str>,
) -> Vec<WorkflowStepConfig> {
    vec![
        step_config(
            "init_once",
            Some(WorkflowStepType::InitOnce),
            None,
            Some("init_once"),
            false,
            false,
            false,
        ),
        step_config(
            "plan",
            Some(WorkflowStepType::Plan),
            Some("plan"),
            None,
            false,
            false,
            true,
        ),
        step_config(
            "qa",
            Some(WorkflowStepType::Qa),
            Some("qa"),
            None,
            qa.is_some(),
            true,
            false,
        ),
        step_config(
            "ticket_scan",
            Some(WorkflowStepType::TicketScan),
            None,
            Some("ticket_scan"),
            ticket_scan,
            true,
            false,
        ),
        step_config(
            "fix",
            Some(WorkflowStepType::Fix),
            Some("fix"),
            None,
            fix.is_some(),
            true,
            false,
        ),
        step_config(
            "retest",
            Some(WorkflowStepType::Retest),
            Some("retest"),
            None,
            retest.is_some(),
            true,
            false,
        ),
    ]
}

/// Default workflow finalize config
pub fn default_workflow_finalize_config() -> WorkflowFinalizeConfig {
    WorkflowFinalizeConfig {
        rules: vec![
            WorkflowFinalizeRule {
                id: "skip_without_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "(qa_skipped == true || qa_enabled == false) && active_ticket_count == 0"
                    .to_string(),
                status: "skipped".to_string(),
                reason: Some("qa skipped and no tickets".to_string()),
            },
            WorkflowFinalizeRule {
                id: "qa_passed_without_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "qa_ran == true && qa_exit_code == 0 && active_ticket_count == 0"
                    .to_string(),
                status: "qa_passed".to_string(),
                reason: Some("qa passed with no tickets".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_disabled_with_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_enabled == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix disabled by workflow".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_failed".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_ran == true && fix_success == false".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix failed".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fixed_without_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_success == true && retest_enabled == false".to_string(),
                status: "fixed".to_string(),
                reason: Some("fixed without retest".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fix_skipped_and_retest_disabled".to_string(),
                engine: StepHookEngine::Cel,
                when: "fix_enabled == true && fix_ran == false && fix_success == false && retest_enabled == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix skipped by prehook and retest disabled".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fixed_retest_skipped_after_fix_success".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_enabled == true && retest_ran == false && fix_success == true"
                    .to_string(),
                status: "fixed".to_string(),
                reason: Some("retest skipped by prehook".to_string()),
            },
            WorkflowFinalizeRule {
                id: "unresolved_retest_skipped_without_fix".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_enabled == true && retest_ran == false && fix_success == false && active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("fix skipped by prehook and retest skipped by prehook".to_string()),
            },
            WorkflowFinalizeRule {
                id: "verified_after_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_ran == true && retest_success == true && retest_new_ticket_count == 0"
                    .to_string(),
                status: "verified".to_string(),
                reason: Some("retest passed".to_string()),
            },
            WorkflowFinalizeRule {
                id: "unresolved_after_retest".to_string(),
                engine: StepHookEngine::Cel,
                when: "retest_ran == true && (retest_success == false || retest_new_ticket_count > 0)"
                    .to_string(),
                status: "unresolved".to_string(),
                reason: Some("retest still failing".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fallback_unresolved_with_tickets".to_string(),
                engine: StepHookEngine::Cel,
                when: "active_ticket_count > 0".to_string(),
                status: "unresolved".to_string(),
                reason: Some("unresolved tickets remain".to_string()),
            },
            WorkflowFinalizeRule {
                id: "fallback_qa_passed".to_string(),
                engine: StepHookEngine::Cel,
                when: "active_ticket_count == 0".to_string(),
                status: "qa_passed".to_string(),
                reason: Some("no active tickets".to_string()),
            },
        ],
    }
}
