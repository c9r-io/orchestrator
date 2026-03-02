use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::artifact::Artifact;

/// Agent output with structured data (replaces exit_code-only results)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    pub run_id: Uuid,
    pub agent_id: String,
    pub phase: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,
    pub metrics: ExecutionMetrics,
    pub confidence: f32,
    pub quality_score: f32,
    pub created_at: DateTime<Utc>,
    /// Structured build errors (populated for build/lint phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_errors: Vec<crate::config::BuildError>,
    /// Structured test failures (populated for test phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_failures: Vec<crate::config::TestFailure>,
}

impl AgentOutput {
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

    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = artifacts;
        self
    }

    pub fn with_metrics(mut self, metrics: ExecutionMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_quality_score(mut self, score: f32) -> Self {
        self.quality_score = score.clamp(0.0, 1.0);
        self
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execution metrics from agent run
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub duration_ms: u64,
    pub tokens_consumed: Option<u64>,
    pub api_calls: Option<u32>,
    pub retry_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::{ArtifactKind, ExecutionMetrics};

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
