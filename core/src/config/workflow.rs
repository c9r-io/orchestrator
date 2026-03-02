use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::{
    CostPreference, SafetyConfig, StepBehavior, StepPrehookConfig, StepScope,
    WorkflowFinalizeConfig,
};

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
