use std::collections::{HashMap, HashSet};

use crate::config::StepPrehookContext;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::step_pool::evaluate_trigger_condition;

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
            let degree = in_degree.get_mut(edge.to.as_str()).ok_or_else(|| {
                anyhow!("Topological sort failed: missing target node {}", edge.to)
            })?;
            *degree += 1;
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
                let degree = in_degree.get_mut(edge.to.as_str()).ok_or_else(|| {
                    anyhow!("Topological sort failed: missing target node {}", edge.to)
                })?;
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
                if evaluate_trigger_condition(condition, context).unwrap_or(false) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_orchestration::StepPrehookContext;

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
        .expect("add node a");

        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .expect("add node b");

        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .expect("add edge a->b");

        let sorted = plan.topological_sort().expect("topological sort");
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
        .expect("add node a");

        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .expect("add node b");

        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .expect("add edge a->b");

        plan.add_edge(WorkflowEdge {
            from: "b".to_string(),
            to: "a".to_string(),
            condition: None,
        })
        .expect("add edge b->a");

        assert!(plan.has_cycles());
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
        .expect("add node qa");

        plan.add_node(WorkflowNode {
            id: "fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .expect("add node fix");

        plan.add_node(WorkflowNode {
            id: "done".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: true,
            repeatable: false,
        })
        .expect("add node done");

        plan.add_edge(WorkflowEdge {
            from: "qa".to_string(),
            to: "fix".to_string(),
            condition: None,
        })
        .expect("add edge qa->fix");

        plan.add_edge(WorkflowEdge {
            from: "fix".to_string(),
            to: "done".to_string(),
            condition: Some("active_ticket_count == 0".to_string()),
        })
        .expect("add edge fix->done");

        let context = StepPrehookContext {
            active_ticket_count: 0,
            upstream_artifacts: Vec::new(),
            build_error_count: 0,
            test_failure_count: 0,
            build_exit_code: None,
            test_exit_code: None,
            ..Default::default()
        };

        let next_from_qa = plan.find_next_nodes("qa", &context);
        assert!(next_from_qa.contains(&"fix".to_string()));

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
        .expect("add node test");

        let node = plan.get_node("test");
        assert!(node.is_some());
        assert_eq!(node.expect("node test should exist").id, "test");

        let none = plan.get_node("nonexistent");
        assert!(none.is_none());
    }

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
        .expect("seed duplicate node a");

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
            .expect_err("operation should fail");
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
        .expect("seed node b");

        let err = plan
            .add_edge(WorkflowEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                condition: None,
            })
            .expect_err("operation should fail");
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
        .expect("seed node a");

        let err = plan
            .add_edge(WorkflowEdge {
                from: "a".to_string(),
                to: "b".to_string(),
                condition: None,
            })
            .expect_err("operation should fail");
        assert!(err.to_string().contains("Target node"));
    }

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
        .expect("add start");
        plan.add_node(WorkflowNode {
            id: "mid".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add mid");
        plan.add_node(WorkflowNode {
            id: "end".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add end");
        plan.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "mid".to_string(),
            condition: None,
        })
        .expect("add edge start->mid");
        plan.add_edge(WorkflowEdge {
            from: "mid".to_string(),
            to: "end".to_string(),
            condition: None,
        })
        .expect("add edge mid->end");

        let entries: Vec<&str> = plan
            .get_entry_nodes()
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"start"));

        let exits: Vec<&str> = plan
            .get_exit_nodes()
            .iter()
            .map(|n| n.id.as_str())
            .collect();
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
        .expect("add only node");

        assert_eq!(plan.get_entry_nodes().len(), 1);
        assert_eq!(plan.get_exit_nodes().len(), 1);
        assert!(!plan.has_cycles());
    }

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
        .expect("add node a");
        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add node b");
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .expect("add edge a->b");

        assert_eq!(plan.get_outgoing_edges("a").len(), 1);
        assert_eq!(plan.get_incoming_edges("b").len(), 1);
        assert!(plan.get_outgoing_edges("b").is_empty());
        assert!(plan.get_incoming_edges("a").is_empty());
        assert!(plan.get_outgoing_edges("nonexistent").is_empty());
    }

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
        .expect("add node a");
        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add node b");
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .expect("add edge a->b");
        plan.add_edge(WorkflowEdge {
            from: "b".to_string(),
            to: "a".to_string(),
            condition: None,
        })
        .expect("add edge b->a");

        let err = plan.topological_sort().expect_err("operation should fail");
        assert!(err.to_string().contains("cycles"));
    }

    #[test]
    fn test_dag_topological_sort_empty() {
        let plan = DynamicExecutionPlan::new();
        let sorted = plan.topological_sort().expect("empty topological sort");
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_dag_topological_sort_rejects_unknown_target() {
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
        .expect("add node a");
        plan.edges.push(WorkflowEdge {
            from: "a".to_string(),
            to: "ghost".to_string(),
            condition: None,
        });

        let err = plan.topological_sort().expect_err("operation should fail");
        assert!(err.to_string().contains("missing target node ghost"));
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
            .expect("add diamond node");
        }
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
        })
        .expect("add edge a->b");
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "c".to_string(),
            condition: None,
        })
        .expect("add edge a->c");
        plan.add_edge(WorkflowEdge {
            from: "b".to_string(),
            to: "d".to_string(),
            condition: None,
        })
        .expect("add edge b->d");
        plan.add_edge(WorkflowEdge {
            from: "c".to_string(),
            to: "d".to_string(),
            condition: None,
        })
        .expect("add edge c->d");

        let sorted = plan.topological_sort().expect("diamond topological sort");
        assert_eq!(sorted.len(), 4);
        let pos = |id: &str| {
            sorted
                .iter()
                .position(|s| s == id)
                .expect("id should be present in sorted output")
        };
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

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
        .expect("add node a");
        plan.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: "fix".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add node b");
        plan.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: Some("active_ticket_count > 0".to_string()),
        })
        .expect("add conditional edge a->b");

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
        .expect("add start node");
        plan.add_node(WorkflowNode {
            id: "end".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: false,
        })
        .expect("add end node");
        plan.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "end".to_string(),
            condition: None,
        })
        .expect("add edge start->end");

        let mut state = DagExecutionState::default();
        state.completed_nodes.insert("start".to_string());
        assert!(!plan.is_completed(&state));
    }

    #[test]
    fn test_prehook_config_default() {
        let cfg = PrehookConfig::default();
        assert_eq!(cfg.engine, "cel");
        assert_eq!(cfg.when, "true");
        assert!(cfg.reason.is_none());
        assert!(!cfg.extended);
    }

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
        .expect("add node n1");
        plan.entry = Some("n1".to_string());

        let json = serde_json::to_string(&plan).expect("serialize plan");
        let plan2: DynamicExecutionPlan = serde_json::from_str(&json).expect("deserialize plan");
        assert_eq!(plan2.entry, Some("n1".to_string()));
        assert!(plan2.nodes.contains_key("n1"));
    }

    #[test]
    fn test_dag_execution_state_default() {
        let state = DagExecutionState::default();
        assert!(state.current_node.is_none());
        assert!(state.completed_nodes.is_empty());
        assert!(state.skipped_nodes.is_empty());
        assert!(state.context.is_empty());
        assert!(state.branch_history.is_empty());
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
        .expect("add start node");

        plan.add_node(WorkflowNode {
            id: "end".to_string(),
            step_type: "done".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: true,
            repeatable: false,
        })
        .expect("add end node");

        plan.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "end".to_string(),
            condition: None,
        })
        .expect("add edge start->end");

        let mut state = DagExecutionState::default();

        assert!(!plan.is_completed(&state));

        state.completed_nodes.insert("end".to_string());

        assert!(plan.is_completed(&state));
    }
}
