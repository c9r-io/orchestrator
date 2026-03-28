use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::artifact::Artifact;

/// Structured output emitted by an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// Run identifier associated with the output.
    pub run_id: Uuid,
    /// Agent identifier that produced the output.
    pub agent_id: String,
    /// Phase name for the run.
    pub phase: String,
    /// Process exit code.
    pub exit_code: i64,
    /// Captured stdout text.
    pub stdout: String,
    /// Captured stderr text.
    pub stderr: String,
    /// Structured artifacts parsed from the run.
    pub artifacts: Vec<Artifact>,
    /// Execution metrics collected for the run.
    pub metrics: ExecutionMetrics,
    /// Confidence score normalized to `[0.0, 1.0]`.
    pub confidence: f32,
    /// Quality score normalized to `[0.0, 1.0]`.
    pub quality_score: f32,
    /// Timestamp when the structured output was created.
    pub created_at: DateTime<Utc>,
    /// Structured build errors (populated for build/lint phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_errors: Vec<orchestrator_config::config::BuildError>,
    /// Structured test failures (populated for test phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_failures: Vec<orchestrator_config::config::TestFailure>,
}

impl AgentOutput {
    /// Creates a new output record with default metrics and scores.
    pub fn new(
        run_id: Uuid,
        agent_id: String,
        phase: String,
        exit_code: i64,
        stdout: String,
        stderr: String,
    ) -> Self {
        Self {
            run_id,
            agent_id,
            phase,
            exit_code,
            stdout,
            stderr,
            artifacts: Vec::new(),
            metrics: ExecutionMetrics::default(),
            confidence: 1.0,
            quality_score: 1.0,
            created_at: Utc::now(),
            build_errors: Vec::new(),
            test_failures: Vec::new(),
        }
    }

    /// Replaces the artifact list on the output.
    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = artifacts;
        self
    }

    /// Replaces execution metrics on the output.
    pub fn with_metrics(mut self, metrics: ExecutionMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Sets the confidence score, clamping to `[0.0, 1.0]`.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Sets the quality score, clamping to `[0.0, 1.0]`.
    pub fn with_quality_score(mut self, score: f32) -> Self {
        self.quality_score = score.clamp(0.0, 1.0);
        self
    }

    /// Returns `true` when the run exited successfully.
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execution metrics recorded for an agent run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Total wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Optional token count consumed by the agent backend.
    pub tokens_consumed: Option<u64>,
    /// Optional API call count issued by the agent backend.
    pub api_calls: Option<u32>,
    /// Number of retries performed before completion.
    pub retry_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactKind, ExecutionMetrics};

    #[test]
    fn test_agent_output_creation() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "qa_agent".to_string(),
            "qa".to_string(),
            0,
            "test output".to_string(),
            "".to_string(),
        );

        assert!(output.is_success());
        assert_eq!(output.confidence, 1.0);
    }

    #[test]
    fn test_agent_output_failure() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "impl_agent".to_string(),
            "implement".to_string(),
            1,
            "".to_string(),
            "error".to_string(),
        );
        assert!(!output.is_success());
    }

    #[test]
    fn test_agent_output_builder_methods() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "agent".to_string(),
            "qa".to_string(),
            0,
            "ok".to_string(),
            "".to_string(),
        )
        .with_confidence(0.85)
        .with_quality_score(0.9)
        .with_metrics(ExecutionMetrics {
            duration_ms: 1000,
            tokens_consumed: Some(500),
            api_calls: Some(3),
            retry_count: 1,
        })
        .with_artifacts(vec![Artifact::new(ArtifactKind::Custom {
            name: "test".to_string(),
        })]);

        assert_eq!(output.confidence, 0.85);
        assert_eq!(output.quality_score, 0.9);
        assert_eq!(output.metrics.duration_ms, 1000);
        assert_eq!(output.artifacts.len(), 1);
    }

    #[test]
    fn test_agent_output_confidence_clamped() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "a".to_string(),
            "p".to_string(),
            0,
            "".to_string(),
            "".to_string(),
        )
        .with_confidence(1.5)
        .with_quality_score(-0.5);

        assert_eq!(output.confidence, 1.0);
        assert_eq!(output.quality_score, 0.0);
    }
}
