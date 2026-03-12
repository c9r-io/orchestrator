use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Directed acyclic graph definition used by collaboration workflows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDag {
    /// Stable workflow identifier.
    pub id: String,
    /// Human-readable workflow name.
    pub name: String,
    /// Nodes keyed by node identifier.
    pub nodes: HashMap<String, WorkflowNode>,
    /// Directed edges describing dependencies.
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDag {
    /// Creates an empty DAG with the given identifier and name.
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    /// Inserts or replaces a node by its identifier.
    pub fn add_node(&mut self, node: WorkflowNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Appends a directed edge.
    pub fn add_edge(&mut self, edge: WorkflowEdge) {
        self.edges.push(edge);
    }

    /// Returns node identifiers that have no incoming edges.
    pub fn get_entry_nodes(&self) -> Vec<&String> {
        let targets: std::collections::HashSet<_> = self.edges.iter().map(|e| &e.to).collect();

        self.nodes.keys().filter(|k| !targets.contains(k)).collect()
    }

    /// Returns nodes whose dependencies have all been completed.
    pub fn get_ready_nodes(&self, completed: &std::collections::HashSet<String>) -> Vec<String> {
        self.nodes
            .keys()
            .filter(|k| !completed.contains(*k))
            .filter(|k| {
                let deps = self.get_dependencies(k);
                deps.iter().all(|d| completed.contains(d))
            })
            .cloned()
            .collect()
    }

    fn get_dependencies(&self, node_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.to == node_id)
            .map(|e| e.from.clone())
            .collect()
    }
}

/// Executable node inside a collaboration DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    /// Stable node identifier.
    pub id: String,
    /// Step kind associated with the node.
    pub step_type: StepType,
    /// Agent-selection requirements for the node.
    pub agent_requirement: AgentRequirement,
    /// Optional prehook expression that gates execution.
    pub prehook: Option<String>,
    /// Runtime execution settings for the node.
    pub config: NodeConfig,
}

/// Agent-selection constraints attached to a DAG node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequirement {
    /// Required capability for candidate agents.
    pub capability: Option<String>,
    /// Preferred agent identifiers ranked ahead of others.
    pub preferred_agents: Vec<String>,
    /// Optional minimum historical success rate for selection.
    pub min_success_rate: Option<f32>,
}

/// Runtime configuration for a DAG node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    /// Optional timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Enables retry behavior for node execution.
    pub retry_enabled: bool,
    /// Maximum retry count when retries are enabled.
    pub max_retries: u32,
}

/// Directed edge between two workflow nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    /// Upstream node identifier.
    pub from: String,
    /// Downstream node identifier.
    pub to: String,
    /// Optional expression that must pass for the edge to activate.
    pub condition: Option<String>,
    /// Optional transform applied to upstream output before passing it forward.
    pub transform: Option<OutputTransform>,
}

/// Mapping from upstream output into downstream shared state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTransform {
    /// Source phase used to select upstream output.
    pub source_phase: String,
    /// Extraction strategy applied to the source output.
    pub extraction: OutputExtraction,
    /// Shared-state key populated on the downstream node.
    pub target_key: String,
}

/// Supported output extraction strategies for DAG edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputExtraction {
    /// Forward all artifacts from the source phase.
    AllArtifacts,
    /// Forward artifacts matching a single artifact kind string.
    ArtifactKind(String),
    /// Forward only the last `N` artifacts.
    LastN(u32),
    /// Apply a custom filter expression.
    Filter(String),
}

/// Logical step kinds used by collaboration DAG nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepType {
    /// One-time initialization step.
    InitOnce,
    /// Quality-assurance step.
    Qa,
    /// Ticket-scanning step.
    TicketScan,
    /// Remediation or implementation step.
    Fix,
    /// Re-test step after a fix.
    Retest,
    /// Loop-guard or termination-check step.
    LoopGuard,
    /// User-defined custom step type.
    Custom(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_dag_entry_nodes() {
        let mut dag = WorkflowDag::new("test".to_string(), "Test Workflow".to_string());

        dag.add_node(WorkflowNode {
            id: "start".to_string(),
            step_type: StepType::InitOnce,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });

        dag.add_node(WorkflowNode {
            id: "qa".to_string(),
            step_type: StepType::Qa,
            agent_requirement: AgentRequirement {
                capability: Some("qa".to_string()),
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });

        dag.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "qa".to_string(),
            condition: None,
            transform: None,
        });

        let entries = dag.get_entry_nodes();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "start");
    }

    #[test]
    fn test_workflow_dag_get_ready_nodes() {
        let mut dag = WorkflowDag::new("test".to_string(), "Test".to_string());

        dag.add_node(WorkflowNode {
            id: "a".to_string(),
            step_type: StepType::InitOnce,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });
        dag.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: StepType::Qa,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });
        dag.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
            transform: None,
        });

        let completed = std::collections::HashSet::new();
        let ready = dag.get_ready_nodes(&completed);
        assert_eq!(ready.len(), 1);
        assert!(ready.contains(&"a".to_string()));

        let mut completed = std::collections::HashSet::new();
        completed.insert("a".to_string());
        let ready = dag.get_ready_nodes(&completed);
        assert_eq!(ready.len(), 1);
        assert!(ready.contains(&"b".to_string()));
    }
}
