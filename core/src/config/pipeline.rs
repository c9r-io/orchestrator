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
        let pv: PipelineVariables = serde_json::from_str(json).unwrap();
        assert!(pv.vars.is_empty());
        assert!(pv.build_errors.is_empty());
    }

    #[test]
    fn test_build_error_level_serde() {
        let err: BuildErrorLevel = serde_json::from_str("\"error\"").unwrap();
        assert_eq!(err, BuildErrorLevel::Error);
        let warn: BuildErrorLevel = serde_json::from_str("\"warning\"").unwrap();
        assert_eq!(warn, BuildErrorLevel::Warning);
    }

    #[test]
    fn test_pipeline_var_inline_limit() {
        assert_eq!(PIPELINE_VAR_INLINE_LIMIT, 4096);
    }
}
