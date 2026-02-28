//! Dynamic Orchestration Module
//!
//! Provides enhanced prehook capabilities, dynamic step execution,
//! and DAG-based workflow orchestration for adaptive agent orchestration.

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

    // ===== New tests for improved coverage =====

    #[test]
    fn test_prehook_decision_default_is_run() {
        let decision = PrehookDecision::default();
        assert!(decision.should_run());
        assert!(!decision.is_branch());
        assert!(!decision.is_dynamic_add());
        assert!(!decision.is_transform());
    }

    #[test]
    fn test_prehook_decision_skip_does_not_run() {
        let decision = PrehookDecision::Skip {
            reason: "test reason".to_string(),
        };
        assert!(!decision.should_run());
        assert!(!decision.is_branch());
        assert!(!decision.is_dynamic_add());
        assert!(!decision.is_transform());
    }

    #[test]
    fn test_prehook_decision_serde_run() {
        let decision = PrehookDecision::Run;
        let json = serde_json::to_string(&decision).unwrap();
        let parsed: PrehookDecision = serde_json::from_str(&json).unwrap();
        assert!(parsed.should_run());
    }

    #[test]
    fn test_prehook_decision_serde_skip() {
        let json = r#"{"action":"Skip","data":{"reason":"no need"}}"#;
        let decision: PrehookDecision = serde_json::from_str(json).unwrap();
        assert!(!decision.should_run());
    }

    #[test]
    fn test_prehook_decision_serde_branch() {
        let json = r#"{"action":"Branch","data":{"target":"fix","context":{}}}"#;
        let decision: PrehookDecision = serde_json::from_str(json).unwrap();
        assert!(decision.is_branch());
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

    // ===== DAG: add_node / add_edge errors =====

    #[test]
    fn test_dag_add_duplicate_node_error() {
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

        let err = plan
            .add_node(WorkflowNode {
                id: "a".to_string(),
                step_type: "fix".to_string(),
                agent_id: None,
                template: None,
                prehook: None,
                is_guard: false,
                repeatable: true,
            })
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_dag_add_edge_missing_source() {
        let mut plan = DynamicExecutionPlan::new();
        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        let err = plan
            .add_edge(WorkflowEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                condition: None,
            })
            .unwrap_err();
        assert!(err.to_string().contains("Source node"));
    }

    #[test]
    fn test_dag_add_edge_missing_target() {
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

        let err = plan
            .add_edge(WorkflowEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                condition: None,
            })
            .unwrap_err();
        assert!(err.to_string().contains("Target node"));
    }

    // ===== DAG: entry/exit nodes =====

    #[test]
    fn test_dag_entry_exit_nodes() {
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
            id: "mid".to_string(),
            step_type: "qa".to_string(),
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
            is_guard: false,
            repeatable: false,
        })
        .unwrap();
        plan.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "mid".to_string(),
            condition: None,
        })
        .unwrap();
        plan.add_edge(WorkflowEdge {
            from: "mid".to_string(),
            to: "end".to_string(),
            condition: None,
        })
        .unwrap();

        let entries: Vec<&str> = plan.get_entry_nodes().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"start"));

        let exits: Vec<&str> = plan.get_exit_nodes().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(exits.len(), 1);
        assert!(exits.contains(&"end"));
    }

    #[test]
    fn test_dag_empty_plan_no_entries_no_exits() {
        let plan = DynamicExecutionPlan::new();
        assert!(plan.get_entry_nodes().is_empty());
        assert!(plan.get_exit_nodes().is_empty());
        assert!(!plan.has_cycles());
    }

    #[test]
    fn test_dag_single_node_is_both_entry_and_exit() {
        let mut plan = DynamicExecutionPlan::new();
        plan.add_node(WorkflowNode {
            id: "only".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .unwrap();

        assert_eq!(plan.get_entry_nodes().len(), 1);
        assert_eq!(plan.get_exit_nodes().len(), 1);
        assert!(!plan.has_cycles());
    }

    // ===== DAG: incoming/outgoing edges =====

    #[test]
    fn test_dag_incoming_outgoing_edges() {
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
            repeatable: false,
        })
        .unwrap();
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .unwrap();

        assert_eq!(plan.get_outgoing_edges("a").len(), 1);
        assert_eq!(plan.get_incoming_edges("b").len(), 1);
        assert!(plan.get_outgoing_edges("b").is_empty());
        assert!(plan.get_incoming_edges("a").is_empty());
        assert!(plan.get_outgoing_edges("nonexistent").is_empty());
    }

    // ===== DAG: topological sort with cycle =====

    #[test]
    fn test_dag_topological_sort_cycle_error() {
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
            repeatable: false,
        })
        .unwrap();
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

        let err = plan.topological_sort().unwrap_err();
        assert!(err.to_string().contains("cycles"));
    }

    #[test]
    fn test_dag_topological_sort_empty() {
        let plan = DynamicExecutionPlan::new();
        let sorted = plan.topological_sort().unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_dag_topological_sort_diamond() {
        let mut plan = DynamicExecutionPlan::new();
        for id in &["a", "b", "c", "d"] {
            plan.add_node(WorkflowNode {
                id: id.to_string(),
                step_type: "step".to_string(),
                agent_id: None,
                template: None,
                prehook: None,
                is_guard: false,
                repeatable: false,
            })
            .unwrap();
        }
        // a -> b, a -> c, b -> d, c -> d
        plan.add_edge(WorkflowEdge { from: "a".to_string(), to: "b".to_string(), condition: None }).unwrap();
        plan.add_edge(WorkflowEdge { from: "a".to_string(), to: "c".to_string(), condition: None }).unwrap();
        plan.add_edge(WorkflowEdge { from: "b".to_string(), to: "d".to_string(), condition: None }).unwrap();
        plan.add_edge(WorkflowEdge { from: "c".to_string(), to: "d".to_string(), condition: None }).unwrap();

        let sorted = plan.topological_sort().unwrap();
        assert_eq!(sorted.len(), 4);
        // a must be before b and c; d must be last
        let pos = |id: &str| sorted.iter().position(|s| s == id).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

    // ===== DAG: find_next_nodes with conditions =====

    #[test]
    fn test_dag_find_next_nodes_conditional_not_met() {
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
            repeatable: false,
        })
        .unwrap();
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: Some("active_ticket_count > 0".to_string()),
        })
        .unwrap();

        let ctx = StepPrehookContext {
            active_ticket_count: 0,
            ..Default::default()
        };
        let next = plan.find_next_nodes("a", &ctx);
        assert!(next.is_empty());
    }

    #[test]
    fn test_dag_find_next_nodes_nonexistent_node() {
        let plan = DynamicExecutionPlan::new();
        let ctx = StepPrehookContext::default();
        let next = plan.find_next_nodes("nope", &ctx);
        assert!(next.is_empty());
    }

    // ===== DAG: is_completed =====

    #[test]
    fn test_dag_is_completed_not_completed_when_only_mid_done() {
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
            is_guard: false,
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
        state.completed_nodes.insert("start".to_string());
        // "start" is not an exit node, so not completed
        assert!(!plan.is_completed(&state));
    }

    // ===== PrehookConfig default =====

    #[test]
    fn test_prehook_config_default() {
        let cfg = PrehookConfig::default();
        assert_eq!(cfg.engine, "cel");
        assert_eq!(cfg.when, "true");
        assert!(cfg.reason.is_none());
        assert!(!cfg.extended);
    }

    // ===== AdaptivePlannerConfig default =====

    #[test]
    fn test_adaptive_planner_config_default() {
        let cfg = AdaptivePlannerConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.provider.is_none());
        assert!(cfg.model.is_none());
        assert_eq!(cfg.max_history, 10);
        assert!((cfg.temperature - 0.7).abs() < f32::EPSILON);
    }

    // ===== AdaptivePlanner: history management =====

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
        // oldest records should have been evicted
        assert_eq!(planner.history[0].task_id, "task_3");
        assert_eq!(planner.history[1].task_id, "task_4");
    }

    // ===== AdaptivePlanner: generate_plan when enabled =====

    #[test]
    fn test_adaptive_planner_generate_plan_enabled() {
        let config = AdaptivePlannerConfig {
            enabled: true,
            ..Default::default()
        };
        let planner = AdaptivePlanner::new(config);
        let ctx = StepPrehookContext::default();
        let plan = planner.generate_plan(&ctx).unwrap();
        // The default plan adds qa and fix nodes
        assert!(plan.nodes.contains_key("qa"));
        assert!(plan.nodes.contains_key("fix"));
        assert_eq!(plan.edges.len(), 1);
    }

    // ===== DagExecutionState default =====

    #[test]
    fn test_dag_execution_state_default() {
        let state = DagExecutionState::default();
        assert!(state.current_node.is_none());
        assert!(state.completed_nodes.is_empty());
        assert!(state.skipped_nodes.is_empty());
        assert!(state.context.is_empty());
        assert!(state.branch_history.is_empty());
    }

    // ===== StepPrehookContext default =====

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

    // ===== DynamicStepPool serde =====

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

    // ===== DynamicExecutionPlan serde =====

    #[test]
    fn test_dynamic_execution_plan_serde_round_trip() {
        let mut plan = DynamicExecutionPlan::new();
        plan.add_node(WorkflowNode {
            id: "n1".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .unwrap();
        plan.entry = Some("n1".to_string());

        let json = serde_json::to_string(&plan).unwrap();
        let plan2: DynamicExecutionPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan2.entry, Some("n1".to_string()));
        assert!(plan2.nodes.contains_key("n1"));
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
