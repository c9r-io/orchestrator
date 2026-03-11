use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::config::{DagFallbackMode, StepScope, TaskRuntimeContext};
use crate::dynamic_orchestration::{
    build_adaptive_execution_graph, build_static_execution_graph, AdaptiveFailureClass,
    AdaptivePlanExecutor, AdaptivePlanSource, AdaptivePlanner, EffectiveExecutionGraph,
    ExecutionGraphNode, ExecutionGraphNodeSpec, ExecutionGraphSource,
};
use crate::events::insert_event;
use crate::scheduler::item_executor::{
    execute_dynamic_step_config, process_item_filtered, ProcessItemRequest, StepExecutionAccumulator,
};
use crate::scheduler::phase_runner::{run_phase_with_selected_agent, SelectedPhaseRunRequest};
use crate::scheduler::task_state::{is_task_paused_in_db, list_task_items_for_cycle, set_task_status};
use crate::scheduler::RunningTask;
use crate::state::InnerState;

use super::{cycle_safety, segment};

pub(super) async fn execute_cycle_graph(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<GraphCycleOutcome> {
    cycle_safety::create_cycle_checkpoint(state, task_id, task_ctx).await?;
    if let Some(action) = cycle_safety::check_invariants(
        state,
        task_id,
        task_ctx,
        crate::config::InvariantCheckPoint::BeforeCycle,
    )
    .await?
    {
        return match action {
            "halt" => {
                set_task_status(state, task_id, "failed", false).await?;
                Err(anyhow!("invariant halt at before_cycle checkpoint"))
            }
            _ => Ok(GraphCycleOutcome::RestartCycle),
        };
    }

    let mut items = list_task_items_for_cycle(state, task_id).await?;
    let mut task_item_paths: Vec<String> = items.iter().map(|item| item.qa_file_path.clone()).collect();
    let mut item_state: HashMap<String, StepExecutionAccumulator> = HashMap::new();

    let graph = match materialize_graph(state, task_id, task_ctx, runtime, &items).await {
        Ok(graph) => graph,
        Err(err) => match task_ctx.execution.fallback_mode {
            DagFallbackMode::StaticSegment => {
                insert_event(
                    state,
                    task_id,
                    None,
                    "dynamic_plan_failed",
                    json!({"mode":"dynamic_dag","fallback_mode":"static_segment","error":err.to_string()}),
                )
                .await?;
                return Ok(GraphCycleOutcome::FallbackToStaticSegment);
            }
            DagFallbackMode::FailClosed => return Err(err),
            DagFallbackMode::DeterministicDag => {
                insert_event(
                    state,
                    task_id,
                    None,
                    "dynamic_plan_failed",
                    json!({"mode":"dynamic_dag","fallback_mode":"deterministic_dag","error":err.to_string()}),
                )
                .await?;
                build_static_execution_graph(task_ctx)?
            }
        },
    };

    insert_event(
        state,
        task_id,
        None,
        "dynamic_plan_materialized",
        json!({
            "mode":"dynamic_dag",
            "source": graph.source,
            "node_count": graph.nodes.len(),
            "edge_count": graph.edges.len(),
            "graph": graph,
        }),
    )
    .await?;

    execute_graph_nodes(
        state,
        task_id,
        task_ctx,
        runtime,
        &graph,
        &mut items,
        &mut item_state,
        &mut task_item_paths,
    )
    .await?;

    segment::finalize_items(state, task_id, task_ctx, &items, &mut item_state).await?;
    Ok(GraphCycleOutcome::Completed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GraphCycleOutcome {
    Completed,
    RestartCycle,
    FallbackToStaticSegment,
}

async fn materialize_graph(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    items: &[crate::dto::TaskItemRow],
) -> Result<EffectiveExecutionGraph> {
    let Some(anchor_item) = items.first() else {
        return build_static_execution_graph(task_ctx);
    };

    if let Some(adaptive_config) = task_ctx.adaptive.clone().filter(|cfg| cfg.enabled) {
        insert_event(
            state,
            task_id,
            Some(anchor_item.id.as_str()),
            "dynamic_plan_generated",
            json!({
                "planner_agent": adaptive_config.planner_agent,
                "cycle": task_ctx.current_cycle,
                "fallback_mode": adaptive_config.fallback_mode,
            }),
        )
        .await?;
        let planner_ctx = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
            .to_prehook_context(task_id, anchor_item, task_ctx, "adaptive_plan");
        let planner = AdaptivePlanner::new(adaptive_config.clone());
        let executor = GraphAdaptiveExecutor {
            state,
            task_id,
            item: anchor_item,
            task_ctx,
            runtime,
        };
        let outcome = planner.generate_plan(&executor, &planner_ctx).await?;
        let source = match outcome.metadata.source {
            AdaptivePlanSource::Planner => ExecutionGraphSource::AdaptivePlanner,
            AdaptivePlanSource::DeterministicFallback => ExecutionGraphSource::DeterministicFallback,
        };
        let graph = build_adaptive_execution_graph(&outcome.plan, source)?;
        insert_event(
            state,
            task_id,
            Some(anchor_item.id.as_str()),
            "dynamic_plan_validated",
            json!({
                "source": source,
                "used_fallback": outcome.metadata.used_fallback,
                "error_class": outcome.metadata.error_class.map(adaptive_failure_class_name),
                "node_count": graph.nodes.len(),
                "edge_count": graph.edges.len(),
            }),
        )
        .await?;
        return Ok(graph);
    }

    let graph = build_static_execution_graph(task_ctx)?;
    insert_event(
        state,
        task_id,
        Some(anchor_item.id.as_str()),
        "dynamic_plan_validated",
        json!({
            "source": "static_baseline",
            "used_fallback": false,
            "node_count": graph.nodes.len(),
            "edge_count": graph.edges.len(),
        }),
    )
    .await?;
    Ok(graph)
}

fn adaptive_failure_class_name(class: AdaptiveFailureClass) -> &'static str {
    match class {
        AdaptiveFailureClass::Disabled => "disabled",
        AdaptiveFailureClass::Misconfigured => "misconfigured",
        AdaptiveFailureClass::ExecutorFailure => "executor_failure",
        AdaptiveFailureClass::InvalidJson => "invalid_json",
        AdaptiveFailureClass::InvalidPlan => "invalid_plan",
    }
}

struct GraphAdaptiveExecutor<'a> {
    state: &'a Arc<InnerState>,
    task_id: &'a str,
    item: &'a crate::dto::TaskItemRow,
    task_ctx: &'a TaskRuntimeContext,
    runtime: &'a RunningTask,
}

#[async_trait::async_trait]
impl AdaptivePlanExecutor for GraphAdaptiveExecutor<'_> {
    async fn execute(
        &self,
        prompt: &str,
        config: &crate::dynamic_orchestration::AdaptivePlannerConfig,
    ) -> Result<String> {
        let planner_agent = config
            .planner_agent
            .as_deref()
            .ok_or_else(|| anyhow!("adaptive planner agent is missing"))?;
        let (command, prompt_delivery) = {
            let active = crate::config_load::read_active_config(self.state)?;
            let agent = crate::selection::resolve_agent_by_id(
                &self.task_ctx.project_id,
                &active.config,
                planner_agent,
            )
            .ok_or_else(|| anyhow!("adaptive planner agent not found: {}", planner_agent))?;
            (agent.command.clone(), agent.prompt_delivery)
        };
        let result = run_phase_with_selected_agent(
            self.state,
            SelectedPhaseRunRequest {
                task_id: self.task_id,
                item_id: self.item.id.as_str(),
                step_id: "adaptive_plan",
                phase: "adaptive_plan",
                tty: false,
                agent_id: planner_agent,
                command_template: &command,
                prompt_delivery,
                rel_path: &self.item.qa_file_path,
                ticket_paths: &[],
                workspace_root: &self.task_ctx.workspace_root,
                workspace_id: &self.task_ctx.workspace_id,
                cycle: self.task_ctx.current_cycle,
                runtime: self.runtime,
                pipeline_vars: None,
                step_timeout_secs: self.task_ctx.safety.step_timeout_secs,
                step_scope: StepScope::Item,
                step_template_prompt: Some(prompt),
                project_id: &self.task_ctx.project_id,
                execution_profile: None,
            },
        )
        .await?;
        let output = result
            .output
            .ok_or_else(|| anyhow!("adaptive planner produced no structured output"))?;
        Ok(output.stdout)
    }
}

async fn execute_graph_nodes(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
    runtime: &RunningTask,
    graph: &EffectiveExecutionGraph,
    items: &mut Vec<crate::dto::TaskItemRow>,
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<()> {
    let mut resolved_incoming: HashMap<String, usize> = graph
        .nodes
        .keys()
        .map(|node_id| (node_id.clone(), 0usize))
        .collect();
    let incoming_total: HashMap<String, usize> = graph
        .nodes
        .keys()
        .map(|node_id| (node_id.clone(), graph.incoming_count(node_id)))
        .collect();
    let mut has_taken_incoming: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = graph
        .nodes
        .keys()
        .filter(|node_id| incoming_total.get(*node_id).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();
    if let Some(entry) = graph.entry.as_ref() {
        queue.retain(|node_id| node_id == entry);
        queue.push_front(entry.clone());
    }
    let mut executed: HashSet<String> = HashSet::new();
    let mut injected_dynamic_ids: HashSet<String> = HashSet::new();

    while let Some(node_id) = queue.pop_front() {
        if executed.contains(&node_id) {
            continue;
        }
        if runtime.stop_flag.load(Ordering::SeqCst) || is_task_paused_in_db(state, task_id).await? {
            return Ok(());
        }
        let node = graph
            .get_node(&node_id)
            .ok_or_else(|| anyhow!("graph node '{}' disappeared", node_id))?;
        insert_event(
            state,
            task_id,
            None,
            "dynamic_node_ready",
            json!({"node_id": node.id, "scope": node.scope}),
        )
        .await?;
        if should_skip_node(task_id, task_ctx, items, item_state, node)? {
            executed.insert(node_id.clone());
            insert_event(
                state,
                task_id,
                None,
                "dynamic_node_skipped",
                json!({"node_id": node.id, "reason": "prehook_false"}),
            )
            .await?;
        } else {
            insert_event(
                state,
                task_id,
                None,
                "dynamic_node_started",
                json!({"node_id": node.id, "scope": node.scope}),
            )
            .await?;
            execute_graph_node(
                state,
                task_id,
                task_ctx,
                runtime,
                node,
                items,
                item_state,
                task_item_paths,
            )
            .await?;
            insert_event(
                state,
                task_id,
                None,
                "dynamic_node_finished",
                json!({"node_id": node.id, "scope": node.scope, "success": true}),
            )
            .await?;
            executed.insert(node_id.clone());
            inject_dynamic_pool_steps(
                state,
                task_id,
                task_ctx,
                runtime,
                node,
                items,
                item_state,
                task_item_paths,
                &mut injected_dynamic_ids,
            )
            .await?;
        }

        for edge in graph.outgoing_edges(&node_id) {
            let taken = evaluate_edge(task_id, task_ctx, items, item_state, edge.condition.as_deref())?;
            insert_event(
                state,
                task_id,
                None,
                "dynamic_edge_evaluated",
                json!({"from": edge.from, "to": edge.to, "condition": edge.condition, "taken": taken}),
            )
            .await?;
            *resolved_incoming.entry(edge.to.clone()).or_insert(0) += 1;
            if taken {
                has_taken_incoming.insert(edge.to.clone());
                insert_event(
                    state,
                    task_id,
                    None,
                    "dynamic_edge_taken",
                    json!({"from": edge.from, "to": edge.to}),
                )
                .await?;
            }
            let total = incoming_total.get(&edge.to).copied().unwrap_or(0);
            let resolved = resolved_incoming.get(&edge.to).copied().unwrap_or(0);
            if resolved >= total
                && (has_taken_incoming.contains(&edge.to) || total == 0)
                && !executed.contains(&edge.to)
            {
                queue.push_back(edge.to.clone());
            }
        }
    }

    Ok(())
}

fn should_skip_node(
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
    items: &[crate::dto::TaskItemRow],
    item_state: &HashMap<String, StepExecutionAccumulator>,
    node: &ExecutionGraphNode,
) -> Result<bool> {
    let Some(prehook) = node.prehook.as_ref() else {
        return Ok(false);
    };
    let Some(anchor_item) = items.first() else {
        return Ok(false);
    };
    let prehook_ctx = match item_state.get(&anchor_item.id) {
        Some(acc) => acc.to_prehook_context(task_id, anchor_item, task_ctx, &node.id),
        None => StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
            .to_prehook_context(task_id, anchor_item, task_ctx, &node.id),
    };
    Ok(!crate::prehook::evaluate_step_prehook_expression(&prehook.when, &prehook_ctx)?)
}

fn evaluate_edge(
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
    items: &[crate::dto::TaskItemRow],
    item_state: &HashMap<String, StepExecutionAccumulator>,
    condition: Option<&str>,
) -> Result<bool> {
    let Some(condition) = condition else {
        return Ok(true);
    };
    let Some(anchor_item) = items.first() else {
        return Ok(false);
    };
    let ctx = match item_state.get(&anchor_item.id) {
        Some(acc) => acc.to_prehook_context(task_id, anchor_item, task_ctx, "dynamic_edge"),
        None => StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
            .to_prehook_context(task_id, anchor_item, task_ctx, "dynamic_edge"),
    };
    crate::prehook::evaluate_step_prehook_expression(condition, &ctx)
}

async fn execute_graph_node(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
    runtime: &RunningTask,
    node: &ExecutionGraphNode,
    items: &mut Vec<crate::dto::TaskItemRow>,
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<()> {
    match &node.spec {
        ExecutionGraphNodeSpec::StaticStep { step_id } => {
            let mut step_ids = HashSet::new();
            step_ids.insert(step_id.clone());
            match node.scope {
                StepScope::Task => {
                    let Some(anchor_item) = items.first() else {
                        return Ok(());
                    };
                    let mut task_acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
                    process_item_filtered(
                        state,
                        ProcessItemRequest {
                            task_id,
                            item: anchor_item,
                            task_item_paths,
                            task_ctx,
                            runtime,
                            step_filter: Some(&step_ids),
                            run_dynamic_steps: false,
                        },
                        &mut task_acc,
                    )
                    .await?;
                    task_ctx.pipeline_vars = task_acc.pipeline_vars.clone();
                    for item in items.iter() {
                        let acc = item_state
                            .entry(item.id.clone())
                            .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
                        acc.merge_task_pipeline_vars(&task_acc.pipeline_vars);
                    }
                }
                StepScope::Item => {
                    for item in items.iter() {
                        let acc = item_state
                            .entry(item.id.clone())
                            .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
                        process_item_filtered(
                            state,
                            ProcessItemRequest {
                                task_id,
                                item,
                                task_item_paths,
                                task_ctx,
                                runtime,
                                step_filter: Some(&step_ids),
                                run_dynamic_steps: false,
                            },
                            acc,
                        )
                        .await?;
                    }
                }
            }
        }
        ExecutionGraphNodeSpec::DynamicStep {
            step_type,
            agent_id,
            template,
        } => {
            let dyn_step = crate::dynamic_orchestration::DynamicStepConfig {
                id: node.id.clone(),
                description: None,
                step_type: step_type.clone(),
                agent_id: agent_id.clone(),
                template: template.clone(),
                trigger: node.prehook.as_ref().map(|prehook| prehook.when.clone()),
                priority: 0,
                max_runs: None,
            };
            for item in items.iter() {
                let acc = item_state
                    .entry(item.id.clone())
                    .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
                execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, &dyn_step)
                    .await?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn inject_dynamic_pool_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    node: &ExecutionGraphNode,
    items: &[crate::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &[String],
    injected_dynamic_ids: &mut HashSet<String>,
) -> Result<()> {
    if task_ctx.dynamic_steps.is_empty() {
        return Ok(());
    }
    for item in items {
        let acc = item_state
            .entry(item.id.clone())
            .or_insert_with(|| StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone()));
        let ctx = acc.to_prehook_context(task_id, item, task_ctx, &node.id);
        let mut pool = crate::dynamic_orchestration::DynamicStepPool::new();
        for dynamic_step in &task_ctx.dynamic_steps {
            pool.add_step(dynamic_step.clone());
        }
        for dynamic_step in pool.find_matching_steps(&ctx) {
            let injection_key = format!("{}:{}", item.id, dynamic_step.id);
            if !injected_dynamic_ids.insert(injection_key) {
                continue;
            }
            insert_event(
                state,
                task_id,
                Some(item.id.as_str()),
                "dynamic_steps_injected",
                json!({"source_node_id": node.id, "dynamic_step_id": dynamic_step.id}),
            )
            .await?;
            execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, dynamic_step)
                .await?;
        }
        let _ = task_item_paths;
    }
    Ok(())
}
