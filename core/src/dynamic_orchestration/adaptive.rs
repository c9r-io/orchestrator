use crate::config::StepPrehookContext;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::dag::{DynamicExecutionPlan, PrehookConfig, WorkflowEdge, WorkflowNode};

pub use orchestrator_config::adaptive::{AdaptiveFallbackMode, AdaptivePlannerConfig};

/// Historical execution record for planning context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionHistoryRecord {
    /// Task that produced this historical record.
    pub task_id: String,
    /// Task item associated with the recorded execution.
    pub item_id: String,
    /// Workflow cycle number for the record.
    pub cycle: u32,
    /// Step-level execution summaries captured for the cycle.
    pub steps: Vec<StepExecutionRecord>,
    /// Final task-item status after the cycle.
    pub final_status: String,
    /// Timestamp when the record was captured.
    pub timestamp: DateTime<Utc>,
}

/// Step-level execution data included in adaptive planning history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepExecutionRecord {
    /// Workflow step identifier.
    pub step_id: String,
    /// Semantic step type used by the planner.
    pub step_type: String,
    /// Exit code returned by the step command.
    pub exit_code: i64,
    /// Measured wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Confidence score reported by the agent, when available.
    pub confidence: Option<f32>,
    /// Quality score reported by the agent, when available.
    pub quality_score: Option<f32>,
    /// Number of tickets created by the step.
    pub tickets_created: i64,
    /// Number of tickets resolved by the step.
    pub tickets_resolved: i64,
}

/// Source used to materialize an adaptive execution plan.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdaptivePlanSource {
    /// The adaptive planner returned a valid plan.
    Planner,
    /// The planner failed and deterministic fallback logic was used.
    DeterministicFallback,
}

/// Failure classes used to explain adaptive planner degradation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveFailureClass {
    /// Adaptive planning is disabled for the workflow.
    Disabled,
    /// Required planner configuration is missing or invalid.
    Misconfigured,
    /// The injected executor failed before returning a plan.
    ExecutorFailure,
    /// The executor returned output that was not valid JSON.
    InvalidJson,
    /// The returned graph failed structural validation.
    InvalidPlan,
}

/// Metadata describing how an adaptive plan outcome was produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdaptivePlanMetadata {
    /// Source that produced the final plan.
    pub source: AdaptivePlanSource,
    /// Whether fallback logic was used.
    pub used_fallback: bool,
    /// Failure class recorded when fallback logic was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<AdaptiveFailureClass>,
    /// Human-readable error message captured during fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Final adaptive planning result returned to the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptivePlanOutcome {
    /// Executable graph selected for the workflow item.
    pub plan: DynamicExecutionPlan,
    /// Metadata describing planner source and degradation state.
    pub metadata: AdaptivePlanMetadata,
    /// Raw planner output before JSON deserialization, when retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<String>,
}

#[async_trait]
/// Injected executor used by `AdaptivePlanner` to obtain raw planner output.
pub trait AdaptivePlanExecutor: Send + Sync {
    /// Executes the planner prompt and returns raw JSON output.
    async fn execute(&self, prompt: &str, config: &AdaptivePlannerConfig) -> Result<String>;
}

/// Adaptive planner that delegates plan generation to an injected executor.
#[derive(Debug, Clone)]
pub struct AdaptivePlanner {
    config: AdaptivePlannerConfig,
    history: Vec<ExecutionHistoryRecord>,
}

impl AdaptivePlanner {
    /// Creates an adaptive planner with empty execution history.
    pub fn new(config: AdaptivePlannerConfig) -> Self {
        Self {
            config,
            history: Vec::new(),
        }
    }

    /// Adds one historical execution record, trimming to the configured history size.
    pub fn add_history(&mut self, record: ExecutionHistoryRecord) {
        if self.history.len() >= self.config.max_history {
            self.history.remove(0);
        }
        self.history.push(record);
    }

    /// Returns the in-memory execution history currently used for prompt generation.
    pub fn history(&self) -> &[ExecutionHistoryRecord] {
        &self.history
    }

    /// Generates a dynamic execution plan using the configured adaptive planner executor.
    pub async fn generate_plan<E>(
        &self,
        executor: &E,
        context: &StepPrehookContext,
    ) -> Result<AdaptivePlanOutcome>
    where
        E: AdaptivePlanExecutor,
    {
        if !self.config.enabled {
            return Err(anyhow!("adaptive planning is not enabled"));
        }

        if self
            .config
            .planner_agent
            .as_deref()
            .map(str::trim)
            .map(str::is_empty)
            .unwrap_or(true)
        {
            return self.handle_failure(
                AdaptiveFailureClass::Misconfigured,
                anyhow!("adaptive planner is enabled but planner_agent is not configured"),
                context,
                None,
            );
        }

        let prompt = self.build_prompt(context)?;
        let response = match executor.execute(&prompt, &self.config).await {
            Ok(response) => response,
            Err(err) => {
                return self.handle_failure(
                    AdaptiveFailureClass::ExecutorFailure,
                    err,
                    context,
                    None,
                );
            }
        };

        let plan = match serde_json::from_str::<DynamicExecutionPlan>(&response) {
            Ok(plan) => plan,
            Err(err) => {
                return self.handle_failure(
                    AdaptiveFailureClass::InvalidJson,
                    anyhow!("adaptive planner returned invalid JSON: {}", err),
                    context,
                    Some(response),
                );
            }
        };

        if let Err(err) = validate_generated_plan(&plan) {
            return self.handle_failure(
                AdaptiveFailureClass::InvalidPlan,
                err,
                context,
                Some(response),
            );
        }

        Ok(AdaptivePlanOutcome {
            plan,
            metadata: AdaptivePlanMetadata {
                source: AdaptivePlanSource::Planner,
                used_fallback: false,
                error_class: None,
                error_message: None,
            },
            raw_output: Some(response),
        })
    }

    fn handle_failure(
        &self,
        class: AdaptiveFailureClass,
        err: anyhow::Error,
        context: &StepPrehookContext,
        raw_output: Option<String>,
    ) -> Result<AdaptivePlanOutcome> {
        match self.config.fallback_mode {
            AdaptiveFallbackMode::SoftFallback => {
                tracing::warn!(
                    error_class = ?class,
                    error = %err,
                    task_id = %context.task_id,
                    item_id = %context.task_item_id,
                    "adaptive planner failed; using deterministic fallback"
                );
                Ok(AdaptivePlanOutcome {
                    plan: deterministic_fallback_plan(context),
                    metadata: AdaptivePlanMetadata {
                        source: AdaptivePlanSource::DeterministicFallback,
                        used_fallback: true,
                        error_class: Some(class),
                        error_message: Some(err.to_string()),
                    },
                    raw_output,
                })
            }
            AdaptiveFallbackMode::FailClosed => Err(err.context(format!(
                "adaptive planning failed ({})",
                adaptive_failure_class_name(class)
            ))),
        }
    }

    fn build_prompt(&self, context: &StepPrehookContext) -> Result<String> {
        let history_json =
            serde_json::to_string(&self.history).context("serialize adaptive planner history")?;

        Ok(format!(
            r#"You are an adaptive workflow planner for an agent orchestrator.
Return ONLY valid JSON that deserializes into:
{{
  "entry": "optional-node-id",
  "nodes": {{
    "node_id": {{
      "id": "node_id",
      "step_type": "qa|fix|retest|custom",
      "agent_id": "optional-agent-id",
      "template": "optional-command-template",
      "prehook": {{
        "engine": "cel",
        "when": "expression",
        "reason": "optional",
        "extended": false
      }},
      "is_guard": false,
      "repeatable": true
    }}
  }},
  "edges": [
    {{
      "from": "node_id",
      "to": "node_id",
      "condition": "optional expression"
    }}
  ]
}}

Rules:
- Output JSON only, no markdown or prose.
- All node ids must be unique.
- The graph must be acyclic.
- Keep plans minimal and executable.
- If fix is unnecessary, omit it instead of adding unreachable nodes.
- Use the configured agent_id only when you need to pin a specific agent.
- Temperature hint: {}

Context:
- Task: {}
- Item: {}
- Cycle: {}
- Active step: {}
- QA file path: {}
- Item status: {}
- Task status: {}
- QA exit code: {:?}
- Fix exit code: {:?}
- Retest exit code: {:?}
- Active tickets: {}
- New tickets: {}
- QA failed: {}
- Fix required: {}
- QA confidence: {:?}
- QA quality score: {:?}
- Build error count: {}
- Test failure count: {}
- Build exit code: {:?}
- Test exit code: {:?}
- Self test exit code: {:?}
- Self test passed: {}
- Max cycles: {}
- Is last cycle: {}
- Self referential safe: {}

Recent execution history:
{}
"#,
            self.config.temperature,
            context.task_id,
            context.task_item_id,
            context.cycle,
            context.step,
            context.qa_file_path,
            context.item_status,
            context.task_status,
            context.qa_exit_code,
            context.fix_exit_code,
            context.retest_exit_code,
            context.active_ticket_count,
            context.new_ticket_count,
            context.qa_failed,
            context.fix_required,
            context.qa_confidence,
            context.qa_quality_score,
            context.build_error_count,
            context.test_failure_count,
            context.build_exit_code,
            context.test_exit_code,
            context.self_test_exit_code,
            context.self_test_passed,
            context.max_cycles,
            context.is_last_cycle,
            context.self_referential_safe,
            history_json
        ))
    }
}

/// Returns the stable label for an adaptive failure class.
pub fn adaptive_failure_class_name(class: AdaptiveFailureClass) -> &'static str {
    match class {
        AdaptiveFailureClass::Disabled => "disabled",
        AdaptiveFailureClass::Misconfigured => "misconfigured",
        AdaptiveFailureClass::ExecutorFailure => "executor_failure",
        AdaptiveFailureClass::InvalidJson => "invalid_json",
        AdaptiveFailureClass::InvalidPlan => "invalid_plan",
    }
}

/// Returns the deterministic fallback plan used when adaptive planning degrades.
pub fn deterministic_fallback_plan(_context: &StepPrehookContext) -> DynamicExecutionPlan {
    let mut plan = DynamicExecutionPlan::new();

    let _ = plan.add_node(WorkflowNode {
        id: "qa".to_string(),
        step_type: "qa".to_string(),
        agent_id: None,
        template: None,
        prehook: None,
        is_guard: false,
        repeatable: false,
    });

    let _ = plan.add_node(WorkflowNode {
        id: "fix".to_string(),
        step_type: "fix".to_string(),
        agent_id: None,
        template: None,
        prehook: Some(PrehookConfig {
            engine: "cel".to_string(),
            when: "active_ticket_count > 0".to_string(),
            reason: Some("Only run fix when there are active tickets".to_string()),
            extended: false,
        }),
        is_guard: false,
        repeatable: true,
    });

    let _ = plan.add_edge(WorkflowEdge {
        from: "qa".to_string(),
        to: "fix".to_string(),
        condition: Some("qa_exit_code != 0 || active_ticket_count > 0".to_string()),
    });

    plan.entry = Some("qa".to_string());
    plan
}

/// Validates the structure of a generated adaptive execution plan.
pub fn validate_generated_plan(plan: &DynamicExecutionPlan) -> Result<()> {
    if plan.nodes.is_empty() {
        anyhow::bail!("adaptive plan must define at least one node");
    }

    if let Some(entry) = plan.entry.as_deref() {
        if !plan.nodes.contains_key(entry) {
            anyhow::bail!("adaptive plan entry node '{}' does not exist", entry);
        }
    }

    for (node_id, node) in &plan.nodes {
        if node.id.trim().is_empty() {
            anyhow::bail!("adaptive plan contains node with empty id");
        }
        if node.id != *node_id {
            anyhow::bail!(
                "adaptive plan node key '{}' does not match node.id '{}'",
                node_id,
                node.id
            );
        }
        if node.step_type.trim().is_empty() {
            anyhow::bail!("adaptive plan node '{}' has empty step_type", node.id);
        }
    }

    for edge in &plan.edges {
        if !plan.nodes.contains_key(&edge.from) {
            anyhow::bail!("adaptive plan edge source '{}' does not exist", edge.from);
        }
        if !plan.nodes.contains_key(&edge.to) {
            anyhow::bail!("adaptive plan edge target '{}' does not exist", edge.to);
        }
    }

    if plan.has_cycles() {
        anyhow::bail!("adaptive plan must be acyclic");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockExecutor {
        response: Option<String>,
        error: Option<String>,
    }

    #[async_trait]
    impl AdaptivePlanExecutor for MockExecutor {
        async fn execute(&self, _prompt: &str, _config: &AdaptivePlannerConfig) -> Result<String> {
            match (&self.response, &self.error) {
                (Some(response), None) => Ok(response.clone()),
                (None, Some(error)) => Err(anyhow!(error.clone())),
                _ => Err(anyhow!("mock executor misconfigured")),
            }
        }
    }

    fn enabled_config() -> AdaptivePlannerConfig {
        AdaptivePlannerConfig {
            enabled: true,
            planner_agent: Some("adaptive-agent".to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_adaptive_planner_disabled() {
        let planner = AdaptivePlanner::new(AdaptivePlannerConfig::default());
        let executor = MockExecutor {
            response: Some("{}".to_string()),
            error: None,
        };

        let result = planner
            .generate_plan(&executor, &StepPrehookContext::default())
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_adaptive_planner_config_default() {
        let cfg = AdaptivePlannerConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.planner_agent.is_none());
        assert_eq!(cfg.max_history, 10);
        assert!((cfg.temperature - 0.7).abs() < f32::EPSILON);
        assert_eq!(cfg.fallback_mode, AdaptiveFallbackMode::SoftFallback);
    }

    #[test]
    fn test_adaptive_planner_add_history_respects_max() {
        let mut planner = AdaptivePlanner::new(AdaptivePlannerConfig {
            max_history: 2,
            ..enabled_config()
        });

        for i in 0..5 {
            planner.add_history(ExecutionHistoryRecord {
                task_id: format!("task_{}", i),
                item_id: "item".to_string(),
                cycle: i,
                steps: vec![],
                final_status: "done".to_string(),
                timestamp: Utc::now(),
            });
        }
        assert_eq!(planner.history().len(), 2);
        assert_eq!(planner.history()[0].task_id, "task_3");
        assert_eq!(planner.history()[1].task_id, "task_4");
    }

    #[tokio::test]
    async fn test_adaptive_planner_generate_plan_enabled() {
        let planner = AdaptivePlanner::new(enabled_config());
        let executor = MockExecutor {
            response: Some(
                r#"{"entry":"qa","nodes":{"qa":{"id":"qa","step_type":"qa","repeatable":false},"fix":{"id":"fix","step_type":"fix","repeatable":true}},"edges":[{"from":"qa","to":"fix","condition":"active_ticket_count > 0"}]}"#
                    .to_string(),
            ),
            error: None,
        };

        let outcome = planner
            .generate_plan(&executor, &StepPrehookContext::default())
            .await
            .expect("adaptive planner should generate a plan when enabled");
        assert_eq!(outcome.metadata.source, AdaptivePlanSource::Planner);
        assert!(outcome.plan.nodes.contains_key("qa"));
        assert!(outcome.plan.nodes.contains_key("fix"));
        assert_eq!(outcome.plan.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_adaptive_planner_soft_fallback_on_invalid_json() {
        let planner = AdaptivePlanner::new(enabled_config());
        let executor = MockExecutor {
            response: Some("not-json".to_string()),
            error: None,
        };

        let outcome = planner
            .generate_plan(&executor, &StepPrehookContext::default())
            .await
            .expect("soft fallback should succeed");
        assert!(outcome.metadata.used_fallback);
        assert_eq!(
            outcome.metadata.error_class,
            Some(AdaptiveFailureClass::InvalidJson)
        );
        assert_eq!(
            outcome.metadata.source,
            AdaptivePlanSource::DeterministicFallback
        );
        assert_eq!(outcome.plan.entry.as_deref(), Some("qa"));
    }

    #[tokio::test]
    async fn test_adaptive_planner_fail_closed_on_invalid_json() {
        let planner = AdaptivePlanner::new(AdaptivePlannerConfig {
            fallback_mode: AdaptiveFallbackMode::FailClosed,
            ..enabled_config()
        });
        let executor = MockExecutor {
            response: Some("not-json".to_string()),
            error: None,
        };

        let err = planner
            .generate_plan(&executor, &StepPrehookContext::default())
            .await
            .expect_err("fail closed should error");
        assert!(err.to_string().contains("invalid_json"));
    }

    #[tokio::test]
    async fn test_adaptive_planner_rejects_missing_planner_agent() {
        let planner = AdaptivePlanner::new(AdaptivePlannerConfig {
            enabled: true,
            planner_agent: None,
            ..Default::default()
        });
        let executor = MockExecutor {
            response: Some("{}".to_string()),
            error: None,
        };

        let outcome = planner
            .generate_plan(&executor, &StepPrehookContext::default())
            .await
            .expect("soft fallback should handle misconfiguration");
        assert_eq!(
            outcome.metadata.error_class,
            Some(AdaptiveFailureClass::Misconfigured)
        );
    }

    #[test]
    fn test_validate_generated_plan_rejects_unknown_entry() {
        let plan = DynamicExecutionPlan {
            nodes: std::collections::HashMap::new(),
            edges: vec![],
            entry: Some("missing".to_string()),
        };
        let err = validate_generated_plan(&plan).expect_err("plan should fail");
        assert!(err.to_string().contains("at least one node"));
    }
}
