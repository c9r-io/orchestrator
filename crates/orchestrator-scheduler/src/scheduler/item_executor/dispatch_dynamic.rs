use agent_orchestrator::config::TaskRuntimeContext;
use agent_orchestrator::dynamic_orchestration::{
    AdaptivePlanExecutor, AdaptivePlanSource, AdaptivePlanner, ExecutionHistoryRecord,
    StepExecutionRecord, StepPrehookContext as DynamicStepContext,
};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::state::InnerState;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::json;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use super::super::RunningTask;
use super::super::phase_runner::{
    RotatingPhaseRunRequest, SelectedPhaseRunRequest, run_phase_with_rotation,
    run_phase_with_selected_agent,
};
use super::accumulator::StepExecutionAccumulator;

/// Execute dynamic steps from the dynamic step pool.
/// Only runs in full-cycle mode (not in segment-filtered mode).
pub(super) async fn execute_dynamic_steps(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    if let Some(adaptive_config) = task_ctx.adaptive_config().filter(|cfg| cfg.enabled) {
        let history = build_adaptive_history(task_id, item.id.as_str(), task_ctx, acc);
        let mut planner = AdaptivePlanner::new(adaptive_config.clone());
        for record in history {
            planner.add_history(record);
        }
        let planner_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, "adaptive_plan");
        insert_event(
            state,
            task_id,
            Some(item.id.as_str()),
            "adaptive_plan_requested",
            json!({
                "planner_agent": adaptive_config.planner_agent,
                "cycle": task_ctx.current_cycle,
                "fallback_mode": adaptive_config.fallback_mode,
            }),
        )
        .await?;

        let executor = AgentBackedAdaptiveExecutor {
            state,
            task_id,
            item_id: item.id.as_str(),
            item,
            task_ctx,
            runtime,
        };
        match planner.generate_plan(&executor, &planner_ctx).await {
            Ok(outcome) => {
                let event_name = match outcome.metadata.source {
                    AdaptivePlanSource::Planner => "adaptive_plan_succeeded",
                    AdaptivePlanSource::DeterministicFallback => "adaptive_plan_fallback_used",
                };
                insert_event(
                    state,
                    task_id,
                    Some(item.id.as_str()),
                    event_name,
                    json!({
                        "planner_agent": adaptive_config.planner_agent,
                        "cycle": task_ctx.current_cycle,
                        "fallback_mode": adaptive_config.fallback_mode,
                        "error_class": outcome.metadata.error_class.map(agent_orchestrator::dynamic_orchestration::adaptive_failure_class_name),
                        "node_count": outcome.plan.nodes.len(),
                        "edge_count": outcome.plan.edges.len(),
                    }),
                )
                .await?;
                return execute_adaptive_plan(
                    state,
                    task_id,
                    item,
                    task_ctx,
                    runtime,
                    acc,
                    &outcome.plan,
                )
                .await;
            }
            Err(err) => {
                insert_event(
                    state,
                    task_id,
                    Some(item.id.as_str()),
                    "adaptive_plan_failed",
                    json!({
                        "planner_agent": adaptive_config.planner_agent,
                        "cycle": task_ctx.current_cycle,
                        "fallback_mode": adaptive_config.fallback_mode,
                        "error": err.to_string(),
                    }),
                )
                .await?;
                return Err(err);
            }
        }
    }

    if task_ctx.dynamic_steps.is_empty() {
        return Ok(());
    }

    let pool = {
        let mut p = agent_orchestrator::dynamic_orchestration::DynamicStepPool::new();
        for ds in task_ctx.dynamic_step_configs() {
            p.add_step(ds.clone());
        }
        p
    };
    let dyn_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, "dynamic");
    let matched: Vec<_> = pool
        .find_matching_steps(&dyn_ctx)
        .into_iter()
        .cloned()
        .collect();
    for ds in &matched {
        execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, ds).await?;
    }

    Ok(())
}

#[async_trait]
impl AdaptivePlanExecutor for AgentBackedAdaptiveExecutor<'_> {
    async fn execute(
        &self,
        prompt: &str,
        config: &agent_orchestrator::dynamic_orchestration::AdaptivePlannerConfig,
    ) -> Result<String> {
        let planner_agent = config
            .planner_agent
            .as_deref()
            .ok_or_else(|| anyhow!("adaptive planner agent is missing"))?;
        let (command, prompt_delivery) = {
            let active = agent_orchestrator::config_load::read_active_config(self.state)?;
            let agent = agent_orchestrator::selection::resolve_agent_by_id(
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
                item_id: self.item_id,
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
                stall_timeout_secs: self.task_ctx.safety.stall_timeout_secs,
                step_scope: agent_orchestrator::config::StepScope::Item,
                step_template_prompt: Some(prompt),
                project_id: &self.task_ctx.project_id,
                execution_profile: None,
                self_referential: self.task_ctx.self_referential,
                command_rule_index: None,
            },
        )
        .await?;
        let output = result
            .output
            .ok_or_else(|| anyhow!("adaptive planner produced no structured output"))?;
        Ok(output.stdout)
    }
}

struct AgentBackedAdaptiveExecutor<'a> {
    state: &'a Arc<InnerState>,
    task_id: &'a str,
    item_id: &'a str,
    item: &'a agent_orchestrator::dto::TaskItemRow,
    task_ctx: &'a TaskRuntimeContext,
    runtime: &'a RunningTask,
}

fn build_dynamic_step_context(
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    acc: &StepExecutionAccumulator,
    step_id: &str,
) -> DynamicStepContext {
    let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, step_id);
    DynamicStepContext {
        task_id: prehook_ctx.task_id,
        task_item_id: prehook_ctx.task_item_id,
        cycle: prehook_ctx.cycle,
        step: prehook_ctx.step,
        qa_file_path: prehook_ctx.qa_file_path,
        item_status: prehook_ctx.item_status,
        task_status: prehook_ctx.task_status,
        qa_exit_code: prehook_ctx.qa_exit_code,
        fix_exit_code: prehook_ctx.fix_exit_code,
        retest_exit_code: prehook_ctx.retest_exit_code,
        active_ticket_count: prehook_ctx.active_ticket_count,
        new_ticket_count: prehook_ctx.new_ticket_count,
        qa_failed: prehook_ctx.qa_failed,
        fix_required: prehook_ctx.fix_required,
        qa_confidence: prehook_ctx.qa_confidence,
        qa_quality_score: prehook_ctx.qa_quality_score,
        fix_has_changes: prehook_ctx.fix_has_changes,
        upstream_artifacts: vec![],
        build_error_count: prehook_ctx.build_error_count,
        test_failure_count: prehook_ctx.test_failure_count,
        build_exit_code: prehook_ctx.build_exit_code,
        test_exit_code: prehook_ctx.test_exit_code,
        self_test_exit_code: prehook_ctx.self_test_exit_code,
        self_test_passed: prehook_ctx.self_test_passed,
        max_cycles: prehook_ctx.max_cycles,
        is_last_cycle: prehook_ctx.is_last_cycle,
        last_sandbox_denied: prehook_ctx.last_sandbox_denied,
        sandbox_denied_count: prehook_ctx.sandbox_denied_count,
        last_sandbox_denial_reason: prehook_ctx.last_sandbox_denial_reason,
        self_referential_safe: prehook_ctx.self_referential_safe,
        self_referential_safe_scenarios: prehook_ctx.self_referential_safe_scenarios,
        vars: prehook_ctx.vars,
    }
}

fn build_adaptive_history(
    task_id: &str,
    item_id: &str,
    task_ctx: &TaskRuntimeContext,
    acc: &StepExecutionAccumulator,
) -> Vec<ExecutionHistoryRecord> {
    let mut steps: Vec<StepExecutionRecord> = acc
        .exit_codes
        .iter()
        .map(|(step_id, exit_code)| StepExecutionRecord {
            step_id: step_id.clone(),
            step_type: step_id.clone(),
            exit_code: *exit_code,
            duration_ms: 0,
            confidence: if step_id.contains("qa") {
                acc.qa_confidence
            } else {
                acc.fix_confidence
            },
            quality_score: if step_id.contains("qa") {
                acc.qa_quality_score
            } else {
                acc.fix_quality_score
            },
            tickets_created: acc.new_ticket_count,
            tickets_resolved: 0,
        })
        .collect();
    steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));

    if steps.is_empty() {
        return Vec::new();
    }

    vec![ExecutionHistoryRecord {
        task_id: task_id.to_string(),
        item_id: item_id.to_string(),
        cycle: task_ctx.current_cycle,
        steps,
        final_status: acc.item_status.clone(),
        timestamp: chrono::Utc::now(),
    }]
}

async fn execute_adaptive_plan(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
    plan: &agent_orchestrator::dynamic_orchestration::DynamicExecutionPlan,
) -> Result<()> {
    let mut queue: VecDeque<String> = if let Some(entry) = plan.entry.clone() {
        VecDeque::from([entry])
    } else {
        let mut entries: Vec<String> = plan
            .get_entry_nodes()
            .into_iter()
            .map(|node| node.id.clone())
            .collect();
        entries.sort();
        entries.into()
    };
    let mut executed = HashSet::new();

    while let Some(node_id) = queue.pop_front() {
        if !executed.insert(node_id.clone()) {
            continue;
        }
        let node = plan
            .get_node(&node_id)
            .ok_or_else(|| anyhow!("adaptive plan node disappeared: {}", node_id))?;
        let dyn_step = agent_orchestrator::dynamic_orchestration::DynamicStepConfig {
            id: node.id.clone(),
            description: None,
            step_type: node.step_type.clone(),
            agent_id: node.agent_id.clone(),
            template: node.template.clone(),
            trigger: node.prehook.as_ref().map(|prehook| prehook.when.clone()),
            priority: 0,
            max_runs: None,
        };
        execute_dynamic_step_config(state, task_id, item, task_ctx, runtime, acc, &dyn_step)
            .await?;

        let dyn_ctx = build_dynamic_step_context(task_id, item, task_ctx, acc, &node_id);
        for next in plan.find_next_nodes(&node_id, &dyn_ctx) {
            if !executed.contains(&next) {
                queue.push_back(next);
            }
        }
    }

    Ok(())
}

pub(crate) async fn execute_dynamic_step_config(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &agent_orchestrator::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    acc: &mut StepExecutionAccumulator,
    ds: &agent_orchestrator::dynamic_orchestration::DynamicStepConfig,
) -> Result<()> {
    let item_id = item.id.as_str();
    insert_event(
        state,
        task_id,
        Some(item_id),
        "dynamic_step_started",
        json!({"step_id": ds.id, "step_type": ds.step_type, "step_scope": "item", "priority": ds.priority}),
    )
    .await?;
    // Resolve StepTemplate reference to actual prompt content before passing to phase execution
    let resolved_prompt = ds.template.as_ref().and_then(|tmpl_name| {
        let cfg = agent_orchestrator::config_load::read_loaded_config(state).ok()?;
        cfg.config
            .project(Some(&task_ctx.project_id))?
            .step_templates
            .get(tmpl_name)
            .map(|t| t.prompt.clone())
    });
    let result = if let Some(agent_id) = ds.agent_id.as_deref() {
        let workspace_root = crate::scheduler::loop_engine::isolation::step_workspace_root(
            task_ctx,
            &acc.pipeline_vars,
            agent_orchestrator::config::StepScope::Item,
        );
        let (command, prompt_delivery) = {
            let active = agent_orchestrator::config_load::read_active_config(state)?;
            let agent = agent_orchestrator::selection::resolve_agent_by_id(
                &task_ctx.project_id,
                &active.config,
                agent_id,
            )
            .ok_or_else(|| {
                anyhow!(
                    "dynamic step '{}' references unknown agent '{}'",
                    ds.id,
                    agent_id
                )
            })?;
            (agent.command.clone(), agent.prompt_delivery)
        };
        run_phase_with_selected_agent(
            state,
            SelectedPhaseRunRequest {
                task_id,
                item_id,
                step_id: &ds.id,
                phase: &ds.step_type,
                tty: false,
                agent_id,
                command_template: &command,
                prompt_delivery,
                rel_path: &item.qa_file_path,
                ticket_paths: &acc.active_tickets,
                workspace_root: &workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: None,
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
                stall_timeout_secs: task_ctx.safety.stall_timeout_secs,
                step_scope: agent_orchestrator::config::StepScope::Item,
                step_template_prompt: resolved_prompt.as_deref(),
                project_id: &task_ctx.project_id,
                execution_profile: None,
                self_referential: task_ctx.self_referential,
                command_rule_index: None,
            },
        )
        .await?
    } else {
        let cap = Some(ds.step_type.as_str());
        let workspace_root = crate::scheduler::loop_engine::isolation::step_workspace_root(
            task_ctx,
            &acc.pipeline_vars,
            agent_orchestrator::config::StepScope::Item,
        );
        run_phase_with_rotation(
            state,
            RotatingPhaseRunRequest {
                task_id,
                item_id,
                step_id: &ds.id,
                phase: &ds.step_type,
                tty: false,
                capability: cap,
                rel_path: &item.qa_file_path,
                ticket_paths: &acc.active_tickets,
                workspace_root: &workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: None,
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
                stall_timeout_secs: task_ctx.safety.stall_timeout_secs,
                step_scope: agent_orchestrator::config::StepScope::Item,
                step_template_prompt: resolved_prompt.as_deref(),
                project_id: &task_ctx.project_id,
                execution_profile: None,
                self_referential: task_ctx.self_referential,
            },
        )
        .await?
    };
    insert_event(
        state,
        task_id,
        Some(item_id),
        "dynamic_step_finished",
        json!({
            "step_id": ds.id,
            "step_scope": "item",
            "exit_code": result.exit_code,
            "success": result.is_success(),
            "execution_profile": result.execution_profile,
            "execution_mode": result.execution_mode,
            "sandbox_denied": result.sandbox_denied,
            "sandbox_denial_reason": result.sandbox_denial_reason,
            "sandbox_violation_kind": result.sandbox_violation_kind,
            "sandbox_resource_kind": result.sandbox_resource_kind,
            "sandbox_network_target": result.sandbox_network_target,
        }),
    )
    .await?;
    acc.exit_codes.insert(ds.id.clone(), result.exit_code);
    acc.step_ran.insert(ds.id.clone(), true);
    acc.apply_run_diagnostics(&result);
    match ds.step_type.as_str() {
        "qa" => {
            acc.flags
                .insert("qa_failed".to_string(), !result.is_success());
            if let Some(output) = result.output.as_ref() {
                acc.qa_confidence = Some(output.confidence);
                acc.qa_quality_score = Some(output.quality_score);
            }
        }
        "fix" => {
            acc.flags
                .insert("fix_success".to_string(), result.is_success());
            if let Some(output) = result.output.as_ref() {
                acc.fix_confidence = Some(output.confidence);
                acc.fix_quality_score = Some(output.quality_score);
            }
        }
        "retest" => {
            acc.flags
                .insert("retest_success".to_string(), result.is_success());
        }
        _ => {}
    }
    Ok(())
}
