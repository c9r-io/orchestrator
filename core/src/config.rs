//! Configuration structures for the orchestrator.
//! Contains all config types: ProjectConfig, OrchestratorConfig, AgentConfig, WorkflowConfig, etc.

use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
            runner: RunnerConfig {
                shell: "/bin/bash".to_string(),
                shell_arg: "-lc".to_string(),
            },
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    pub shell: String,
    pub shell_arg: String,
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
    Qa,
    TicketScan,
    Fix,
    Retest,
    LoopGuard,
}

impl WorkflowStepType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InitOnce => "init_once",
            Self::Qa => "qa",
            Self::TicketScan => "ticket_scan",
            Self::Fix => "fix",
            Self::Retest => "retest",
            Self::LoopGuard => "loop_guard",
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

/// Default workflow steps builder
pub fn default_workflow_steps(
    qa: Option<&str>,
    ticket_scan: bool,
    fix: Option<&str>,
    retest: Option<&str>,
) -> Vec<WorkflowStepConfig> {
    vec![
        WorkflowStepConfig {
            id: "init_once".to_string(),
            description: None,
            step_type: Some(WorkflowStepType::InitOnce),
            required_capability: None,
            builtin: Some("init_once".to_string()),
            enabled: false,
            repeatable: false,
            is_guard: false,
            cost_preference: None,
            prehook: None,
        },
        WorkflowStepConfig {
            id: "qa".to_string(),
            description: None,
            step_type: Some(WorkflowStepType::Qa),
            required_capability: Some("qa".to_string()),
            builtin: None,
            enabled: qa.is_some(),
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
        },
        WorkflowStepConfig {
            id: "ticket_scan".to_string(),
            description: None,
            step_type: Some(WorkflowStepType::TicketScan),
            required_capability: None,
            builtin: Some("ticket_scan".to_string()),
            enabled: ticket_scan,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
        },
        WorkflowStepConfig {
            id: "fix".to_string(),
            description: None,
            step_type: Some(WorkflowStepType::Fix),
            required_capability: Some("fix".to_string()),
            builtin: None,
            enabled: fix.is_some(),
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
        },
        WorkflowStepConfig {
            id: "retest".to_string(),
            description: None,
            step_type: Some(WorkflowStepType::Retest),
            required_capability: Some("retest".to_string()),
            builtin: None,
            enabled: retest.is_some(),
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
        },
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
