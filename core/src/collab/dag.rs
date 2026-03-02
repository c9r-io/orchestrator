use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Directed Acyclic Graph workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDag {
    pub id: String,
    pub name: String,
    pub nodes: HashMap<String, WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDag {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: WorkflowNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: WorkflowEdge) {
        self.edges.push(edge);
    }

    pub fn get_entry_nodes(&self) -> Vec<&String> {
        let targets: std::collections::HashSet<_> = self.edges.iter().map(|e| &e.to).collect();

        self.nodes.keys().filter(|k| !targets.contains(k)).collect()
    }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub step_type: StepType,
    pub agent_requirement: AgentRequirement,
    pub prehook: Option<String>,
    pub config: NodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequirement {
    pub capability: Option<String>,
    pub preferred_agents: Vec<String>,
    pub min_success_rate: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    pub timeout_ms: Option<u64>,
    pub retry_enabled: bool,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
    pub transform: Option<OutputTransform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTransform {
    pub source_phase: String,
    pub extraction: OutputExtraction,
    pub target_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputExtraction {
    AllArtifacts,
    ArtifactKind(String),
    LastN(u32),
    Filter(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepType {
    InitOnce,
    Qa,
    TicketScan,
    Fix,
    Retest,
    LoopGuard,
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
