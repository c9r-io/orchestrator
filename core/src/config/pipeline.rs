use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum byte length for a pipeline variable value to remain inline.
/// Values exceeding this are spilled to a file and the inline value is truncated.
/// 4 KB leaves headroom for bash escaping inflation (~1.5-2x) plus template
/// boilerplate within the 16 KB runner safety limit.
pub const PIPELINE_VAR_INLINE_LIMIT: usize = 4096;

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
}
