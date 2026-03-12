use std::collections::HashMap;

use crate::config::{StepPrehookConfig, StepScope, TaskExecutionStep, TaskRuntimeContext};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionGraphSource {
    #[default]
    StaticBaseline,
    AdaptivePlanner,
    DeterministicFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionGraphNodeSpec {
    StaticStep {
        step_id: String,
    },
    DynamicStep {
        step_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraphNode {
    pub id: String,
    pub scope: StepScope,
    #[serde(default)]
    pub repeatable: bool,
    #[serde(default)]
    pub is_guard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<StepPrehookConfig>,
    pub spec: ExecutionGraphNodeSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionGraphEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EffectiveExecutionGraph {
    #[serde(default)]
    pub source: ExecutionGraphSource,
    #[serde(default)]
    pub nodes: HashMap<String, ExecutionGraphNode>,
    #[serde(default)]
    pub edges: Vec<ExecutionGraphEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

impl EffectiveExecutionGraph {
    pub fn add_node(&mut self, node: ExecutionGraphNode) -> Result<()> {
        if self.nodes.insert(node.id.clone(), node).is_some() {
            return Err(anyhow!("graph node '{}' already exists", self.nodes.len()));
        }
        Ok(())
    }

    pub fn add_edge(&mut self, edge: ExecutionGraphEdge) -> Result<()> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(anyhow!("graph edge source '{}' does not exist", edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(anyhow!("graph edge target '{}' does not exist", edge.to));
        }
        self.edges.push(edge);
        Ok(())
    }

    pub fn get_node(&self, node_id: &str) -> Option<&ExecutionGraphNode> {
        self.nodes.get(node_id)
    }

    pub fn outgoing_edges(&self, node_id: &str) -> Vec<&ExecutionGraphEdge> {
        self.edges
            .iter()
            .filter(|edge| edge.from == node_id)
            .collect()
    }

    pub fn incoming_count(&self, node_id: &str) -> usize {
        self.edges.iter().filter(|edge| edge.to == node_id).count()
    }

    pub fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            return Err(anyhow!("effective execution graph has no nodes"));
        }
        if let Some(entry) = self.entry.as_deref() {
            if !self.nodes.contains_key(entry) {
                return Err(anyhow!(
                    "effective execution graph entry '{}' is missing",
                    entry
                ));
            }
        }
        let mut in_degree: HashMap<&str, usize> = self
            .nodes
            .keys()
            .map(|node_id| (node_id.as_str(), 0usize))
            .collect();
        for edge in &self.edges {
            let Some(degree) = in_degree.get_mut(edge.to.as_str()) else {
                return Err(anyhow!("graph edge target '{}' is missing", edge.to));
            };
            *degree += 1;
        }
        let mut ready: Vec<&str> = in_degree
            .iter()
            .filter_map(|(node_id, degree)| (*degree == 0).then_some(*node_id))
            .collect();
        let mut visited = 0usize;
        while let Some(node_id) = ready.pop() {
            visited += 1;
            for edge in self.outgoing_edges(node_id) {
                let degree = in_degree
                    .get_mut(edge.to.as_str())
                    .ok_or_else(|| anyhow!("graph edge target '{}' is missing", edge.to))?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push(edge.to.as_str());
                }
            }
        }
        if visited != self.nodes.len() {
            return Err(anyhow!("effective execution graph contains a cycle"));
        }
        Ok(())
    }
}

fn static_step_node(step: &TaskExecutionStep) -> Option<ExecutionGraphNode> {
    if !step.enabled || step.is_guard || step.id == "init_once" {
        return None;
    }
    Some(ExecutionGraphNode {
        id: step.id.clone(),
        scope: step.resolved_scope(),
        repeatable: step.repeatable,
        is_guard: step.is_guard,
        prehook: step.prehook.clone(),
        spec: ExecutionGraphNodeSpec::StaticStep {
            step_id: step.id.clone(),
        },
    })
}

pub fn build_static_execution_graph(
    task_ctx: &TaskRuntimeContext,
) -> Result<EffectiveExecutionGraph> {
    let mut graph = EffectiveExecutionGraph {
        source: ExecutionGraphSource::StaticBaseline,
        ..EffectiveExecutionGraph::default()
    };
    let mut previous: Option<String> = None;
    for step in &task_ctx.execution_plan.steps {
        let Some(node) = static_step_node(step) else {
            continue;
        };
        let node_id = node.id.clone();
        graph.add_node(node)?;
        if graph.entry.is_none() {
            graph.entry = Some(node_id.clone());
        }
        if let Some(prev) = previous.as_ref() {
            graph.add_edge(ExecutionGraphEdge {
                from: prev.clone(),
                to: node_id.clone(),
                condition: None,
            })?;
        }
        previous = Some(node_id);
    }
    graph.validate()?;
    Ok(graph)
}

pub fn build_adaptive_execution_graph(
    plan: &super::DynamicExecutionPlan,
    source: ExecutionGraphSource,
) -> Result<EffectiveExecutionGraph> {
    let mut graph = EffectiveExecutionGraph {
        source,
        entry: plan.entry.clone(),
        ..EffectiveExecutionGraph::default()
    };
    for node in plan.nodes.values() {
        graph.add_node(ExecutionGraphNode {
            id: node.id.clone(),
            scope: crate::config::default_scope_for_step_id(&node.step_type),
            repeatable: node.repeatable,
            is_guard: node.is_guard,
            prehook: node.prehook.as_ref().map(|prehook| StepPrehookConfig {
                engine: crate::config::StepHookEngine::Cel,
                when: prehook.when.clone(),
                reason: prehook.reason.clone(),
                ui: None,
                extended: prehook.extended,
            }),
            spec: ExecutionGraphNodeSpec::DynamicStep {
                step_type: node.step_type.clone(),
                agent_id: node.agent_id.clone(),
                template: node.template.clone(),
            },
        })?;
    }
    for edge in &plan.edges {
        graph.add_edge(ExecutionGraphEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            condition: edge.condition.clone(),
        })?;
    }
    graph.validate()?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_static_execution_graph_skips_init_and_guard() {
        let task_ctx = TaskRuntimeContext {
            workspace_id: "ws".to_string(),
            workspace_root: "/tmp".into(),
            ticket_dir: "tickets".to_string(),
            execution_plan: std::sync::Arc::new(crate::config::TaskExecutionPlan {
                steps: vec![
                    crate::config::TaskExecutionStep {
                        id: "init_once".to_string(),
                        required_capability: None,
                        template: None,
                        execution_profile: None,
                        builtin: Some("init_once".to_string()),
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: Default::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    crate::config::TaskExecutionStep {
                        id: "plan".to_string(),
                        required_capability: Some("plan".to_string()),
                        template: None,
                        execution_profile: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: Default::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    crate::config::TaskExecutionStep {
                        id: "qa".to_string(),
                        required_capability: Some("qa".to_string()),
                        template: None,
                        execution_profile: None,
                        builtin: None,
                        enabled: true,
                        repeatable: true,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: Default::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                    crate::config::TaskExecutionStep {
                        id: "loop_guard".to_string(),
                        required_capability: None,
                        template: None,
                        execution_profile: None,
                        builtin: Some("loop_guard".to_string()),
                        enabled: true,
                        repeatable: true,
                        is_guard: true,
                        cost_preference: None,
                        prehook: None,
                        tty: false,
                        outputs: vec![],
                        pipe_to: None,
                        command: None,
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: Default::default(),
                        max_parallel: None,
                        timeout_secs: None,
                        item_select_config: None,
                        store_inputs: vec![],
                        store_outputs: vec![],
                    },
                ],
                loop_policy: Default::default(),
                finalize: Default::default(),
                max_parallel: None,
            }),
            execution: Default::default(),
            current_cycle: 1,
            init_done: true,
            dynamic_steps: std::sync::Arc::new(vec![]),
            adaptive: std::sync::Arc::new(None),
            pipeline_vars: Default::default(),
            safety: std::sync::Arc::new(Default::default()),
            self_referential: false,
            consecutive_failures: 0,
            project_id: "default".to_string(),
            pinned_invariants: std::sync::Arc::new(vec![]),
            workflow_id: "wf".to_string(),
            spawn_depth: 0,
        };

        let graph = build_static_execution_graph(&task_ctx).expect("graph");
        assert_eq!(graph.entry.as_deref(), Some("plan"));
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.contains_key("plan"));
        assert!(graph.nodes.contains_key("qa"));
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].from, "plan");
        assert_eq!(graph.edges[0].to, "qa");
    }

    #[test]
    fn build_adaptive_execution_graph_preserves_conditions() {
        let mut plan = super::super::DynamicExecutionPlan {
            entry: Some("qa".to_string()),
            ..Default::default()
        };
        plan.add_node(super::super::WorkflowNode {
            id: "qa".to_string(),
            step_type: "qa".to_string(),
            agent_id: None,
            template: None,
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .expect("add qa node");
        plan.add_node(super::super::WorkflowNode {
            id: "fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: Some("fixer".to_string()),
            template: Some("fix {rel_path}".to_string()),
            prehook: None,
            is_guard: false,
            repeatable: true,
        })
        .expect("add fix node");
        plan.add_edge(super::super::WorkflowEdge {
            from: "qa".to_string(),
            to: "fix".to_string(),
            condition: Some("active_ticket_count > 0".to_string()),
        })
        .expect("add edge");

        let graph = build_adaptive_execution_graph(&plan, ExecutionGraphSource::AdaptivePlanner)
            .expect("graph");
        assert_eq!(graph.source, ExecutionGraphSource::AdaptivePlanner);
        assert_eq!(graph.entry.as_deref(), Some("qa"));
        assert_eq!(
            graph.edges[0].condition.as_deref(),
            Some("active_ticket_count > 0")
        );
    }
}
