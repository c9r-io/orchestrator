use serde::{Deserialize, Serialize};

/// Configuration for a single invariant constraint (WP04).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvariantConfig {
    /// Stable identifier for the invariant.
    pub name: String,
    /// Human-readable description shown in diagnostics.
    #[serde(default)]
    pub description: String,
    /// Optional shell command executed to evaluate the invariant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Expected exit code for `command`, when command-based evaluation is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exit: Option<i32>,
    /// Optional pipeline variable name used to capture command output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_as: Option<String>,
    /// CEL-style assertion evaluated against the captured result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assert_expr: Option<String>,
    /// Marks the invariant as immutable once the task starts.
    #[serde(default)]
    pub immutable: bool,
    /// Task checkpoints where the invariant should run.
    #[serde(default = "default_check_at")]
    pub check_at: Vec<InvariantCheckPoint>,
    /// Policy to apply when the invariant fails.
    #[serde(default)]
    pub on_violation: OnViolation,
    /// Files that must remain unchanged while the invariant is active.
    #[serde(default)]
    pub protected_files: Vec<String>,
}

fn default_check_at() -> Vec<InvariantCheckPoint> {
    vec![
        InvariantCheckPoint::AfterImplement,
        InvariantCheckPoint::BeforeComplete,
    ]
}

/// When an invariant should be checked during cycle execution.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InvariantCheckPoint {
    /// Run before a workflow cycle begins.
    BeforeCycle,
    /// Run after the implement/fix phase completes.
    AfterImplement,
    /// Run before a task is restarted.
    BeforeRestart,
    /// Run before the task is marked complete.
    BeforeComplete,
}

/// What to do when an invariant is violated.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnViolation {
    /// Stop execution immediately.
    #[default]
    Halt,
    /// Roll back the task item before continuing.
    Rollback,
    /// Record a warning but keep the workflow running.
    Warn,
}

/// Result of evaluating a single invariant.
#[derive(Debug, Clone)]
pub struct InvariantResult {
    /// Name of the invariant that ran.
    pub name: String,
    /// Whether the invariant passed.
    pub passed: bool,
    /// Human-readable evaluation summary.
    pub message: String,
    /// Violation policy attached to the invariant.
    pub on_violation: OnViolation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invariant_config_defaults() {
        let json = r#"{"name": "test", "description": "a test"}"#;
        let cfg: InvariantConfig =
            serde_json::from_str(json).expect("deserialize invariant config");
        assert_eq!(cfg.name, "test");
        assert!(!cfg.immutable);
        assert_eq!(cfg.check_at.len(), 2);
        assert!(cfg.check_at.contains(&InvariantCheckPoint::AfterImplement));
        assert!(cfg.check_at.contains(&InvariantCheckPoint::BeforeComplete));
        assert_eq!(cfg.on_violation, OnViolation::Halt);
        assert!(cfg.protected_files.is_empty());
    }

    #[test]
    fn test_invariant_config_full() {
        let json = r#"{
            "name": "no_unsafe",
            "description": "No unsafe code",
            "command": "grep -r unsafe src/",
            "expect_exit": 1,
            "assert_expr": "exit_code == 1",
            "immutable": true,
            "check_at": ["before_cycle", "after_implement"],
            "on_violation": "rollback",
            "protected_files": ["Cargo.toml", "src/main.rs"]
        }"#;
        let cfg: InvariantConfig =
            serde_json::from_str(json).expect("deserialize full invariant config");
        assert!(cfg.immutable);
        assert_eq!(cfg.on_violation, OnViolation::Rollback);
        assert_eq!(cfg.protected_files.len(), 2);
        assert_eq!(cfg.check_at.len(), 2);
    }

    #[test]
    fn test_on_violation_default() {
        let v = OnViolation::default();
        assert_eq!(v, OnViolation::Halt);
    }

    #[test]
    fn test_checkpoint_serde_round_trip() {
        for s in &[
            "\"before_cycle\"",
            "\"after_implement\"",
            "\"before_restart\"",
            "\"before_complete\"",
        ] {
            let cp: InvariantCheckPoint = serde_json::from_str(s).expect("deserialize checkpoint");
            let json = serde_json::to_string(&cp).expect("serialize checkpoint");
            assert_eq!(&json, s);
        }
    }

    #[test]
    fn test_on_violation_serde_round_trip() {
        for s in &["\"halt\"", "\"rollback\"", "\"warn\""] {
            let ov: OnViolation = serde_json::from_str(s).expect("deserialize on_violation");
            let json = serde_json::to_string(&ov).expect("serialize on_violation");
            assert_eq!(&json, s);
        }
    }
}
