use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::dag::{DynamicExecutionPlan, PrehookConfig, WorkflowEdge, WorkflowNode};
use super::step_pool::StepPrehookContext;

/// Configuration for LLM-driven adaptive planning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptivePlannerConfig {
    /// Whether adaptive planning is enabled
    #[serde(default)]
    pub enabled: bool,
    /// LLM provider (openai, anthropic, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Maximum number of history entries to consider
    #[serde(default = "default_10")]
    pub max_history: usize,
    /// Temperature for LLM generation
    #[serde(default = "default_07")]
    pub temperature: f32,
}

fn default_10() -> usize {
    10
}

fn default_07() -> f32 {
    0.7
}

impl Default for AdaptivePlannerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            max_history: 10,
            temperature: 0.7,
        }
    }
}

/// Historical execution record for learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionHistoryRecord {
    /// Task ID
    pub task_id: String,
    /// Item ID
    pub item_id: String,
    /// Cycle number
    pub cycle: u32,
    /// Steps executed
    pub steps: Vec<StepExecutionRecord>,
    /// Final status
    pub final_status: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecutionRecord {
    pub step_id: String,
    pub step_type: String,
    pub exit_code: i64,
    pub duration_ms: u64,
    pub confidence: Option<f32>,
    pub quality_score: Option<f32>,
    pub tickets_created: i64,
    pub tickets_resolved: i64,
}

/// Trait for LLM clients (placeholder for actual implementation)
pub trait LlmClient: Send + Sync {
    /// Generate a response from the LLM
    fn generate(&self, prompt: &str, config: &AdaptivePlannerConfig) -> Result<String>;

    /// Generate a JSON-structured response
    fn generate_json(
        &self,
        prompt: &str,
        config: &AdaptivePlannerConfig,
    ) -> Result<serde_json::Value>;
}

/// Adaptive planner that uses LLM to generate execution plans
#[derive(Debug, Clone)]
pub struct AdaptivePlanner {
    config: AdaptivePlannerConfig,
    history: Vec<ExecutionHistoryRecord>,
}

impl AdaptivePlanner {
    /// Create a new adaptive planner
    pub fn new(config: AdaptivePlannerConfig) -> Self {
        Self {
            config,
            history: Vec::new(),
        }
    }

    /// Add a history record
    pub fn add_history(&mut self, record: ExecutionHistoryRecord) {
        if self.history.len() >= self.config.max_history {
            self.history.remove(0);
        }
        self.history.push(record);
    }

    /// Generate an execution plan based on context and history
    pub fn generate_plan(&self, context: &StepPrehookContext) -> Result<DynamicExecutionPlan> {
        if !self.config.enabled {
            return Err(anyhow!("Adaptive planning is not enabled"));
        }

        tracing::warn!("adaptive planner using hardcoded fallback; LLM integration not implemented");

        let _prompt = self.build_prompt(context);

        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "qa".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })?;

        plan.add_node(WorkflowNode {
            id: "fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: Some(PrehookConfig {
                engine: "cel".to_string(),
                when: "active_ticket_count > 0".to_string(),
                reason: Some("Only run fix when there are tickets".to_string()),
                extended: false,
            }),
            is_guard: false,
            repeatable: true,
        })?;

        plan.add_edge(WorkflowEdge {
            from: "qa".to_string(),
            to: "fix".to_string(),
            condition: Some("qa_exit_code != 0 || active_ticket_count > 0".to_string()),
        })?;

        Ok(plan)
    }

    fn build_prompt(&self, context: &StepPrehookContext) -> String {
        let history_json = serde_json::to_string(&self.history).unwrap_or_default();

        format!(
            r#"Context:
- Task: {}
- Item: {}
- Cycle: {}
- QA Exit Code: {:?}
- Active Tickets: {}
- QA Confidence: {:?}

Recent History:
{}

Generate a dynamic execution plan as JSON with 'nodes' and 'edges'."#,
            context.task_id,
            context.task_item_id,
            context.cycle,
            context.qa_exit_code,
            context.active_ticket_count,
            context.qa_confidence,
            history_json
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_planner_disabled() {
        let config = AdaptivePlannerConfig {
            enabled: false,
            ..Default::default()
        };
        let planner = AdaptivePlanner::new(config);

        let context = StepPrehookContext::default();
        let result = planner.generate_plan(&context);
        assert!(result.is_err());
    }

    #[test]
    fn test_adaptive_planner_config_default() {
        let cfg = AdaptivePlannerConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.provider.is_none());
        assert!(cfg.model.is_none());
        assert_eq!(cfg.max_history, 10);
        assert!((cfg.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn test_adaptive_planner_add_history_respects_max() {
        let config = AdaptivePlannerConfig {
            max_history: 2,
            ..Default::default()
        };
        let mut planner = AdaptivePlanner::new(config);

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
        assert_eq!(planner.history.len(), 2);
        assert_eq!(planner.history[0].task_id, "task_3");
        assert_eq!(planner.history[1].task_id, "task_4");
    }

    #[test]
    fn test_adaptive_planner_generate_plan_enabled() {
        let config = AdaptivePlannerConfig {
            enabled: true,
            ..Default::default()
        };
        let planner = AdaptivePlanner::new(config);
        let ctx = StepPrehookContext::default();
        let plan = planner
            .generate_plan(&ctx)
            .expect("adaptive planner should generate a plan when enabled");
        assert!(plan.nodes.contains_key("qa"));
        assert!(plan.nodes.contains_key("fix"));
        assert_eq!(plan.edges.len(), 1);
    }
}
