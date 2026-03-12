use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    AgentConfig, EnvStoreConfig, ExecutionProfileConfig, InvariantConfig, StepTemplateConfig,
    WorkflowConfig,
};

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
    /// Safety policy profile for self-referential workflows
    #[serde(default)]
    pub profile: WorkflowSafetyProfile,
    /// WP04: Invariant constraints enforced by the engine
    #[serde(default)]
    pub invariants: Vec<InvariantConfig>,
    /// WP02: Maximum total spawned tasks per parent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spawned_tasks: Option<usize>,
    /// WP02: Maximum spawn depth (parent → child → grandchild)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spawn_depth: Option<usize>,
    /// WP02: Minimum seconds between spawn bursts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_cooldown_seconds: Option<u64>,
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
            profile: WorkflowSafetyProfile::default(),
            invariants: Vec::new(),
            max_spawned_tasks: None,
            max_spawn_depth: None,
            spawn_cooldown_seconds: None,
        }
    }
}

/// Checkpoint strategy for rollback support
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStrategy {
    #[default]
    /// Disable checkpoint creation.
    None,
    /// Create Git tags for rollback checkpoints.
    GitTag,
    /// Create Git stash entries for rollback checkpoints.
    GitStash,
}

/// Safety profile for self-referential workflows.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSafetyProfile {
    #[default]
    /// Standard safety behavior for ordinary workflows.
    Standard,
    /// Extra guardrails for self-referential workflows targeting the orchestrator source tree.
    SelfReferentialProbe,
}

/// Workspace configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// Path to the workspace root, relative to the application root unless absolute.
    pub root_path: String,
    /// QA target paths evaluated for this workspace.
    pub qa_targets: Vec<String>,
    /// Directory used to store QA tickets for the workspace.
    pub ticket_dir: String,
    /// When true, the workspace points to the orchestrator's own source tree
    #[serde(default)]
    pub self_referential: bool,
}

/// Project-level configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional human-readable description of the project.
    pub description: Option<String>,
    #[serde(default)]
    /// Workspaces available within the project.
    pub workspaces: HashMap<String, WorkspaceConfig>,
    #[serde(default)]
    /// Agents available within the project.
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    /// Workflows available within the project.
    pub workflows: HashMap<String, WorkflowConfig>,
    #[serde(default)]
    /// Named step templates reusable by workflows in the project.
    pub step_templates: HashMap<String, StepTemplateConfig>,
    #[serde(default)]
    /// Environment stores scoped to the project.
    pub env_stores: HashMap<String, EnvStoreConfig>,
    #[serde(default)]
    /// Execution profiles available within the project.
    pub execution_profiles: HashMap<String, ExecutionProfileConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_safety_config_serde_round_trip() {
        let cfg = SafetyConfig {
            max_consecutive_failures: 5,
            auto_rollback: true,
            checkpoint_strategy: CheckpointStrategy::GitTag,
            step_timeout_secs: Some(600),
            binary_snapshot: true,
            profile: WorkflowSafetyProfile::SelfReferentialProbe,
            ..SafetyConfig::default()
        };
        let json = serde_json::to_string(&cfg).expect("serialize safety config");
        let cfg2: SafetyConfig = serde_json::from_str(&json).expect("deserialize safety config");
        assert_eq!(cfg2.max_consecutive_failures, 5);
        assert!(cfg2.auto_rollback);
        assert!(matches!(
            cfg2.checkpoint_strategy,
            CheckpointStrategy::GitTag
        ));
        assert_eq!(cfg2.step_timeout_secs, Some(600));
        assert!(cfg2.binary_snapshot);
        assert_eq!(cfg2.profile, WorkflowSafetyProfile::SelfReferentialProbe);
    }

    #[test]
    fn test_safety_config_deserialize_minimal() {
        let json = r#"{}"#;
        let cfg: SafetyConfig = serde_json::from_str(json).expect("deserialize minimal safety");
        assert_eq!(cfg.max_consecutive_failures, 3);
        assert!(!cfg.auto_rollback);
        assert_eq!(cfg.profile, WorkflowSafetyProfile::Standard);
    }

    #[test]
    fn test_checkpoint_strategy_default() {
        let strat = CheckpointStrategy::default();
        assert!(matches!(strat, CheckpointStrategy::None));
    }

    #[test]
    fn test_checkpoint_strategy_serde_round_trip() {
        for s in &["\"none\"", "\"git_tag\"", "\"git_stash\""] {
            let strat: CheckpointStrategy =
                serde_json::from_str(s).expect("deserialize checkpoint strategy");
            let json = serde_json::to_string(&strat).expect("serialize checkpoint strategy");
            assert_eq!(&json, s);
        }
    }
}
