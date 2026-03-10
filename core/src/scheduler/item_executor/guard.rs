use crate::config::{ExecutionMode, TaskExecutionStep, TaskRuntimeContext};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::state::{read_agent_health, read_agent_metrics, write_agent_metrics, InnerState};
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;

use super::super::phase_runner::{run_phase, shell_escape, PhaseRunRequest};
use super::super::task_state::count_unresolved_items;
use super::super::RunningTask;

pub struct GuardResult {
    pub should_stop: bool,
    pub reason: String,
}

pub async fn execute_guard_step(
    state: &Arc<InnerState>,
    task_id: &str,
    step: &TaskExecutionStep,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<GuardResult> {
    if let ExecutionMode::Builtin { name } = step.effective_execution_mode().as_ref() {
        if name == "loop_guard" {
            let unresolved = count_unresolved_items(state, task_id).await?;
            // Respect stop_when_no_unresolved config: only stop on zero unresolved
            // when the guard is configured to do so. In Fixed mode with max_cycles,
            // the loop_engine's evaluate_loop_guard_rules handles cycle counting
            // separately, so the builtin guard should not short-circuit it.
            let should_stop = task_ctx
                .execution_plan
                .loop_policy
                .guard
                .stop_when_no_unresolved
                && unresolved == 0;
            return Ok(GuardResult {
                should_stop,
                reason: if should_stop {
                    "no_unresolved".to_string()
                } else {
                    "has_unresolved".to_string()
                },
            });
        }
    }

    let (agent_id, template, _prompt_delivery) = {
        let active = crate::config_load::read_active_config(state)?;
        let health_map = read_agent_health(state);
        let metrics_map = read_agent_metrics(state);
        let agents = crate::selection::resolve_effective_agents(
            &task_ctx.project_id,
            &active.config,
            step.required_capability.as_deref(),
        );
        if let Some(capability) = &step.required_capability {
            select_agent_advanced(
                capability,
                agents,
                &health_map,
                &metrics_map,
                &HashSet::new(),
            )?
        } else {
            select_agent_by_preference(agents)?
        }
    };

    {
        let mut metrics_map = write_agent_metrics(state);
        let metrics = metrics_map
            .entry(agent_id.clone())
            .or_insert_with(crate::metrics::MetricsCollector::new_agent_metrics);
        crate::metrics::MetricsCollector::increment_load(metrics);
    }

    let command = template
        .replace("{task_id}", &shell_escape(task_id))
        .replace(
            "{cycle}",
            &shell_escape(&task_ctx.current_cycle.to_string()),
        );

    let result = run_phase(
        state,
        PhaseRunRequest {
            task_id,
            item_id: task_id,
            step_id: &step.id,
            phase: "guard",
            tty: step.tty,
            command,
            workspace_root: &task_ctx.workspace_root,
            workspace_id: &task_ctx.workspace_id,
            agent_id: &agent_id,
            runtime,
            step_timeout_secs: None,
            step_scope: crate::config::StepScope::Task,
            prompt_delivery: crate::config::PromptDelivery::Arg,
            prompt_payload: None,
            pipe_stdin: false,
            project_id: &task_ctx.project_id,
            execution_profile: None,
        },
    )
    .await?;

    let guard_output = result
        .output
        .as_ref()
        .map(|o| o.stdout.clone())
        .unwrap_or_default();
    let parsed: serde_json::Value =
        serde_json::from_str(&guard_output).unwrap_or(serde_json::Value::Null);
    let should_stop = parsed
        .get("should_stop")
        .and_then(|v| v.as_bool())
        .or_else(|| parsed.get("continue").and_then(|v| v.as_bool()).map(|v| !v))
        .unwrap_or(false);
    let reason = parsed
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "guard_json".to_string());

    Ok(GuardResult {
        should_stop,
        reason,
    })
}

/// Pure function: evaluate the builtin loop_guard decision.
pub(crate) fn evaluate_builtin_loop_guard(
    stop_when_no_unresolved: bool,
    unresolved: u64,
) -> GuardResult {
    let should_stop = stop_when_no_unresolved && unresolved == 0;
    GuardResult {
        should_stop,
        reason: if should_stop {
            "no_unresolved".to_string()
        } else {
            "has_unresolved".to_string()
        },
    }
}

/// Pure function: parse guard output JSON from stdout.
pub(crate) fn parse_guard_output(stdout: &str) -> GuardResult {
    let parsed: serde_json::Value =
        serde_json::from_str(stdout).unwrap_or(serde_json::Value::Null);
    let should_stop = parsed
        .get("should_stop")
        .and_then(|v| v.as_bool())
        .or_else(|| parsed.get("continue").and_then(|v| v.as_bool()).map(|v| !v))
        .unwrap_or(false);
    let reason = parsed
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "guard_json".to_string());
    GuardResult {
        should_stop,
        reason,
    }
}
