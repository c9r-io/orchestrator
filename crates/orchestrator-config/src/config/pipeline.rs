use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum byte length for a pipeline variable value to remain inline.
/// Values exceeding this are spilled to a file and the inline value is truncated.
/// 4 KB leaves headroom for bash escaping inflation (~1.5-2x) plus template
/// boilerplate within the 16 KB runner safety limit.
pub const PIPELINE_VAR_INLINE_LIMIT: usize = 4096;

/// Narrow durable channels that remain after legacy coordination retirement.
///
/// These fields are not a general workflow state store: they carry one
/// user-intent value and three scheduler-owned sandbox safety signals.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreservedExecutionChannels {
    /// User intent supplied when the task is created.
    #[serde(default)]
    pub goal: String,
    /// Whether the latest runner attempt was denied by sandbox policy.
    #[serde(default)]
    pub last_sandbox_denied: bool,
    /// Number of sandbox denials observed for the task/item state.
    #[serde(default)]
    pub sandbox_denied_count: u32,
    /// Bounded explanation for the latest sandbox denial.
    #[serde(default)]
    pub last_sandbox_denial_reason: Option<String>,
}

/// Typed scheduler-owned signals consumed by deterministic governance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSignals {
    /// Exit code from the latest self-test builtin.
    #[serde(default)]
    pub self_test_exit_code: Option<i64>,
    /// Whether the latest self-test builtin passed.
    #[serde(default)]
    pub self_test_passed: bool,
    /// Bare tool names observed in the latest structured run.
    #[serde(default)]
    pub tools_called: Vec<String>,
    /// Number of failed tool results in the latest structured run.
    #[serde(default)]
    pub tool_error_count: i64,
    /// Bounded numeric observations accepted by the `record_metric` tool.
    #[serde(default)]
    pub metrics: HashMap<String, f64>,
}

/// Pipeline variables passed between steps
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineVariables {
    /// Explicit residual intent/safety channels, separate from legacy vars.
    #[serde(default)]
    pub preserved: PreservedExecutionChannels,
    /// Explicit runtime/governance signals, not author-defined state.
    #[serde(default)]
    pub signals: ExecutionSignals,
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

impl PipelineVariables {
    /// Migrate the four previously allowlisted keys out of the generic map.
    ///
    /// Existing `pipeline_vars_json` rows remain readable while every new
    /// serialization stores these values only in the narrow carrier.
    pub fn normalize_preserved_channels(&mut self) {
        if let Some(goal) = self.vars.remove("goal")
            && self.preserved.goal.is_empty()
        {
            self.preserved.goal = goal;
        }
        if let Some(value) = self.vars.remove("last_sandbox_denied") {
            self.preserved.last_sandbox_denied = value == "true";
        }
        if let Some(value) = self.vars.remove("sandbox_denied_count")
            && let Ok(count) = value.parse()
        {
            self.preserved.sandbox_denied_count = count;
        }
        if let Some(value) = self.vars.remove("last_sandbox_denial_reason")
            && !value.is_empty()
        {
            self.preserved.last_sandbox_denial_reason = Some(value);
        }
    }
}

/// Build error with source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildError {
    /// Source file that emitted the error, when available.
    pub file: Option<String>,
    /// 1-based source line for the error, when available.
    pub line: Option<u32>,
    /// 1-based source column for the error, when available.
    pub column: Option<u32>,
    /// Human-readable compiler or build-system message.
    pub message: String,
    /// Severity assigned to the build diagnostic.
    pub level: BuildErrorLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Severity levels recorded for build diagnostics.
pub enum BuildErrorLevel {
    /// A failing diagnostic that should block the pipeline.
    Error,
    /// A non-fatal diagnostic surfaced to the workflow.
    Warning,
}

/// Test failure with source location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    /// Test case or suite name that failed.
    pub test_name: String,
    /// Source file associated with the failure, when available.
    pub file: Option<String>,
    /// 1-based source line associated with the failure, when available.
    pub line: Option<u32>,
    /// Human-readable failure message.
    pub message: String,
    /// Captured stdout emitted by the failing test, when available.
    pub stdout: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_variables_default() {
        let pv = PipelineVariables::default();
        assert_eq!(pv.preserved, PreservedExecutionChannels::default());
        assert!(pv.signals.tools_called.is_empty());
        assert!(pv.vars.is_empty());
        assert!(pv.build_errors.is_empty());
        assert!(pv.test_failures.is_empty());
        assert_eq!(pv.prev_stdout, "");
        assert_eq!(pv.prev_stderr, "");
        assert_eq!(pv.diff, "");
    }

    #[test]
    fn test_pipeline_variables_deserialize_minimal() {
        let json = r#"{}"#;
        let pv: PipelineVariables =
            serde_json::from_str(json).expect("deserialize minimal pipeline variables");
        assert!(pv.vars.is_empty());
        assert!(pv.build_errors.is_empty());
    }

    #[test]
    fn test_build_error_level_serde() {
        let err: BuildErrorLevel =
            serde_json::from_str("\"error\"").expect("deserialize error level");
        assert_eq!(err, BuildErrorLevel::Error);
        let warn: BuildErrorLevel =
            serde_json::from_str("\"warning\"").expect("deserialize warning level");
        assert_eq!(warn, BuildErrorLevel::Warning);
    }

    #[test]
    fn test_pipeline_var_inline_limit() {
        assert_eq!(PIPELINE_VAR_INLINE_LIMIT, 4096);
    }

    #[test]
    fn legacy_preserved_keys_migrate_out_of_generic_vars() {
        let mut pv: PipelineVariables = serde_json::from_str(
            r#"{"vars":{"goal":"ship","last_sandbox_denied":"true","sandbox_denied_count":"2","last_sandbox_denial_reason":"network","other":"kept"}}"#,
        )
        .expect("legacy pipeline state");
        pv.normalize_preserved_channels();
        assert_eq!(pv.preserved.goal, "ship");
        assert!(pv.preserved.last_sandbox_denied);
        assert_eq!(pv.preserved.sandbox_denied_count, 2);
        assert_eq!(
            pv.preserved.last_sandbox_denial_reason.as_deref(),
            Some("network")
        );
        assert_eq!(pv.vars, HashMap::from([("other".into(), "kept".into())]));

        let serialized = serde_json::to_value(&pv).expect("serialize migrated pipeline state");
        let vars = serialized["vars"].as_object().expect("generic vars object");
        assert!(!vars.contains_key("goal"));
        assert!(!vars.contains_key("last_sandbox_denied"));
        assert!(!vars.contains_key("sandbox_denied_count"));
        assert!(!vars.contains_key("last_sandbox_denial_reason"));
    }

    #[test]
    fn explicit_preserved_goal_wins_and_legacy_duplicate_is_removed() {
        let mut pv: PipelineVariables =
            serde_json::from_str(r#"{"preserved":{"goal":"typed"},"vars":{"goal":"legacy"}}"#)
                .expect("mixed-version pipeline state");

        pv.normalize_preserved_channels();

        assert_eq!(pv.preserved.goal, "typed");
        assert!(!pv.vars.contains_key("goal"));
    }
}
