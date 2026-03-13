use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::json;
use uuid::Uuid;

use crate::config::{DagFallbackMode, StepScope, TaskRuntimeContext};
use crate::dynamic_orchestration::{
    build_adaptive_execution_graph, build_static_execution_graph, AdaptiveFailureClass,
    AdaptivePlanExecutor, AdaptivePlanSource, AdaptivePlanner, EffectiveExecutionGraph,
    ExecutionGraphNode, ExecutionGraphNodeSpec, ExecutionGraphSource,
};
use crate::events::insert_event;
use crate::scheduler::item_executor::{
    execute_dynamic_step_config, process_item_filtered, ProcessItemRequest,
    StepExecutionAccumulator,
};
use crate::scheduler::phase_runner::{run_phase_with_selected_agent, SelectedPhaseRunRequest};
use crate::scheduler::task_state::{
    is_task_paused_in_db, list_task_items_for_cycle, set_task_status,
};
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
    let mut task_item_paths: Vec<String> =
        items.iter().map(|item| item.qa_file_path.clone()).collect();
    let mut item_state: HashMap<String, StepExecutionAccumulator> = HashMap::new();

    let materialized = match materialize_graph(state, task_id, task_ctx, runtime, &items).await {
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
                let materialized = GraphMaterialization {
                    graph: build_static_execution_graph(task_ctx)?,
                    graph_run_id: Uuid::new_v4().to_string(),
                    source: "deterministic_fallback".to_string(),
                    cycle: task_ctx.current_cycle as i64,
                    fallback_mode: Some("deterministic_dag".to_string()),
                    planner_failure_class: None,
                    planner_failure_message: Some(err.to_string()),
                    planner_raw_output_json: None,
                    normalized_plan_json: None,
                };
                insert_graph_run(state, task_id, &materialized).await?;
                materialized
            }
        },
    };
    let graph = &materialized.graph;

    emit_graph_event(
        state,
        task_id,
        None,
        "dynamic_plan_materialized",
        &materialized,
        json!({
            "node_count": graph.nodes.len(),
            "edge_count": graph.edges.len(),
            "graph": graph,
        }),
    )
    .await?;
    persist_graph_snapshot(
        state,
        task_id,
        &materialized.graph_run_id,
        "effective_graph",
        graph,
    )
    .await?;
    state
        .task_repo
        .update_task_graph_run_status(&materialized.graph_run_id, "running")
        .await?;

    let execution_replay = execute_graph_nodes(
        state,
        task_id,
        task_ctx,
        runtime,
        graph,
        &materialized,
        &mut items,
        &mut item_state,
        &mut task_item_paths,
    )
    .await;

    match execution_replay {
        Ok(replay) => {
            if task_ctx.execution.persist_graph_snapshots {
                persist_graph_snapshot(
                    state,
                    task_id,
                    &materialized.graph_run_id,
                    "execution_replay",
                    &replay,
                )
                .await?;
            }
            state
                .task_repo
                .update_task_graph_run_status(&materialized.graph_run_id, "completed")
                .await?;
        }
        Err(err) => {
            state
                .task_repo
                .update_task_graph_run_status(&materialized.graph_run_id, "failed")
                .await?;
            return Err(err);
        }
    }

    segment::finalize_items(state, task_id, task_ctx, &items, &mut item_state).await?;
    Ok(GraphCycleOutcome::Completed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GraphCycleOutcome {
    Completed,
    RestartCycle,
    FallbackToStaticSegment,
}

#[derive(Debug, Clone)]
struct GraphMaterialization {
    graph: EffectiveExecutionGraph,
    graph_run_id: String,
    source: String,
    cycle: i64,
    fallback_mode: Option<String>,
    planner_failure_class: Option<String>,
    planner_failure_message: Option<String>,
    planner_raw_output_json: Option<String>,
    normalized_plan_json: Option<String>,
}

impl GraphMaterialization {
    fn graph_run_id(&self) -> &str {
        &self.graph_run_id
    }

    fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct NodeExecutionRecord {
    node_id: String,
    success: bool,
    skipped: bool,
    reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct EdgeDecisionRecord {
    from: String,
    to: String,
    condition: Option<String>,
    taken: bool,
    reason: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct GraphExecutionReplay {
    node_execution_order: Vec<String>,
    node_results: Vec<NodeExecutionRecord>,
    edge_decisions: Vec<EdgeDecisionRecord>,
}

async fn materialize_graph(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    items: &[crate::dto::TaskItemRow],
) -> Result<GraphMaterialization> {
    let graph_run_id = Uuid::new_v4().to_string();
    let cycle = task_ctx.current_cycle as i64;
    let Some(anchor_item) = items.first() else {
        let graph = build_static_execution_graph(task_ctx)?;
        let materialized = GraphMaterialization {
            graph,
            graph_run_id,
            source: "static_baseline".to_string(),
            cycle,
            fallback_mode: None,
            planner_failure_class: None,
            planner_failure_message: None,
            planner_raw_output_json: None,
            normalized_plan_json: None,
        };
        insert_graph_run(state, task_id, &materialized).await?;
        return Ok(materialized);
    };

    if let Some(adaptive_config) = task_ctx.adaptive_config().filter(|cfg| cfg.enabled) {
        let fallback_mode = dag_fallback_mode_name(task_ctx.execution.fallback_mode).to_string();
        let generation_ctx = GraphMaterialization {
            graph: EffectiveExecutionGraph::default(),
            graph_run_id: graph_run_id.clone(),
            source: "adaptive_planner".to_string(),
            cycle,
            fallback_mode: Some(fallback_mode.clone()),
            planner_failure_class: None,
            planner_failure_message: None,
            planner_raw_output_json: None,
            normalized_plan_json: None,
        };
        emit_graph_event(
            state,
            task_id,
            Some(anchor_item.id.as_str()),
            "dynamic_plan_generated",
            &generation_ctx,
            json!({
                "planner_agent": adaptive_config.planner_agent,
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
            AdaptivePlanSource::DeterministicFallback => {
                ExecutionGraphSource::DeterministicFallback
            }
        };
        let graph = build_adaptive_execution_graph(&outcome.plan, source)?;
        let materialized = GraphMaterialization {
            graph,
            graph_run_id,
            source: execution_graph_source_name(source).to_string(),
            cycle,
            fallback_mode: Some(fallback_mode),
            planner_failure_class: outcome
                .metadata
                .error_class
                .map(adaptive_failure_class_name)
                .map(ToString::to_string),
            planner_failure_message: outcome.metadata.error_message.clone(),
            planner_raw_output_json: outcome.raw_output.clone(),
            normalized_plan_json: Some(
                serde_json::to_string(&outcome.plan)
                    .map_err(|err| anyhow!("serialize normalized plan: {err}"))?,
            ),
        };
        insert_graph_run(state, task_id, &materialized).await?;
        if task_ctx.execution.persist_graph_snapshots {
            if let Some(raw_output) = materialized.planner_raw_output_json.as_ref() {
                persist_graph_snapshot_payload(
                    state,
                    task_id,
                    materialized.graph_run_id(),
                    "planner_raw_output",
                    raw_output.clone(),
                )
                .await?;
            }
            if let Some(normalized_plan_json) = materialized.normalized_plan_json.as_ref() {
                persist_graph_snapshot_payload(
                    state,
                    task_id,
                    materialized.graph_run_id(),
                    "normalized_plan",
                    normalized_plan_json.clone(),
                )
                .await?;
            }
        }
        emit_graph_event(
            state,
            task_id,
            Some(anchor_item.id.as_str()),
            "dynamic_plan_validated",
            &materialized,
            json!({
                "used_fallback": outcome.metadata.used_fallback,
                "error_class": outcome.metadata.error_class.map(adaptive_failure_class_name),
                "node_count": materialized.graph.nodes.len(),
                "edge_count": materialized.graph.edges.len(),
            }),
        )
        .await?;
        return Ok(materialized);
    }

    let graph = build_static_execution_graph(task_ctx)?;
    let materialized = GraphMaterialization {
        graph,
        graph_run_id,
        source: "static_baseline".to_string(),
        cycle,
        fallback_mode: None,
        planner_failure_class: None,
        planner_failure_message: None,
        planner_raw_output_json: None,
        normalized_plan_json: None,
    };
    insert_graph_run(state, task_id, &materialized).await?;
    emit_graph_event(
        state,
        task_id,
        Some(anchor_item.id.as_str()),
        "dynamic_plan_validated",
        &materialized,
        json!({
            "used_fallback": false,
            "node_count": materialized.graph.nodes.len(),
            "edge_count": materialized.graph.edges.len(),
        }),
    )
    .await?;
    Ok(materialized)
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
                self_referential: self.task_ctx.self_referential,
            },
        )
        .await?;
        let output = result
            .output
            .ok_or_else(|| anyhow!("adaptive planner produced no structured output"))?;
        Ok(output.stdout)
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_graph_nodes(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
    runtime: &RunningTask,
    graph: &EffectiveExecutionGraph,
    graph_run: &GraphMaterialization,
    items: &mut [crate::dto::TaskItemRow],
    item_state: &mut HashMap<String, StepExecutionAccumulator>,
    task_item_paths: &mut Vec<String>,
) -> Result<GraphExecutionReplay> {
    let mut replay = GraphExecutionReplay::default();
    let mut resolved_incoming: HashMap<&str, usize> = graph
        .nodes
        .keys()
        .map(|node_id| (node_id.as_str(), 0usize))
        .collect();
    let incoming_total: HashMap<&str, usize> = graph
        .nodes
        .keys()
        .map(|node_id| (node_id.as_str(), graph.incoming_count(node_id)))
        .collect();
    let mut has_taken_incoming: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = graph
        .nodes
        .keys()
        .map(|node_id| node_id.as_str())
        .filter(|node_id| incoming_total.get(*node_id).copied().unwrap_or(0) == 0)
        .collect();
    if let Some(entry) = graph.entry.as_ref() {
        let entry = entry.as_str();
        queue.retain(|node_id| *node_id == entry);
        queue.push_front(entry);
    }
    let mut executed: HashSet<&str> = HashSet::new();
    let mut injected_dynamic_ids: HashSet<String> = HashSet::new();

    while let Some(node_id) = queue.pop_front() {
        if executed.contains(node_id) {
            continue;
        }
        if runtime.stop_flag.load(Ordering::SeqCst) || is_task_paused_in_db(state, task_id).await? {
            return Ok(replay);
        }
        let node = graph
            .get_node(node_id)
            .ok_or_else(|| anyhow!("graph node '{}' disappeared", node_id))?;
        emit_graph_event(
            state,
            task_id,
            None,
            "dynamic_node_ready",
            graph_run,
            json!({"node_id": node.id, "scope": node.scope}),
        )
        .await?;
        if should_skip_node(task_id, task_ctx, items, item_state, node)? {
            executed.insert(node_id);
            replay.node_execution_order.push(node.id.clone());
            replay.node_results.push(NodeExecutionRecord {
                node_id: node.id.clone(),
                success: true,
                skipped: true,
                reason: Some("prehook_false".to_string()),
            });
            emit_graph_event(
                state,
                task_id,
                None,
                "dynamic_node_skipped",
                graph_run,
                json!({"node_id": node.id, "reason": "prehook_false"}),
            )
            .await?;
        } else {
            emit_graph_event(
                state,
                task_id,
                None,
                "dynamic_node_started",
                graph_run,
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
            replay.node_execution_order.push(node.id.clone());
            replay.node_results.push(NodeExecutionRecord {
                node_id: node.id.clone(),
                success: true,
                skipped: false,
                reason: None,
            });
            emit_graph_event(
                state,
                task_id,
                None,
                "dynamic_node_finished",
                graph_run,
                json!({"node_id": node.id, "scope": node.scope, "success": true}),
            )
            .await?;
            executed.insert(node_id);
            inject_dynamic_pool_steps(
                state,
                task_id,
                task_ctx,
                runtime,
                graph_run,
                node,
                items,
                item_state,
                task_item_paths,
                &mut injected_dynamic_ids,
            )
            .await?;
        }

        for edge in graph.outgoing_edges(node_id) {
            let to_node = edge.to.as_str();
            let taken = evaluate_edge(
                task_id,
                task_ctx,
                items,
                item_state,
                edge.condition.as_deref(),
            )?;
            let reason = match edge.condition.as_deref() {
                None => "unconditional".to_string(),
                Some(_) if taken => "cel_true".to_string(),
                Some(_) => "cel_false".to_string(),
            };
            replay.edge_decisions.push(EdgeDecisionRecord {
                from: edge.from.clone(),
                to: to_node.to_string(),
                condition: edge.condition.clone(),
                taken,
                reason: reason.clone(),
            });
            emit_graph_event(
                state,
                task_id,
                None,
                "dynamic_edge_evaluated",
                graph_run,
                json!({"from": edge.from, "to": edge.to, "condition": edge.condition, "taken": taken, "reason": reason}),
            )
            .await?;
            *resolved_incoming.entry(to_node).or_insert(0) += 1;
            if taken {
                has_taken_incoming.insert(to_node);
                emit_graph_event(
                    state,
                    task_id,
                    None,
                    "dynamic_edge_taken",
                    graph_run,
                    json!({"from": edge.from, "to": edge.to}),
                )
                .await?;
            }
            let total = incoming_total.get(to_node).copied().unwrap_or(0);
            let resolved = resolved_incoming.get(to_node).copied().unwrap_or(0);
            if resolved >= total
                && (has_taken_incoming.contains(to_node) || total == 0)
                && !executed.contains(to_node)
            {
                queue.push_back(to_node);
            }
        }
    }

    Ok(replay)
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
    let prehook_ctx =
        match item_state.get(&anchor_item.id) {
            Some(acc) => acc.to_prehook_context(task_id, anchor_item, task_ctx, &node.id),
            None => StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                .to_prehook_context(task_id, anchor_item, task_ctx, &node.id),
        };
    Ok(!crate::prehook::evaluate_step_prehook_expression(
        &prehook.when,
        &prehook_ctx,
    )?)
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
    let ctx =
        match item_state.get(&anchor_item.id) {
            Some(acc) => acc.to_prehook_context(task_id, anchor_item, task_ctx, "dynamic_edge"),
            None => StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                .to_prehook_context(task_id, anchor_item, task_ctx, "dynamic_edge"),
        };
    crate::prehook::evaluate_step_prehook_expression(condition, &ctx)
}

#[allow(clippy::too_many_arguments)]
async fn execute_graph_node(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
    runtime: &RunningTask,
    node: &ExecutionGraphNode,
    items: &mut [crate::dto::TaskItemRow],
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
                    let mut task_acc =
                        StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
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
                        let acc = item_state.entry(item.id.clone()).or_insert_with(|| {
                            StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                        });
                        acc.merge_task_pipeline_vars(&task_acc.pipeline_vars);
                    }
                }
                StepScope::Item => {
                    for item in items.iter() {
                        let acc = item_state.entry(item.id.clone()).or_insert_with(|| {
                            StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                        });
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
                let acc = item_state.entry(item.id.clone()).or_insert_with(|| {
                    StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone())
                });
                execute_dynamic_step_config(
                    state, task_id, item, task_ctx, runtime, acc, &dyn_step,
                )
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
    graph_run: &GraphMaterialization,
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
        for dynamic_step in task_ctx.dynamic_step_configs() {
            pool.add_step(dynamic_step.clone());
        }
        for dynamic_step in pool.find_matching_steps(&ctx) {
            let injection_key = format!("{}:{}", item.id, dynamic_step.id);
            if !injected_dynamic_ids.insert(injection_key) {
                continue;
            }
            emit_graph_event(
                state,
                task_id,
                Some(item.id.as_str()),
                "dynamic_steps_injected",
                graph_run,
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

async fn insert_graph_run(
    state: &Arc<InnerState>,
    task_id: &str,
    materialized: &GraphMaterialization,
) -> Result<()> {
    state
        .task_repo
        .insert_task_graph_run(crate::task_repository::NewTaskGraphRun {
            graph_run_id: materialized.graph_run_id.clone(),
            task_id: task_id.to_string(),
            cycle: materialized.cycle,
            mode: "dynamic_dag".to_string(),
            source: materialized.source().to_string(),
            status: "materialized".to_string(),
            fallback_mode: materialized.fallback_mode.clone(),
            planner_failure_class: materialized.planner_failure_class.clone(),
            planner_failure_message: materialized.planner_failure_message.clone(),
            entry_node_id: materialized.graph.entry.clone(),
            node_count: materialized.graph.nodes.len() as i64,
            edge_count: materialized.graph.edges.len() as i64,
        })
        .await
}

async fn persist_graph_snapshot_payload(
    state: &Arc<InnerState>,
    task_id: &str,
    graph_run_id: &str,
    snapshot_kind: &str,
    payload_json: String,
) -> Result<()> {
    state
        .task_repo
        .insert_task_graph_snapshot(crate::task_repository::NewTaskGraphSnapshot {
            graph_run_id: graph_run_id.to_string(),
            task_id: task_id.to_string(),
            snapshot_kind: snapshot_kind.to_string(),
            payload_json,
        })
        .await
}

async fn persist_graph_snapshot<T: serde::Serialize>(
    state: &Arc<InnerState>,
    task_id: &str,
    graph_run_id: &str,
    snapshot_kind: &str,
    payload: &T,
) -> Result<()> {
    state
        .task_repo
        .insert_task_graph_snapshot(crate::task_repository::NewTaskGraphSnapshot {
            graph_run_id: graph_run_id.to_string(),
            task_id: task_id.to_string(),
            snapshot_kind: snapshot_kind.to_string(),
            payload_json: serde_json::to_string(payload)
                .map_err(|err| anyhow!("serialize {snapshot_kind} snapshot: {err}"))?,
        })
        .await
}

async fn emit_graph_event(
    state: &Arc<InnerState>,
    task_id: &str,
    task_item_id: Option<&str>,
    event_type: &str,
    graph_run: &GraphMaterialization,
    payload: serde_json::Value,
) -> Result<()> {
    let mut payload_obj = payload.as_object().cloned().unwrap_or_default();
    payload_obj.insert("mode".to_string(), json!("dynamic_dag"));
    payload_obj.insert("cycle".to_string(), json!(graph_run.cycle));
    if !graph_run.graph_run_id.is_empty() {
        payload_obj.insert("graph_run_id".to_string(), json!(graph_run.graph_run_id()));
    }
    if !graph_run.source().is_empty() {
        payload_obj.insert("source".to_string(), json!(graph_run.source()));
    }
    insert_event(
        state,
        task_id,
        task_item_id,
        event_type,
        serde_json::Value::Object(payload_obj),
    )
    .await
}

fn execution_graph_source_name(source: ExecutionGraphSource) -> &'static str {
    match source {
        ExecutionGraphSource::StaticBaseline => "static_baseline",
        ExecutionGraphSource::AdaptivePlanner => "adaptive_planner",
        ExecutionGraphSource::DeterministicFallback => "deterministic_fallback",
    }
}

fn dag_fallback_mode_name(mode: DagFallbackMode) -> &'static str {
    match mode {
        DagFallbackMode::DeterministicDag => "deterministic_dag",
        DagFallbackMode::StaticSegment => "static_segment",
        DagFallbackMode::FailClosed => "fail_closed",
    }
}
