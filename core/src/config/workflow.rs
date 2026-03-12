use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::{
    CostPreference, ItemSelectConfig, SafetyConfig, StepBehavior, StepPrehookConfig, StepScope,
    StoreInputConfig, StoreOutputConfig, WorkflowFinalizeConfig,
};

/// Workflow step configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepConfig {
    /// Stable step identifier used in workflow definitions and traces.
    pub id: String,
    /// Human-readable description shown in generated docs or diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Required agent capability for agent-backed steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    /// Reference to a StepTemplate resource name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Execution profile name used to select host or sandbox behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile: Option<String>,
    /// Builtin implementation name for builtin-backed steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    /// Whether the step is enabled.
    pub enabled: bool,
    /// Whether the step should run again on subsequent loop cycles.
    #[serde(default = "default_true")]
    pub repeatable: bool,
    /// Whether this step can terminate the workflow loop.
    #[serde(default)]
    pub is_guard: bool,
    /// Optional cost preference hint used during agent selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<CostPreference>,
    /// Conditional execution hook evaluated before the step runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<StepPrehookConfig>,
    /// Whether command execution should request a TTY.
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
    /// Maximum parallel items for item-scoped steps (per-step override)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,
    /// Per-step timeout in seconds (overrides global safety.step_timeout_secs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// WP03: Configuration for item_select builtin step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_select_config: Option<ItemSelectConfig>,
    /// Store inputs: read values from workflow stores before step execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_inputs: Vec<StoreInputConfig>,
    /// Store outputs: write pipeline vars to workflow stores after step execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_outputs: Vec<StoreOutputConfig>,
}

fn default_true() -> bool {
    true
}

/// Execution mode used to schedule a workflow.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionMode {
    /// Execute static task and item segments defined directly in YAML.
    #[default]
    StaticSegment,
    /// Materialize a dynamic DAG at runtime before execution.
    DynamicDag,
}

/// Failure handling strategy when dynamic DAG planning is unavailable.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DagFallbackMode {
    /// Use the deterministic DAG builder.
    #[default]
    DeterministicDag,
    /// Fall back to the static segment executor.
    StaticSegment,
    /// Treat planning failures as terminal errors.
    FailClosed,
}

/// Workflow-level execution settings for dynamic planning persistence and fallback.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowExecutionConfig {
    /// Runtime execution mode for the workflow.
    #[serde(default)]
    pub mode: WorkflowExecutionMode,
    /// Fallback strategy used when dynamic planning fails.
    #[serde(default)]
    pub fallback_mode: DagFallbackMode,
    /// Whether graph runs and snapshots should be persisted.
    #[serde(default = "default_true")]
    pub persist_graph_snapshots: bool,
}

/// Complete workflow definition used by the scheduler.
///
/// # Examples
///
/// ```rust
/// use agent_orchestrator::config::{LoopMode, WorkflowConfig};
///
/// let workflow = WorkflowConfig::default();
/// assert!(workflow.steps.is_empty());
/// assert!(matches!(workflow.loop_policy.mode, LoopMode::Once));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowConfig {
    /// Ordered step list for static execution segments.
    #[serde(default)]
    pub steps: Vec<WorkflowStepConfig>,
    /// Workflow-level execution mode and persistence settings.
    #[serde(default)]
    pub execution: WorkflowExecutionConfig,
    /// Loop policy controlling cycle count and guard behavior.
    #[serde(rename = "loop", default)]
    pub loop_policy: WorkflowLoopConfig,
    /// Finalization behavior applied after loop completion.
    #[serde(default)]
    pub finalize: WorkflowFinalizeConfig,
    /// Legacy QA template identifier preserved for compatibility.
    #[serde(default)]
    pub qa: Option<String>,
    /// Legacy fix template identifier preserved for compatibility.
    #[serde(default)]
    pub fix: Option<String>,
    /// Legacy retest template identifier preserved for compatibility.
    #[serde(default)]
    pub retest: Option<String>,
    /// Dynamically eligible steps that can be added at runtime.
    #[serde(default)]
    pub dynamic_steps: Vec<crate::dynamic_orchestration::DynamicStepConfig>,
    /// Adaptive planning configuration for agent-driven DAG generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<crate::dynamic_orchestration::AdaptivePlannerConfig>,
    /// Safety configuration for self-bootstrap scenarios
    #[serde(default)]
    pub safety: SafetyConfig,
    /// Default max parallelism for item-scoped segments (1 = sequential)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,
}

/// Loop mode used to control workflow repetition.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    /// Run the workflow exactly once.
    #[default]
    Once,
    /// Run the workflow for a fixed number of cycles.
    Fixed,
    /// Continue looping until a guard or external action stops execution.
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

/// Guard settings evaluated between workflow cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLoopGuardConfig {
    /// Whether loop-guard evaluation is enabled.
    pub enabled: bool,
    /// Stop execution once no unresolved items remain.
    pub stop_when_no_unresolved: bool,
    /// Optional hard cap on the number of cycles.
    pub max_cycles: Option<u32>,
    /// Optional agent template used for guard evaluation.
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

/// Loop policy combining mode and guard settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowLoopConfig {
    /// Loop repetition mode.
    pub mode: LoopMode,
    /// Guard settings evaluated after each cycle.
    #[serde(default)]
    pub guard: WorkflowLoopGuardConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_loop_guard_default() {
        let cfg = WorkflowLoopGuardConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.stop_when_no_unresolved);
        assert!(cfg.max_cycles.is_none());
        assert!(cfg.agent_template.is_none());
    }

    #[test]
    fn test_loop_mode_default() {
        let mode = LoopMode::default();
        assert!(matches!(mode, LoopMode::Once));
    }

    #[test]
    fn test_loop_mode_from_str_valid() {
        assert!(matches!(
            LoopMode::from_str("once").expect("parse once"),
            LoopMode::Once
        ));
        assert!(matches!(
            LoopMode::from_str("fixed").expect("parse fixed"),
            LoopMode::Fixed
        ));
        assert!(matches!(
            LoopMode::from_str("infinite").expect("parse infinite"),
            LoopMode::Infinite
        ));
    }

    #[test]
    fn test_loop_mode_from_str_invalid() {
        let err = LoopMode::from_str("bogus").expect_err("operation should fail");
        assert!(err.contains("unknown loop mode"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn test_loop_mode_serde_round_trip() {
        for mode_str in &["\"once\"", "\"fixed\"", "\"infinite\""] {
            let mode: LoopMode = serde_json::from_str(mode_str).expect("deserialize loop mode");
            let json = serde_json::to_string(&mode).expect("serialize loop mode");
            assert_eq!(&json, mode_str);
        }
    }

    #[test]
    fn test_workflow_loop_config_default() {
        let cfg = WorkflowLoopConfig::default();
        assert!(matches!(cfg.mode, LoopMode::Once));
        assert!(cfg.guard.enabled);
    }
}
