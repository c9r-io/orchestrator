//! Dynamic Orchestration Module
//!
//! Provides enhanced prehook capabilities, dynamic step execution,
//! and DAG-based workflow orchestration for adaptive agent orchestration.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// Phase 1: Prehook 2.0 - Extended Return Types
// ============================================================================

/// Extended prehook decision that supports dynamic orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "data")]
#[derive(Default)]
pub enum PrehookDecision {
    /// Execute the step (default behavior)
    #[default]
    Run,
    /// Skip the step with a reason
    Skip {
        #[serde(default)]
        reason: String,
    },
    /// Branch to a different step
    Branch {
        /// Target step ID to jump to
        target: String,
        /// Context to pass to the target step
        #[serde(default)]
        context: HashMap<String, serde_json::Value>,
    },
    /// Dynamically add new steps to the execution plan
    DynamicAdd {
        /// Steps to add
        steps: Vec<DynamicStepInstance>,
    },
    /// Transform/replace the template for subsequent steps
    Transform {
        /// New template content
        template: String,
        /// Which step types to apply the transform to
        #[serde(default)]
        target_steps: Vec<String>,
    },
}

impl From<bool> for PrehookDecision {
    fn from(should_run: bool) -> Self {
        if should_run {
            Self::Run
        } else {
            Self::Skip {
                reason: "Condition evaluated to false".to_string(),
            }
        }
    }
}

impl PrehookDecision {
    /// Returns true if the step should be executed
    pub fn should_run(&self) -> bool {
        matches!(
            self,
            Self::Run | Self::DynamicAdd { .. } | Self::Transform { .. }
        )
    }

    /// Returns true if this decision involves branching
    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch { .. })
    }

    /// Returns true if this decision adds dynamic steps
    pub fn is_dynamic_add(&self) -> bool {
        matches!(self, Self::DynamicAdd { .. })
    }

    /// Returns true if this decision transforms templates
    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }
}

/// A dynamic step instance created at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicStepInstance {
    /// Unique identifier for this step instance
    pub id: String,
    /// Reference to the dynamic step definition
    pub source_id: String,
    /// The step type
    pub step_type: String,
    /// Agent ID to use (if applicable)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Template to execute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Additional context for this step
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

// ============================================================================
// Phase 2: Dynamic Step Registry
// ============================================================================

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
            // Use CEL evaluation via the main module
            // This is a simplified check - full implementation would call the CEL evaluator
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactSummary {
    pub phase: String,
    pub kind: String,
    pub path: Option<String>,
}

/// Simple condition evaluator for basic triggers
/// This is a placeholder - full implementation would use the CEL evaluator
fn evaluate_simple_condition(condition: &str, context: &StepPrehookContext) -> bool {
    // Basic condition evaluation for simple cases
    // In production, this would use the full CEL evaluator from main.rs

    // Check for common patterns
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

// ============================================================================
// Phase 3: DAG Execution Engine
// ============================================================================

/// A node in the workflow DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    /// Unique identifier for this node
    pub id: String,
    /// The step type
    pub step_type: String,
    /// Agent ID to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Template to execute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Prehook configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<PrehookConfig>,
    /// Whether this is a guard node
    #[serde(default)]
    pub is_guard: bool,
    /// Whether this node is repeatable
    #[serde(default = "default_true")]
    pub repeatable: bool,
}

fn default_true() -> bool {
    true
}

/// A directed edge in the workflow DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    /// Source node ID
    pub from: String,
    /// Target node ID
    pub to: String,
    /// CEL condition for this edge (None = unconditional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Prehook configuration for dynamic orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrehookConfig {
    /// Engine to use (cel, etc.)
    #[serde(default)]
    pub engine: String,
    /// Condition expression
    pub when: String,
    /// Reason (for logging)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether to return extended decision (Phase 1+)
    #[serde(default)]
    pub extended: bool,
}

impl Default for PrehookConfig {
    fn default() -> Self {
        Self {
            engine: "cel".to_string(),
            when: "true".to_string(),
            reason: None,
            extended: false,
        }
    }
}

/// Dynamic execution plan with DAG structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DynamicExecutionPlan {
    /// All nodes in the DAG
    #[serde(default)]
    pub nodes: HashMap<String, WorkflowNode>,
    /// All edges in the DAG
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    /// Entry point node ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

impl DynamicExecutionPlan {
    /// Create a new empty execution plan
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node to the plan
    pub fn add_node(&mut self, node: WorkflowNode) -> Result<()> {
        if self.nodes.contains_key(&node.id) {
            return Err(anyhow!("Node {} already exists", node.id));
        }
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// Add an edge to the plan
    pub fn add_edge(&mut self, edge: WorkflowEdge) -> Result<()> {
        // Validate that both nodes exist
        if !self.nodes.contains_key(&edge.from) {
            return Err(anyhow!("Source node {} does not exist", edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(anyhow!("Target node {} does not exist", edge.to));
        }
        self.edges.push(edge);
        Ok(())
    }

    /// Get nodes that have no incoming edges (starting points)
    pub fn get_entry_nodes(&self) -> Vec<&WorkflowNode> {
        let has_incoming: HashSet<&str> = self.edges.iter().map(|e| e.to.as_str()).collect();

        self.nodes
            .values()
            .filter(|n| !has_incoming.contains(n.id.as_str()))
            .collect()
    }

    /// Get nodes that have no outgoing edges (endpoints)
    pub fn get_exit_nodes(&self) -> Vec<&WorkflowNode> {
        let has_outgoing: HashSet<&str> = self.edges.iter().map(|e| e.from.as_str()).collect();

        self.nodes
            .values()
            .filter(|n| !has_outgoing.contains(n.id.as_str()))
            .collect()
    }

    /// Get outgoing edges from a node
    pub fn get_outgoing_edges(&self, node_id: &str) -> Vec<&WorkflowEdge> {
        self.edges.iter().filter(|e| e.from == node_id).collect()
    }

    /// Get incoming edges to a node
    pub fn get_incoming_edges(&self, node_id: &str) -> Vec<&WorkflowEdge> {
        self.edges.iter().filter(|e| e.to == node_id).collect()
    }

    /// Check for cycles in the DAG
    pub fn has_cycles(&self) -> bool {
        let mut visited: HashSet<String> = HashSet::new();
        let mut rec_stack: HashSet<String> = HashSet::new();

        fn dfs(
            node: String,
            plan: &DynamicExecutionPlan,
            visited: &mut HashSet<String>,
            rec_stack: &mut HashSet<String>,
        ) -> bool {
            visited.insert(node.clone());
            rec_stack.insert(node.clone());

            for edge in plan.get_outgoing_edges(&node) {
                let target = edge.to.clone();
                if !visited.contains(&target) {
                    if dfs(target, plan, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(&target) {
                    return true;
                }
            }

            rec_stack.remove(&node);
            false
        }

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id)
                && dfs(node_id.clone(), self, &mut visited, &mut rec_stack)
            {
                return true;
            }
        }

        false
    }

    /// Topological sort (returns nodes in execution order)
    /// Returns Err if there are cycles
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        if self.has_cycles() {
            return Err(anyhow!("Cannot topological sort: graph has cycles"));
        }

        let mut in_degree: HashMap<&str, usize> =
            self.nodes.keys().map(|k| (k.as_str(), 0)).collect();

        for edge in &self.edges {
            *in_degree.get_mut(&edge.to.as_str()).unwrap() += 1;
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(k, _)| *k)
            .collect();

        let mut result: Vec<String> = Vec::new();

        while let Some(node) = queue.pop() {
            result.push(node.to_string());

            for edge in self.get_outgoing_edges(node) {
                let degree = in_degree.get_mut(&edge.to.as_str()).unwrap();
                *degree -= 1;
                if *degree == 0 {
                    queue.push(&edge.to);
                }
            }
        }

        if result.len() != self.nodes.len() {
            return Err(anyhow!("Topological sort failed: graph has cycles"));
        }

        Ok(result)
    }

    pub fn find_next_nodes(
        &self,
        current_node_id: &str,
        context: &StepPrehookContext,
    ) -> Vec<String> {
        let mut next_nodes = Vec::new();

        for edge in self.get_outgoing_edges(current_node_id) {
            if let Some(ref condition) = edge.condition {
                if evaluate_simple_condition(condition, context) {
                    next_nodes.push(edge.to.clone());
                }
            } else {
                next_nodes.push(edge.to.clone());
            }
        }

        next_nodes
    }

    pub fn get_node(&self, node_id: &str) -> Option<&WorkflowNode> {
        self.nodes.get(node_id)
    }

    pub fn is_completed(&self, state: &DagExecutionState) -> bool {
        let exit_nodes = self.get_exit_nodes();
        for node in exit_nodes {
            if state.completed_nodes.contains(&node.id) {
                return true;
            }
        }
        false
    }
}

/// Execution state for the DAG engine
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DagExecutionState {
    /// Current node being executed
    pub current_node: Option<String>,
    /// Completed nodes
    #[serde(default)]
    pub completed_nodes: HashSet<String>,
    /// Skipped nodes
    #[serde(default)]
    pub skipped_nodes: HashSet<String>,
    /// Dynamic context accumulated during execution
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
    /// Branch history for debugging
    #[serde(default)]
    pub branch_history: Vec<BranchRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRecord {
    pub from_node: String,
    pub to_node: String,
    pub condition: Option<String>,
    pub result: bool,
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Phase 4: LLM-Driven Adaptive Planning (Interfaces)
// ============================================================================

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

        // Build prompt from context and history
        let _prompt = self.build_prompt(context);

        // In a real implementation, this would call an LLM
        // For now, return a simple default plan
        let mut plan = DynamicExecutionPlan::new();

        // Add default nodes
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

        // Add edges
        plan.add_edge(WorkflowEdge {
            from: "qa".to_string(),
            to: "fix".to_string(),
            condition: Some("qa_exit_code != 0 || active_ticket_count > 0".to_string()),
        })?;

        Ok(plan)
    }

    /// Build prompt from context and history
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prehook_decision_from_bool() {
        assert!(PrehookDecision::from(true).should_run());
        assert!(!PrehookDecision::from(false).should_run());
    }

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
    fn test_dag_topological_sort() {
        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "a".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .unwrap();

        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .unwrap();

        let sorted = plan.topological_sort().unwrap();
        assert_eq!(sorted, vec!["a", "b"]);
    }

    #[test]
    fn test_dag_cycle_detection() {
        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "a".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .unwrap();

        // Create a cycle: a -> b -> a
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .unwrap();

        plan.add_edge(WorkflowEdge {
            from: "b".to_string(),
            to: "a".to_string(),
            condition: None,
        })
        .unwrap();

        assert!(plan.has_cycles());
    }

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
    fn test_prehook_decision_branch() {
        let decision = PrehookDecision::Branch {
            target: "fix".to_string(),
            context: HashMap::new(),
        };
        assert!(decision.is_branch());
        assert!(!decision.should_run());
    }

    #[test]
    fn test_prehook_decision_dynamic_add() {
        let step = DynamicStepInstance {
            id: "dynamic_fix_1".to_string(),
            source_id: "quick_fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: Some("quick_fixer".to_string()),
            template: Some("fix {rel_path}".to_string()),
            context: HashMap::new(),
        };
        let decision = PrehookDecision::DynamicAdd { steps: vec![step] };
        assert!(decision.is_dynamic_add());
        assert!(decision.should_run());
    }

    #[test]
    fn test_prehook_decision_transform() {
        let decision = PrehookDecision::Transform {
            template: "new_template {rel_path}".to_string(),
            target_steps: vec!["fix".to_string()],
        };
        assert!(decision.is_transform());
        assert!(decision.should_run());
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
    fn test_dag_find_next_nodes() {
        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "qa".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        plan.add_node(WorkflowNode {
            id: "fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .unwrap();

        plan.add_node(WorkflowNode {
            id: "done".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: true,
            repeatable: false,
        })
        .unwrap();

        // Unconditional edge: qa -> fix
        plan.add_edge(WorkflowEdge {
            from: "qa".to_string(),
            to: "fix".to_string(),
            condition: None,
        })
        .unwrap();

        // Conditional edge: fix -> done (when no tickets)
        plan.add_edge(WorkflowEdge {
            from: "fix".to_string(),
            to: "done".to_string(),
            condition: Some("active_ticket_count == 0".to_string()),
        })
        .unwrap();

        let context = StepPrehookContext {
            active_ticket_count: 0,
            upstream_artifacts: Vec::new(),
            build_error_count: 0,
            test_failure_count: 0,
            build_exit_code: None,
            test_exit_code: None,
            ..Default::default()
        };

        // From qa should find fix (unconditional)
        let next_from_qa = plan.find_next_nodes("qa", &context);
        assert!(next_from_qa.contains(&"fix".to_string()));

        // From fix with 0 tickets should find done
        let next_from_fix = plan.find_next_nodes("fix", &context);
        assert!(next_from_fix.contains(&"done".to_string()));
    }

    #[test]
    fn test_dag_get_node() {
        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "test".to_string(),
            step_type: "qa".to_string(),
            agent_id: Some("echo".to_string()),
            template: Some("echo test".to_string()),
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        let node = plan.get_node("test");
        assert!(node.is_some());
        assert_eq!(node.unwrap().id, "test");

        let none = plan.get_node("nonexistent");
        assert!(none.is_none());
    }

    #[test]
    fn test_dag_is_completed() {
        let mut plan = DynamicExecutionPlan::new();

        plan.add_node(WorkflowNode {
            id: "start".to_string(),
            step_type: "init".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        plan.add_node(WorkflowNode {
            id: "end".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: true,
            repeatable: false,
        })
        .unwrap();

        plan.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "end".to_string(),
            condition: None,
        })
        .unwrap();

        let mut state = DagExecutionState::default();

        // Not completed yet
        assert!(!plan.is_completed(&state));

        // Mark end node as completed
        state.completed_nodes.insert("end".to_string());

        // Now should be completed
        assert!(plan.is_completed(&state));
    }
}
