//! Configuration structures for the orchestrator.
//! Contains all config types: ProjectConfig, OrchestratorConfig, AgentConfig, WorkflowConfig, etc.

use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Maximum byte length for a pipeline variable value to remain inline.
/// Values exceeding this are spilled to a file and the inline value is truncated.
/// 4 KB leaves headroom for bash escaping inflation (~1.5-2x) plus template
/// boilerplate within the 16 KB runner safety limit.
pub const PIPELINE_VAR_INLINE_LIMIT: usize = 4096;

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

/// Execution scope for a workflow step
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepScope {
    /// Runs once per cycle (plan, implement, self_test, align_tests, doc_governance)
    Task,
    /// Runs per item/QA file (qa_testing, ticket_fix)
    #[default]
    Item,
}

// ── Step Behavior declarations ─────────────────────────────────────

/// Declarative behavior attached to each workflow step.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepBehavior {
    #[serde(default)]
    pub on_failure: OnFailureAction,
    #[serde(default)]
    pub on_success: OnSuccessAction,
    #[serde(default)]
    pub captures: Vec<CaptureDecl>,
    #[serde(default)]
    pub post_actions: Vec<PostAction>,
    #[serde(default)]
    pub execution: ExecutionMode,
    #[serde(default)]
    pub collect_artifacts: bool,
}

/// What to do when a step fails.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnFailureAction {
    #[default]
    Continue,
    SetStatus {
        status: String,
    },
    EarlyReturn {
        status: String,
    },
}

/// What to do when a step succeeds.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnSuccessAction {
    #[default]
    Continue,
    SetStatus {
        status: String,
    },
}

/// A single capture declaration: what to extract from a step result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureDecl {
    pub var: String,
    pub source: CaptureSource,
}

/// Source of a captured value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    Stdout,
    Stderr,
    ExitCode,
    FailedFlag,
    SuccessFlag,
}

/// Post-step action to run after a step completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PostAction {
    CreateTicket,
    ScanTickets,
}

/// How a step is executed.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExecutionMode {
    #[default]
    Agent,
    Builtin {
        name: String,
    },
    Chain,
}

/// Resolved semantic meaning for a workflow step after applying defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSemanticKind {
    Builtin { name: String },
    Agent { capability: String },
    Command,
    Chain,
}

/// Known workflow step IDs
const KNOWN_STEP_IDS: &[&str] = &[
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
    "qa_doc_gen",
    "qa_testing",
    "ticket_fix",
    "doc_governance",
    "align_tests",
    "self_test",
    "smoke_chain",
];

const KNOWN_BUILTIN_STEP_NAMES: &[&str] = &["init_once", "loop_guard", "ticket_scan", "self_test"];

/// Validate that a step type string is a known step ID.
pub fn validate_step_type(value: &str) -> Result<String, String> {
    if KNOWN_STEP_IDS.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!("unknown workflow step type: {}", value))
    }
}

pub fn is_known_builtin_step_name(value: &str) -> bool {
    KNOWN_BUILTIN_STEP_NAMES.contains(&value)
}

pub fn default_builtin_for_step_id(step_id: &str) -> Option<&'static str> {
    match step_id {
        "init_once" => Some("init_once"),
        "loop_guard" => Some("loop_guard"),
        "ticket_scan" => Some("ticket_scan"),
        "self_test" => Some("self_test"),
        _ => None,
    }
}

pub fn default_required_capability_for_step_id(step_id: &str) -> Option<&'static str> {
    match step_id {
        "qa" => Some("qa"),
        "fix" => Some("fix"),
        "retest" => Some("retest"),
        "plan" => Some("plan"),
        "build" => Some("build"),
        "test" => Some("test"),
        "lint" => Some("lint"),
        "implement" => Some("implement"),
        "review" => Some("review"),
        "git_ops" => Some("git_ops"),
        "qa_doc_gen" => Some("qa_doc_gen"),
        "qa_testing" => Some("qa_testing"),
        "ticket_fix" => Some("ticket_fix"),
        "doc_governance" => Some("doc_governance"),
        "align_tests" => Some("align_tests"),
        "smoke_chain" => Some("smoke_chain"),
        _ => None,
    }
}

pub fn resolve_step_semantic_kind(step: &WorkflowStepConfig) -> Result<StepSemanticKind, String> {
    if step.builtin.is_some() && step.required_capability.is_some() {
        return Err(format!(
            "step '{}' cannot define both builtin and required_capability",
            step.id
        ));
    }

    if !step.chain_steps.is_empty() {
        return Ok(StepSemanticKind::Chain);
    }

    if step.command.is_some() {
        return Ok(StepSemanticKind::Command);
    }

    if let Some(ref builtin) = step.builtin {
        if !is_known_builtin_step_name(builtin) {
            return Err(format!(
                "step '{}' uses unknown builtin '{}'",
                step.id, builtin
            ));
        }
        return Ok(StepSemanticKind::Builtin {
            name: builtin.clone(),
        });
    }

    if let Some(ref capability) = step.required_capability {
        return Ok(StepSemanticKind::Agent {
            capability: capability.clone(),
        });
    }

    if let Some(builtin) = default_builtin_for_step_id(&step.id) {
        return Ok(StepSemanticKind::Builtin {
            name: builtin.to_string(),
        });
    }

    if let Some(capability) = default_required_capability_for_step_id(&step.id) {
        return Ok(StepSemanticKind::Agent {
            capability: capability.to_string(),
        });
    }

    Err(format!(
        "step '{}' is missing builtin, required_capability, command, or chain_steps",
        step.id
    ))
}

pub fn normalize_step_execution_mode(step: &mut WorkflowStepConfig) -> Result<(), String> {
    match resolve_step_semantic_kind(step)? {
        StepSemanticKind::Builtin { name } => {
            step.builtin = Some(name.clone());
            step.required_capability = None;
            step.behavior.execution = ExecutionMode::Builtin { name };
        }
        StepSemanticKind::Agent { capability } => {
            step.required_capability = Some(capability);
            step.behavior.execution = ExecutionMode::Agent;
        }
        StepSemanticKind::Command => {
            step.behavior.execution = ExecutionMode::Builtin {
                name: step.id.clone(),
            };
        }
        StepSemanticKind::Chain => {
            step.behavior.execution = ExecutionMode::Chain;
        }
    }
    Ok(())
}

/// Returns true if a step ID produces structured output for pipeline variables
pub fn has_structured_output(step_id: &str) -> bool {
    matches!(
        step_id,
        "build" | "test" | "lint" | "qa_testing" | "self_test" | "smoke_chain"
    )
}

/// Returns the default execution scope for a step ID.
/// Task-scoped steps run once per cycle; item-scoped steps fan-out per QA file.
pub fn default_scope_for_step_id(step_id: &str) -> StepScope {
    match step_id {
        // Item-scoped: fan-out per QA file
        "qa" | "qa_testing" | "ticket_fix" | "ticket_scan" | "fix" | "retest" => StepScope::Item,
        // Everything else defaults to task-scoped
        _ => StepScope::Task,
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
    Fixed,
    Infinite,
}

impl FromStr for LoopMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "once" => Ok(Self::Once),
            "fixed" => Ok(Self::Fixed),
            "infinite" => Ok(Self::Infinite),
            _ => Err(format!(
                "unknown loop mode: {} (expected once|fixed|infinite)",
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
    /// Sub-steps to execute in sequence for smoke_chain step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain_steps: Vec<WorkflowStepConfig>,
    /// Execution scope (defaults based on step id)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<StepScope>,
    /// Declarative step behavior (on_failure, captures, post_actions, etc.)
    #[serde(default)]
    pub behavior: StepBehavior,
}

fn default_true() -> bool {
    true
}

/// Task execution step (runtime representation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionStep {
    pub id: String,
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
    /// Sub-steps to execute in sequence for smoke_chain step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain_steps: Vec<TaskExecutionStep>,
    /// Execution scope override (defaults based on step type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<StepScope>,
    /// Declarative step behavior (on_failure, captures, post_actions, etc.)
    #[serde(default)]
    pub behavior: StepBehavior,
}

impl TaskExecutionStep {
    /// Returns the resolved scope: explicit override or default based on step id.
    pub fn resolved_scope(&self) -> StepScope {
        self.scope
            .unwrap_or_else(|| default_scope_for_step_id(&self.id))
    }
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
    /// Find step by string id
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
    /// Maximum number of cycles configured for this workflow
    pub max_cycles: u32,
    /// Whether this is the last cycle (cycle == max_cycles)
    pub is_last_cycle: bool,
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
    pub is_last_cycle: bool,
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
    required_capability: Option<&str>,
    builtin: Option<&str>,
    enabled: bool,
    repeatable: bool,
    tty: bool,
) -> WorkflowStepConfig {
    WorkflowStepConfig {
        id: id.to_string(),
        description: None,
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
        chain_steps: vec![],
        scope: None,
        behavior: StepBehavior::default(),
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
        step_config("init_once", None, Some("init_once"), false, false, false),
        step_config("plan", Some("plan"), None, false, false, true),
        step_config("qa", Some("qa"), None, qa.is_some(), true, false),
        step_config(
            "ticket_scan",
            None,
            Some("ticket_scan"),
            ticket_scan,
            true,
            false,
        ),
        step_config("fix", Some("fix"), None, fix.is_some(), true, false),
        step_config(
            "retest",
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
                when: "(qa_skipped == true || qa_enabled == false) && active_ticket_count == 0 && is_last_cycle"
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

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Default impls =====

    #[test]
    fn test_orchestrator_config_default() {
        let cfg = OrchestratorConfig::default();
        assert!(cfg.projects.is_empty());
        assert!(cfg.workspaces.is_empty());
        assert!(cfg.agents.is_empty());
        assert!(cfg.workflows.is_empty());
        assert!(!cfg.resume.auto);
        assert_eq!(cfg.defaults.project, "");
        assert_eq!(cfg.defaults.workspace, "");
        assert_eq!(cfg.defaults.workflow, "");
    }

    #[test]
    fn test_runner_config_default() {
        let cfg = RunnerConfig::default();
        assert_eq!(cfg.shell, "/bin/bash");
        assert_eq!(cfg.shell_arg, "-lc");
        assert_eq!(cfg.policy, RunnerPolicy::Legacy);
        assert_eq!(cfg.executor, RunnerExecutorKind::Shell);
        assert_eq!(cfg.allowed_shells.len(), 3);
        assert!(cfg.allowed_shells.contains(&"/bin/bash".to_string()));
        assert_eq!(cfg.allowed_shell_args, vec!["-lc", "-c"]);
        assert!(cfg.env_allowlist.contains(&"PATH".to_string()));
        assert!(cfg.env_allowlist.contains(&"HOME".to_string()));
        assert!(cfg.redaction_patterns.contains(&"token".to_string()));
        assert!(cfg.redaction_patterns.contains(&"secret".to_string()));
    }

    #[test]
    fn test_safety_config_default() {
        let cfg = SafetyConfig::default();
        assert_eq!(cfg.max_consecutive_failures, 3);
        assert!(!cfg.auto_rollback);
        assert!(matches!(cfg.checkpoint_strategy, CheckpointStrategy::None));
        assert!(cfg.step_timeout_secs.is_none());
        assert!(!cfg.binary_snapshot);
    }

    #[test]
    fn test_workflow_loop_guard_default() {
        let cfg = WorkflowLoopGuardConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.stop_when_no_unresolved);
        assert!(cfg.max_cycles.is_none());
        assert!(cfg.agent_template.is_none());
    }

    #[test]
    fn test_agent_config_default_and_new() {
        let cfg = AgentConfig::default();
        assert!(cfg.capabilities.is_empty());
        assert!(cfg.templates.is_empty());
        assert_eq!(cfg.metadata.name, "");
        assert!(cfg.metadata.description.is_none());
        assert!(cfg.metadata.version.is_none());
        assert!(cfg.metadata.cost.is_none());

        let cfg2 = AgentConfig::new();
        assert!(cfg2.capabilities.is_empty());
    }

    #[test]
    fn test_resource_metadata_store_default() {
        let store = ResourceMetadataStore::default();
        assert!(store.workspaces.is_empty());
        assert!(store.agents.is_empty());
        assert!(store.workflows.is_empty());
    }

    #[test]
    fn test_resource_stored_metadata_default() {
        let meta = ResourceStoredMetadata::default();
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }

    #[test]
    fn test_pipeline_variables_default() {
        let pv = PipelineVariables::default();
        assert!(pv.vars.is_empty());
        assert!(pv.build_errors.is_empty());
        assert!(pv.test_failures.is_empty());
        assert_eq!(pv.prev_stdout, "");
        assert_eq!(pv.prev_stderr, "");
        assert_eq!(pv.diff, "");
    }

    #[test]
    fn test_loop_mode_default() {
        let mode = LoopMode::default();
        assert!(matches!(mode, LoopMode::Once));
    }

    #[test]
    fn test_step_scope_default() {
        let scope = StepScope::default();
        assert_eq!(scope, StepScope::Item);
    }

    #[test]
    fn test_cost_preference_default() {
        let pref = CostPreference::default();
        assert_eq!(pref, CostPreference::Balance);
    }

    #[test]
    fn test_checkpoint_strategy_default() {
        let strat = CheckpointStrategy::default();
        assert!(matches!(strat, CheckpointStrategy::None));
    }

    #[test]
    fn test_step_hook_engine_default() {
        let engine = StepHookEngine::default();
        assert!(matches!(engine, StepHookEngine::Cel));
    }

    #[test]
    fn test_runner_policy_default() {
        let policy = RunnerPolicy::default();
        assert_eq!(policy, RunnerPolicy::Legacy);
    }

    #[test]
    fn test_runner_executor_kind_default() {
        let kind = RunnerExecutorKind::default();
        assert_eq!(kind, RunnerExecutorKind::Shell);
    }

    // ===== FromStr impls =====

    #[test]
    fn test_loop_mode_from_str_valid() {
        assert!(matches!(
            LoopMode::from_str("once").unwrap(),
            LoopMode::Once
        ));
        assert!(matches!(
            LoopMode::from_str("fixed").unwrap(),
            LoopMode::Fixed
        ));
        assert!(matches!(
            LoopMode::from_str("infinite").unwrap(),
            LoopMode::Infinite
        ));
    }

    #[test]
    fn test_loop_mode_from_str_invalid() {
        let err = LoopMode::from_str("bogus").unwrap_err();
        assert!(err.contains("unknown loop mode"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn test_validate_step_type_known_ids() {
        for id in &[
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
            "qa_doc_gen",
            "qa_testing",
            "ticket_fix",
            "doc_governance",
            "align_tests",
            "self_test",
            "smoke_chain",
        ] {
            assert!(validate_step_type(id).is_ok(), "expected valid for {}", id);
        }
    }

    #[test]
    fn test_validate_step_type_unknown_id() {
        let result = validate_step_type("my_custom_step");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown workflow step type"));
    }

    // ===== has_structured_output =====

    #[test]
    fn test_has_structured_output() {
        assert!(has_structured_output("build"));
        assert!(has_structured_output("test"));
        assert!(has_structured_output("lint"));
        assert!(has_structured_output("qa_testing"));
        assert!(has_structured_output("self_test"));
        assert!(has_structured_output("smoke_chain"));

        assert!(!has_structured_output("plan"));
        assert!(!has_structured_output("fix"));
        assert!(!has_structured_output("implement"));
        assert!(!has_structured_output("review"));
        assert!(!has_structured_output("qa"));
        assert!(!has_structured_output("doc_governance"));
    }

    // ===== default_scope_for_step_id =====

    #[test]
    fn test_default_scope_task_steps() {
        let task_scoped = vec![
            "plan",
            "qa_doc_gen",
            "implement",
            "self_test",
            "align_tests",
            "doc_governance",
            "review",
            "build",
            "test",
            "lint",
            "git_ops",
            "smoke_chain",
            "loop_guard",
            "init_once",
        ];
        for id in task_scoped {
            assert_eq!(
                default_scope_for_step_id(id),
                StepScope::Task,
                "expected Task for {}",
                id
            );
        }
    }

    #[test]
    fn test_default_scope_item_steps() {
        let item_scoped = vec![
            "qa",
            "qa_testing",
            "ticket_fix",
            "ticket_scan",
            "fix",
            "retest",
        ];
        for id in item_scoped {
            assert_eq!(
                default_scope_for_step_id(id),
                StepScope::Item,
                "expected Item for {}",
                id
            );
        }
    }

    // ===== AgentConfig methods =====

    #[test]
    fn test_agent_supports_capability() {
        let mut agent = AgentConfig::new();
        agent.capabilities = vec!["plan".to_string(), "qa".to_string()];
        assert!(agent.supports_capability("plan"));
        assert!(agent.supports_capability("qa"));
        assert!(!agent.supports_capability("fix"));
    }

    #[test]
    fn test_agent_get_template() {
        let mut agent = AgentConfig::new();
        agent
            .templates
            .insert("plan".to_string(), "plan template".to_string());
        assert_eq!(
            agent.get_template("plan"),
            Some(&"plan template".to_string())
        );
        assert_eq!(agent.get_template("fix"), None);
    }

    // ===== TaskExecutionStep::resolved_scope =====

    #[test]
    fn test_resolved_scope_explicit_override() {
        let step = TaskExecutionStep {
            id: "qa".to_string(), // default would be Item
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: Some(StepScope::Task), // explicit override
            behavior: StepBehavior::default(),
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn test_resolved_scope_from_step_id() {
        let step = TaskExecutionStep {
            id: "plan".to_string(),
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
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
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn test_resolved_scope_unknown_id_defaults_to_task() {
        let step = TaskExecutionStep {
            id: "my_custom_step".to_string(),
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
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
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    // ===== TaskExecutionPlan::step_by_id =====

    #[test]
    fn test_task_execution_plan_step_by_id_found() {
        let plan = TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".to_string(),
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
                },
                TaskExecutionStep {
                    id: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: true,
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
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
        };

        let found = plan.step_by_id("qa");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "qa");

        let found_plan = plan.step_by_id("plan");
        assert!(found_plan.is_some());
        assert_eq!(found_plan.unwrap().id, "plan");
    }

    #[test]
    fn test_task_execution_plan_step_by_id_not_found() {
        let plan = TaskExecutionPlan {
            steps: vec![],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
        };
        assert!(plan.step_by_id("fix").is_none());
    }

    // ===== default_workflow_steps =====

    #[test]
    fn test_default_workflow_steps_all_disabled() {
        let steps = default_workflow_steps(None, false, None, None);
        assert_eq!(steps.len(), 6);
        // qa disabled
        let qa = steps.iter().find(|s| s.id == "qa").unwrap();
        assert!(!qa.enabled);
        // ticket_scan disabled
        let ts = steps.iter().find(|s| s.id == "ticket_scan").unwrap();
        assert!(!ts.enabled);
        // fix disabled
        let fix = steps.iter().find(|s| s.id == "fix").unwrap();
        assert!(!fix.enabled);
        // retest disabled
        let retest = steps.iter().find(|s| s.id == "retest").unwrap();
        assert!(!retest.enabled);
    }

    #[test]
    fn test_default_workflow_steps_all_enabled() {
        let steps = default_workflow_steps(
            Some("qa_agent"),
            true,
            Some("fix_agent"),
            Some("retest_agent"),
        );
        let qa = steps.iter().find(|s| s.id == "qa").unwrap();
        assert!(qa.enabled);
        let ts = steps.iter().find(|s| s.id == "ticket_scan").unwrap();
        assert!(ts.enabled);
        let fix = steps.iter().find(|s| s.id == "fix").unwrap();
        assert!(fix.enabled);
        let retest = steps.iter().find(|s| s.id == "retest").unwrap();
        assert!(retest.enabled);
    }

    #[test]
    fn test_default_workflow_steps_tty_flags() {
        let steps = default_workflow_steps(None, false, None, None);
        // only plan should have tty=true
        let plan = steps.iter().find(|s| s.id == "plan").unwrap();
        assert!(plan.tty);
        for s in steps.iter().filter(|s| s.id != "plan") {
            assert!(!s.tty, "step {} should not have tty", s.id);
        }
    }

    #[test]
    fn test_default_workflow_steps_repeatable() {
        let steps = default_workflow_steps(Some("qa"), true, Some("fix"), Some("retest"));
        let init = steps.iter().find(|s| s.id == "init_once").unwrap();
        assert!(!init.repeatable);
        let plan = steps.iter().find(|s| s.id == "plan").unwrap();
        assert!(!plan.repeatable);
        // qa, ticket_scan, fix, retest are repeatable
        for id in &["qa", "ticket_scan", "fix", "retest"] {
            let s = steps.iter().find(|s| s.id == *id).unwrap();
            assert!(s.repeatable, "step {} should be repeatable", id);
        }
    }

    // ===== default_workflow_finalize_config =====

    #[test]
    fn test_default_workflow_finalize_config_rule_count() {
        let cfg = default_workflow_finalize_config();
        assert_eq!(cfg.rules.len(), 12);
    }

    #[test]
    fn test_default_workflow_finalize_config_skip_without_tickets_has_is_last_cycle() {
        let cfg = default_workflow_finalize_config();
        let rule = cfg
            .rules
            .iter()
            .find(|r| r.id == "skip_without_tickets")
            .unwrap();
        assert!(
            rule.when.contains("is_last_cycle"),
            "skip_without_tickets must include is_last_cycle guard"
        );
        assert_eq!(rule.status, "skipped");
    }

    #[test]
    fn test_default_workflow_finalize_config_rule_ids_unique() {
        let cfg = default_workflow_finalize_config();
        let mut ids: Vec<&str> = cfg.rules.iter().map(|r| r.id.as_str()).collect();
        let original_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), original_len, "finalize rule IDs must be unique");
    }

    #[test]
    fn test_default_workflow_finalize_config_all_rules_have_reasons() {
        let cfg = default_workflow_finalize_config();
        for rule in &cfg.rules {
            assert!(
                rule.reason.is_some(),
                "rule {} should have a reason",
                rule.id
            );
        }
    }

    #[test]
    fn test_default_workflow_finalize_config_fallback_rules_last() {
        let cfg = default_workflow_finalize_config();
        let last_two: Vec<&str> = cfg
            .rules
            .iter()
            .rev()
            .take(2)
            .map(|r| r.id.as_str())
            .collect();
        assert!(last_two.contains(&"fallback_qa_passed"));
        assert!(last_two.contains(&"fallback_unresolved_with_tickets"));
    }

    // ===== Serialization round-trips =====

    #[test]
    fn test_orchestrator_config_serde_round_trip() {
        let cfg = OrchestratorConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: OrchestratorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.defaults.project, cfg.defaults.project);
        assert!(cfg2.projects.is_empty());
    }

    #[test]
    fn test_runner_config_serde_round_trip() {
        let cfg = RunnerConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: RunnerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.shell, cfg.shell);
        assert_eq!(cfg2.policy, cfg.policy);
    }

    #[test]
    fn test_loop_mode_serde_round_trip() {
        for mode_str in &["\"once\"", "\"fixed\"", "\"infinite\""] {
            let mode: LoopMode = serde_json::from_str(mode_str).unwrap();
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(&json, mode_str);
        }
    }

    #[test]
    fn test_step_scope_serde_round_trip() {
        for scope_str in &["\"task\"", "\"item\""] {
            let scope: StepScope = serde_json::from_str(scope_str).unwrap();
            let json = serde_json::to_string(&scope).unwrap();
            assert_eq!(&json, scope_str);
        }
    }

    #[test]
    fn test_cost_preference_serde_round_trip() {
        for pref_str in &["\"performance\"", "\"quality\"", "\"balance\""] {
            let pref: CostPreference = serde_json::from_str(pref_str).unwrap();
            let json = serde_json::to_string(&pref).unwrap();
            assert_eq!(&json, pref_str);
        }
    }

    #[test]
    fn test_checkpoint_strategy_serde_round_trip() {
        for s in &["\"none\"", "\"git_tag\"", "\"git_stash\""] {
            let strat: CheckpointStrategy = serde_json::from_str(s).unwrap();
            let json = serde_json::to_string(&strat).unwrap();
            assert_eq!(&json, s);
        }
    }

    #[test]
    fn test_build_error_level_serde() {
        let err: BuildErrorLevel = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(err, BuildErrorLevel::Error);
        let warn: BuildErrorLevel = serde_json::from_str("\"warning\"").unwrap();
        assert_eq!(warn, BuildErrorLevel::Warning);
    }

    #[test]
    fn test_safety_config_serde_round_trip() {
        let cfg = SafetyConfig {
            max_consecutive_failures: 5,
            auto_rollback: true,
            checkpoint_strategy: CheckpointStrategy::GitTag,
            step_timeout_secs: Some(600),
            binary_snapshot: true,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: SafetyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.max_consecutive_failures, 5);
        assert!(cfg2.auto_rollback);
        assert!(matches!(
            cfg2.checkpoint_strategy,
            CheckpointStrategy::GitTag
        ));
        assert_eq!(cfg2.step_timeout_secs, Some(600));
        assert!(cfg2.binary_snapshot);
    }

    #[test]
    fn test_workflow_finalize_config_serde_round_trip() {
        let cfg = default_workflow_finalize_config();
        let json = serde_json::to_string(&cfg).unwrap();
        let cfg2: WorkflowFinalizeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg2.rules.len(), cfg.rules.len());
        assert_eq!(cfg2.rules[0].id, cfg.rules[0].id);
    }

    // ===== Serde with defaults (missing optional fields) =====

    #[test]
    fn test_runner_config_deserialize_minimal() {
        let json = r#"{"shell": "/bin/sh"}"#;
        let cfg: RunnerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.shell, "/bin/sh");
        // defaults should kick in
        assert_eq!(cfg.shell_arg, "-lc");
        assert_eq!(cfg.policy, RunnerPolicy::Legacy);
        assert!(!cfg.allowed_shells.is_empty());
    }

    #[test]
    fn test_safety_config_deserialize_minimal() {
        let json = r#"{}"#;
        let cfg: SafetyConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.max_consecutive_failures, 3);
        assert!(!cfg.auto_rollback);
    }

    #[test]
    fn test_pipeline_variables_deserialize_minimal() {
        let json = r#"{}"#;
        let pv: PipelineVariables = serde_json::from_str(json).unwrap();
        assert!(pv.vars.is_empty());
        assert!(pv.build_errors.is_empty());
    }

    // ===== PIPELINE_VAR_INLINE_LIMIT constant =====

    #[test]
    fn test_pipeline_var_inline_limit() {
        assert_eq!(PIPELINE_VAR_INLINE_LIMIT, 4096);
    }

    // ===== WorkflowStepConfig via step_config helper =====

    #[test]
    fn test_step_config_helper() {
        let s = step_config("my_id", Some("build"), None, true, false, false);
        assert_eq!(s.id, "my_id");
        assert_eq!(s.required_capability, Some("build".to_string()));
        assert!(s.builtin.is_none());
        assert!(s.enabled);
        assert!(!s.repeatable);
        assert!(!s.is_guard);
        assert!(s.cost_preference.is_none());
        assert!(s.prehook.is_none());
        assert!(!s.tty);
        assert!(s.outputs.is_empty());
        assert!(s.pipe_to.is_none());
        assert!(s.command.is_none());
        assert!(s.chain_steps.is_empty());
        assert!(s.scope.is_none());
    }

    // ===== WorkflowLoopConfig default =====

    #[test]
    fn test_workflow_loop_config_default() {
        let cfg = WorkflowLoopConfig::default();
        assert!(matches!(cfg.mode, LoopMode::Once));
        assert!(cfg.guard.enabled);
    }

    // ===== WorkflowFinalizeConfig default =====

    #[test]
    fn test_workflow_finalize_config_default_empty() {
        let cfg = WorkflowFinalizeConfig::default();
        assert!(cfg.rules.is_empty());
    }

    // ===== AgentSelectionConfig default =====

    #[test]
    fn test_agent_selection_config_default() {
        let cfg = AgentSelectionConfig::default();
        assert!(cfg.weights.is_none());
        // The strategy defaults to CapabilityAware via the serde default fn
        // but Default derive gives CostBased; verify the struct-level default
    }

    // ===== StepPrehookUiConfig default =====

    #[test]
    fn test_step_prehook_ui_config_default() {
        let cfg = StepPrehookUiConfig::default();
        assert!(cfg.mode.is_none());
        assert!(cfg.preset_id.is_none());
        assert!(cfg.expr.is_none());
    }

    // ===== default_project function =====

    #[test]
    fn test_default_project() {
        assert_eq!(default_project(), "default");
    }
}
