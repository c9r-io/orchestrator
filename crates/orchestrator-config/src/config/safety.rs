use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    AgentConfig, EnvStoreConfig, ExecutionProfileConfig, HealthPolicyConfig, InvariantConfig,
    SecretStoreConfig, StepTemplateConfig, TriggerConfig, WorkflowConfig,
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
    /// FR-035: Per-item per-step consecutive failure threshold before blocking
    #[serde(default = "default_max_item_step_failures")]
    pub max_item_step_failures: u32,
    /// FR-035: Minimum cycle interval in seconds; rapid cycles below this trigger pause
    #[serde(default = "default_min_cycle_interval_secs")]
    pub min_cycle_interval_secs: u64,
    /// Stall auto-kill threshold in seconds. When a step produces less than
    /// `LOW_OUTPUT_DELTA_THRESHOLD_BYTES` per heartbeat for this duration, the
    /// step is killed with exit_code=-7. Default (None) uses the built-in 900s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_timeout_secs: Option<u64>,
    /// FR-052: Maximum seconds to wait for in-flight runs when no heartbeat activity
    #[serde(default = "default_inflight_wait_timeout_secs")]
    pub inflight_wait_timeout_secs: u64,
    /// FR-052: Heartbeat must be within this many seconds to be considered active
    #[serde(default = "default_inflight_heartbeat_grace_secs")]
    pub inflight_heartbeat_grace_secs: u64,
}

fn default_max_consecutive_failures() -> u32 {
    3
}

fn default_max_item_step_failures() -> u32 {
    3
}

fn default_min_cycle_interval_secs() -> u64 {
    60
}

fn default_inflight_wait_timeout_secs() -> u64 {
    300
}

fn default_inflight_heartbeat_grace_secs() -> u64 {
    60
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
            stall_timeout_secs: None,
            max_item_step_failures: 3,
            min_cycle_interval_secs: 60,
            inflight_wait_timeout_secs: 300,
            inflight_heartbeat_grace_secs: 60,
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
    /// Default health policy for agents operating in this workspace.
    #[serde(default, skip_serializing_if = "HealthPolicyConfig::is_default")]
    pub health_policy: HealthPolicyConfig,
    /// Optional directory for pipeline variable spill files, relative to root_path.
    /// Defaults to `.orchestrator/artifacts` when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts_dir: Option<String>,
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
    /// Secret stores scoped to the project. All values are sensitive.
    pub secret_stores: HashMap<String, SecretStoreConfig>,
    #[serde(default)]
    /// Execution profiles available within the project.
    pub execution_profiles: HashMap<String, ExecutionProfileConfig>,
    #[serde(default)]
    /// Trigger definitions scoped to the project.
    pub triggers: HashMap<String, TriggerConfig>,
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
        assert!(cfg.stall_timeout_secs.is_none());
        assert!(!cfg.binary_snapshot);
        assert_eq!(cfg.max_item_step_failures, 3);
        assert_eq!(cfg.min_cycle_interval_secs, 60);
        assert_eq!(cfg.inflight_wait_timeout_secs, 300);
        assert_eq!(cfg.inflight_heartbeat_grace_secs, 60);
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
        assert_eq!(cfg.max_item_step_failures, 3);
        assert_eq!(cfg.min_cycle_interval_secs, 60);
        assert_eq!(cfg.inflight_wait_timeout_secs, 300);
        assert_eq!(cfg.inflight_heartbeat_grace_secs, 60);
    }

    #[test]
    fn test_fr052_fields_serde_round_trip() {
        let cfg = SafetyConfig {
            inflight_wait_timeout_secs: 600,
            inflight_heartbeat_grace_secs: 120,
            ..SafetyConfig::default()
        };
        let json = serde_json::to_string(&cfg).expect("serialize FR-052 safety config");
        let cfg2: SafetyConfig =
            serde_json::from_str(&json).expect("deserialize FR-052 safety config");
        assert_eq!(cfg2.inflight_wait_timeout_secs, 600);
        assert_eq!(cfg2.inflight_heartbeat_grace_secs, 120);
    }

    #[test]
    fn test_fr052_fields_explicit_json_deserialization() {
        let json = r#"{"inflight_wait_timeout_secs": 10, "inflight_heartbeat_grace_secs": 30}"#;
        let cfg: SafetyConfig =
            serde_json::from_str(json).expect("deserialize explicit FR-052 fields");
        assert_eq!(cfg.inflight_wait_timeout_secs, 10);
        assert_eq!(cfg.inflight_heartbeat_grace_secs, 30);
        assert_eq!(cfg.max_consecutive_failures, 3);
    }

    #[test]
    fn test_fr035_fields_serde_round_trip() {
        let cfg = SafetyConfig {
            max_item_step_failures: 7,
            min_cycle_interval_secs: 120,
            ..SafetyConfig::default()
        };
        let json = serde_json::to_string(&cfg).expect("serialize FR-035 safety config");
        let cfg2: SafetyConfig =
            serde_json::from_str(&json).expect("deserialize FR-035 safety config");
        assert_eq!(cfg2.max_item_step_failures, 7);
        assert_eq!(cfg2.min_cycle_interval_secs, 120);
    }

    #[test]
    fn test_fr035_fields_explicit_json_deserialization() {
        let json = r#"{"max_item_step_failures": 5, "min_cycle_interval_secs": 30}"#;
        let cfg: SafetyConfig =
            serde_json::from_str(json).expect("deserialize explicit FR-035 fields");
        assert_eq!(cfg.max_item_step_failures, 5);
        assert_eq!(cfg.min_cycle_interval_secs, 30);
        // Other fields should remain at their defaults.
        assert_eq!(cfg.max_consecutive_failures, 3);
        assert!(!cfg.auto_rollback);
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
