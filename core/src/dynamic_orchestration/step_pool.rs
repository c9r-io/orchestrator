use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for a dynamic step in the pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicStepConfig {
    /// Unique identifier for this dynamic step
    pub id: String,
    /// Description for documentation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The step type
    pub step_type: String,
    /// Agent ID to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Template for the agent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// CEL trigger condition - when to consider this step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Priority (higher = more likely to be selected)
    #[serde(default)]
    pub priority: i32,
    /// Maximum times this step can be executed per item
    #[serde(default)]
    pub max_runs: Option<u32>,
}

impl DynamicStepConfig {
    /// Check if this step matches the current context
    pub fn matches(&self, context: &StepPrehookContext) -> bool {
        if let Some(ref trigger) = self.trigger {
            return evaluate_simple_condition(trigger, context);
        }
        false
    }
}

/// Pool of dynamic steps available for runtime selection
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DynamicStepPool {
    /// Map of dynamic step ID -> config
    #[serde(default)]
    pub steps: HashMap<String, DynamicStepConfig>,
}

impl DynamicStepPool {
    /// Create a new empty pool
    pub fn new() -> Self {
        Self {
            steps: HashMap::new(),
        }
    }

    /// Add a step to the pool
    pub fn add_step(&mut self, step: DynamicStepConfig) {
        self.steps.insert(step.id.clone(), step);
    }

    /// Find steps that match the current context
    pub fn find_matching_steps(&self, context: &StepPrehookContext) -> Vec<&DynamicStepConfig> {
        let mut matches: Vec<_> = self
            .steps
            .values()
            .filter(|step| step.matches(context))
            .collect();

        // Sort by priority (descending)
        matches.sort_by(|a, b| b.priority.cmp(&a.priority));
        matches
    }

    /// Get a step by ID
    pub fn get(&self, id: &str) -> Option<&DynamicStepConfig> {
        self.steps.get(id)
    }
}

/// Context passed to dynamic step evaluation
/// Mirrors the main module's StepPrehookContext for compatibility
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepPrehookContext {
    pub task_id: String,
    pub task_item_id: String,
    pub cycle: u32,
    pub step: String,
    pub qa_file_path: String,
    pub item_status: String,
    pub task_status: String,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub qa_failed: bool,
    pub fix_required: bool,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_has_changes: Option<bool>,
    #[serde(default)]
    pub upstream_artifacts: Vec<ArtifactSummary>,
    #[serde(default)]
    pub build_error_count: i64,
    #[serde(default)]
    pub test_failure_count: i64,
    pub build_exit_code: Option<i64>,
    pub test_exit_code: Option<i64>,
    #[serde(default)]
    pub self_test_exit_code: Option<i64>,
    #[serde(default)]
    pub self_test_passed: bool,
    #[serde(default)]
    pub max_cycles: u32,
    #[serde(default)]
    pub is_last_cycle: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactSummary {
    pub phase: String,
    pub kind: String,
    pub path: Option<String>,
}

/// Simple condition evaluator for basic triggers
pub(crate) fn evaluate_simple_condition(condition: &str, context: &StepPrehookContext) -> bool {
    if condition.contains("active_ticket_count") {
        if condition.contains("> 0") && context.active_ticket_count > 0 {
            return true;
        }
        if condition.contains("== 0") && context.active_ticket_count == 0 {
            return true;
        }
    }

    if condition.contains("qa_exit_code") {
        if condition.contains("!= 0") && context.qa_exit_code.is_some_and(|c| c != 0) {
            return true;
        }
        if condition.contains("== 0") && (context.qa_exit_code == Some(0)) {
            return true;
        }
    }

    if condition.contains("qa_confidence") {
        if let Some(confidence) = context.qa_confidence {
            if condition.contains("> 0.8") && confidence > 0.8 {
                return true;
            }
            if condition.contains("> 0.5") && confidence > 0.5 {
                return true;
            }
        }
    }

    if condition.contains("cycle") {
        if condition.contains("> 2") && context.cycle > 2 {
            return true;
        }
        if condition.contains("> 0") && context.cycle > 0 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_step_pool() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "step1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: Some("fixer".to_string()),
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 10,
            max_runs: None,
        });

        let context = StepPrehookContext {
            active_ticket_count: 5,
            upstream_artifacts: Vec::new(),
            build_error_count: 0,
            test_failure_count: 0,
            build_exit_code: None,
            test_exit_code: None,
            ..Default::default()
        };

        let matches = pool.find_matching_steps(&context);
        assert!(!matches.is_empty());
    }

    #[test]
    fn test_dynamic_step_pool_priority() {
        let mut pool = DynamicStepPool::new();

        pool.add_step(DynamicStepConfig {
            id: "low_priority".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 1,
            max_runs: None,
        });

        pool.add_step(DynamicStepConfig {
            id: "high_priority".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 100,
            max_runs: None,
        });

        let context = StepPrehookContext {
            active_ticket_count: 5,
            upstream_artifacts: Vec::new(),
            build_error_count: 0,
            test_failure_count: 0,
            build_exit_code: None,
            test_exit_code: None,
            ..Default::default()
        };

        let matches = pool.find_matching_steps(&context);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].id, "high_priority");
    }

    #[test]
    fn test_dynamic_step_pool_empty() {
        let pool = DynamicStepPool::new();
        assert!(pool.steps.is_empty());
        let ctx = StepPrehookContext::default();
        let matches = pool.find_matching_steps(&ctx);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_dynamic_step_pool_get() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "s1".to_string(),
            description: Some("desc".to_string()),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: None,
            priority: 0,
            max_runs: Some(3),
        });
        assert!(pool.get("s1").is_some());
        assert_eq!(pool.get("s1").unwrap().max_runs, Some(3));
        assert!(pool.get("nonexistent").is_none());
    }

    #[test]
    fn test_dynamic_step_pool_overwrite() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: None,
            priority: 1,
            max_runs: None,
        });
        pool.add_step(DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            trigger: None,
            priority: 99,
            max_runs: None,
        });
        assert_eq!(pool.steps.len(), 1);
        assert_eq!(pool.get("s1").unwrap().step_type, "qa");
        assert_eq!(pool.get("s1").unwrap().priority, 99);
    }

    #[test]
    fn test_dynamic_step_no_trigger_does_not_match() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: None,
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            active_ticket_count: 5,
            ..Default::default()
        };
        assert!(!step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_qa_exit_code_nonzero() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("qa_exit_code != 0".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            qa_exit_code: Some(1),
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_qa_exit_code_zero() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("qa_exit_code == 0".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            qa_exit_code: Some(0),
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_qa_confidence_high() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("qa_confidence > 0.8".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            qa_confidence: Some(0.9),
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_qa_confidence_low() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("qa_confidence > 0.8".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            qa_confidence: Some(0.3),
            ..Default::default()
        };
        assert!(!step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_qa_confidence_medium() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("qa_confidence > 0.5".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            qa_confidence: Some(0.6),
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_cycle_gt_2() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("cycle > 2".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            cycle: 3,
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_cycle_gt_0() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("cycle > 0".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            cycle: 1,
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_does_not_match_cycle_zero() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("cycle > 0".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            cycle: 0,
            ..Default::default()
        };
        assert!(!step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_matches_active_tickets_zero() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count == 0".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext {
            active_ticket_count: 0,
            ..Default::default()
        };
        assert!(step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_unknown_condition_returns_false() {
        let step = DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("some_unknown_field == true".to_string()),
            priority: 0,
            max_runs: None,
        };
        let ctx = StepPrehookContext::default();
        assert!(!step.matches(&ctx));
    }

    #[test]
    fn test_dynamic_step_pool_priority_three_steps() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "low".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: -5,
            max_runs: None,
        });
        pool.add_step(DynamicStepConfig {
            id: "mid".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 0,
            max_runs: None,
        });
        pool.add_step(DynamicStepConfig {
            id: "high".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 50,
            max_runs: None,
        });
        let ctx = StepPrehookContext {
            active_ticket_count: 1,
            ..Default::default()
        };
        let matches = pool.find_matching_steps(&ctx);
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].id, "high");
        assert_eq!(matches[1].id, "mid");
        assert_eq!(matches[2].id, "low");
    }

    #[test]
    fn test_dynamic_step_pool_no_matches() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "s1".to_string(),
            description: None,
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 10,
            max_runs: None,
        });
        let ctx = StepPrehookContext {
            active_ticket_count: 0,
            ..Default::default()
        };
        let matches = pool.find_matching_steps(&ctx);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_dynamic_step_pool_serde_round_trip() {
        let mut pool = DynamicStepPool::new();
        pool.add_step(DynamicStepConfig {
            id: "s1".to_string(),
            description: Some("my step".to_string()),
            step_type: "fix".to_string(),
            agent_id: Some("agent1".to_string()),
            template: Some("tpl".to_string()),
            trigger: Some("active_ticket_count > 0".to_string()),
            priority: 42,
            max_runs: Some(3),
        });
        let json = serde_json::to_string(&pool).unwrap();
        let pool2: DynamicStepPool = serde_json::from_str(&json).unwrap();
        assert_eq!(pool2.steps.len(), 1);
        let s = pool2.get("s1").unwrap();
        assert_eq!(s.priority, 42);
        assert_eq!(s.max_runs, Some(3));
    }

    #[test]
    fn test_step_prehook_context_default() {
        let ctx = StepPrehookContext::default();
        assert_eq!(ctx.cycle, 0);
        assert_eq!(ctx.active_ticket_count, 0);
        assert!(!ctx.qa_failed);
        assert!(!ctx.fix_required);
        assert!(ctx.qa_exit_code.is_none());
        assert!(!ctx.self_test_passed);
        assert!(!ctx.is_last_cycle);
        assert_eq!(ctx.max_cycles, 0);
    }
}
