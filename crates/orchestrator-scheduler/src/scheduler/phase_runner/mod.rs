mod record;
mod setup;
mod spawn;
mod tests;
mod types;
mod util;
mod validate;
mod wait;

pub use types::{PhaseRunRequest, RotatingPhaseRunRequest, SelectedPhaseRunRequest};
pub(crate) use util::shell_escape;

use agent_orchestrator::config::PromptDelivery;
use agent_orchestrator::events::insert_event;
use agent_orchestrator::metrics::MetricsCollector;
use agent_orchestrator::runner::{DaemonPidGuardBlocked, SandboxBackendError};
use agent_orchestrator::selection::{select_agent_advanced, select_agent_by_preference};
use agent_orchestrator::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use uuid::Uuid;

use super::RunningTask;

use record::record_phase_results;
use setup::setup_phase_execution;
use spawn::spawn_phase_process;
use util::detect_sandbox_violation;
use validate::validate_phase_output_stage;
use wait::wait_for_process;

#[allow(clippy::too_many_arguments)]
fn render_step_template_prompt(
    prompt: &str,
    rel_path: &str,
    phase: &str,
    cycle: u32,
    ticket_paths: &[String],
    task_id: &str,
    item_id: &str,
    workspace_root: &std::path::Path,
    workspace_id: &str,
    pipeline_vars: Option<&agent_orchestrator::config::PipelineVariables>,
) -> String {
    let mut rendered = prompt
        .replace("{rel_path}", rel_path)
        .replace("{phase}", phase)
        .replace("{cycle}", &cycle.to_string());
    rendered = rendered.replace("{ticket_paths}", &ticket_paths.join(" "));
    if pipeline_vars.is_some()
        || rendered.contains("{source_tree}")
        || rendered.contains("{workspace_root}")
    {
        let ctx = agent_orchestrator::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            cycle,
            phase.to_string(),
            workspace_root.to_path_buf(),
            workspace_id.to_string(),
        );
        rendered = ctx.render_template_with_pipeline(&rendered, pipeline_vars);
    }
    rendered
}

fn apply_prompt_delivery(
    command_template: &str,
    rendered_prompt: Option<&str>,
    prompt_delivery: PromptDelivery,
    agent_id: &str,
) -> (String, Option<String>) {
    match prompt_delivery {
        PromptDelivery::Arg => {
            let cmd = if let Some(prompt) = rendered_prompt {
                command_template.replace("{prompt}", &shell_escape(prompt))
            } else {
                command_template.to_string()
            };
            (cmd, None)
        }
        _ => {
            if command_template.contains("{prompt}") {
                tracing::warn!(
                    agent_id = %agent_id,
                    "command contains {{prompt}} but prompt_delivery={:?}; placeholder ignored",
                    prompt_delivery
                );
            }
            (
                command_template.to_string(),
                rendered_prompt.map(ToOwned::to_owned),
            )
        }
    }
}

/// Orchestrator: runs a single phase with timeout by calling the 5 extracted stages in sequence.
async fn run_phase_with_timeout(
    state: &Arc<InnerState>,
    request: PhaseRunRequest<'_>,
) -> Result<agent_orchestrator::dto::RunResult> {
    let PhaseRunRequest {
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        command,
        command_template,
        workspace_root,
        workspace_id,
        agent_id,
        runtime,
        step_timeout_secs,
        stall_timeout_secs,
        step_scope,
        prompt_delivery,
        prompt_payload,
        pipe_stdin: req_pipe_stdin,
        project_id,
        execution_profile,
        self_referential,
        command_rule_index,
    } = request;

    // Stage 1: setup
    let mut setup = match setup_phase_execution(
        state,
        task_id,
        item_id,
        phase,
        tty,
        command,
        command_template,
        workspace_root,
        workspace_id,
        agent_id,
        prompt_delivery,
        &prompt_payload,
        project_id,
        execution_profile,
        self_referential,
        command_rule_index,
    )
    .await
    {
        Ok(setup) => setup,
        Err(err) => {
            if let Some(result) = handle_daemon_pid_guard_blocked(
                state, &err, task_id, item_id, step_id, phase, step_scope, agent_id,
            )
            .await?
            {
                return Ok(result);
            }
            if let Some(result) = handle_sandbox_backend_error(
                state,
                &err,
                task_id,
                item_id,
                step_id,
                phase,
                step_scope,
                agent_id,
                execution_profile.unwrap_or("host"),
            )
            .await?
            {
                return Ok(result);
            }
            return Err(err);
        }
    };

    // Stage 2: spawn
    let spawn_result = match spawn_phase_process(
        state,
        &mut setup,
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        workspace_root,
        agent_id,
        runtime,
        step_scope,
        &prompt_payload,
        req_pipe_stdin,
    )
    .await
    {
        Ok(spawn_result) => spawn_result,
        Err(err) => {
            if let Some(result) = handle_daemon_pid_guard_blocked(
                state, &err, task_id, item_id, step_id, phase, step_scope, agent_id,
            )
            .await?
            {
                return Ok(result);
            }
            if let Some(result) = handle_sandbox_backend_error(
                state,
                &err,
                task_id,
                item_id,
                step_id,
                phase,
                step_scope,
                agent_id,
                &setup.execution_profile.name,
            )
            .await?
            {
                return Ok(result);
            }
            return Err(err);
        }
    };

    // TTY early return
    if let Some(result) = spawn_result.tty_early_return {
        return Ok(result);
    }

    // Stage 3: wait
    let wait_result = wait_for_process(
        state,
        task_id,
        item_id,
        step_id,
        phase,
        step_scope,
        step_timeout_secs,
        stall_timeout_secs,
        runtime,
        spawn_result.child_pid,
        spawn_result.output_capture,
        &setup.stdout_path,
        &setup.stderr_path,
    )
    .await?;

    // Stage 4: validate
    let validated = validate_phase_output_stage(
        phase,
        setup.run_uuid,
        &setup.run_id,
        agent_id,
        wait_result.exit_code,
        &setup.stdout_path,
        &setup.stderr_path,
        &setup.redaction_patterns,
    )
    .await?;

    let sandbox_violation =
        detect_sandbox_violation(&setup.execution_profile, &wait_result, &setup.stderr_path).await;
    let validated = super::phase_runner::types::ValidatedOutput {
        sandbox_denied: sandbox_violation.denied,
        sandbox_event_type: sandbox_violation.event_type,
        sandbox_reason_code: sandbox_violation.reason_code,
        sandbox_denial_reason: sandbox_violation.reason,
        sandbox_denial_stderr_excerpt: sandbox_violation.stderr_excerpt,
        sandbox_resource_kind: sandbox_violation.resource_kind,
        sandbox_network_target: sandbox_violation.network_target,
        ..validated
    };

    // Stage 5: record results
    record_phase_results(
        state,
        &setup,
        &validated,
        &spawn_result.session_id,
        task_id,
        item_id,
        step_id,
        phase,
        step_scope,
        tty,
        workspace_root,
        workspace_id,
        agent_id,
        wait_result.duration,
    )
    .await?;

    let duration_ms = wait_result.duration.as_millis() as u64;
    Ok(agent_orchestrator::dto::RunResult {
        success: validated.success,
        exit_code: validated.final_exit_code,
        stdout_path: setup.stdout_path.to_string_lossy().to_string(),
        stderr_path: setup.stderr_path.to_string_lossy().to_string(),
        timed_out: wait_result.timed_out,
        duration_ms: Some(duration_ms),
        output: Some(validated.redacted_output),
        validation_status: validated.validation_status.to_string(),
        agent_id: agent_id.to_string(),
        run_id: setup.run_id,
        execution_profile: setup.execution_profile.name,
        execution_mode: match setup.execution_profile.mode {
            agent_orchestrator::config::ExecutionProfileMode::Host => "host".to_string(),
            agent_orchestrator::config::ExecutionProfileMode::Sandbox => "sandbox".to_string(),
        },
        sandbox_denied: validated.sandbox_denied,
        sandbox_denial_reason: validated.sandbox_denial_reason.clone(),
        sandbox_violation_kind: validated.sandbox_event_type.map(str::to_string),
        sandbox_resource_kind: validated
            .sandbox_resource_kind
            .as_ref()
            .map(|value| value.as_str().to_string()),
        sandbox_network_target: validated.sandbox_network_target.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn handle_sandbox_backend_error(
    state: &Arc<InnerState>,
    err: &anyhow::Error,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    step_scope: agent_orchestrator::config::StepScope,
    agent_id: &str,
    execution_profile: &str,
) -> Result<Option<agent_orchestrator::dto::RunResult>> {
    let Some(sandbox_err) = err.downcast_ref::<SandboxBackendError>() else {
        return Ok(None);
    };
    let run_id = Uuid::new_v4().to_string();
    insert_event(
        state,
        task_id,
        Some(item_id),
        sandbox_err.event_type,
        json!({
            "step": phase,
            "step_id": step_id,
            "step_scope": match step_scope {
                agent_orchestrator::config::StepScope::Task => "task",
                agent_orchestrator::config::StepScope::Item => "item",
            },
            "agent_id": agent_id,
            "run_id": run_id,
            "execution_profile": execution_profile,
            "execution_mode": "sandbox",
            "reason_code": sandbox_err.reason_code,
            "reason": sandbox_err.reason_code,
            "resource_kind": sandbox_err.resource_kind.as_ref().map(|value| value.as_str()),
            "backend": sandbox_err.backend,
            "stderr_excerpt": sandbox_err.to_string(),
        }),
    )
    .await?;
    Ok(Some(agent_orchestrator::dto::RunResult {
        success: false,
        exit_code: -7,
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: Some(0),
        output: None,
        validation_status: "failed".to_string(),
        agent_id: agent_id.to_string(),
        run_id,
        execution_profile: sandbox_err.execution_profile.clone(),
        execution_mode: "sandbox".to_string(),
        sandbox_denied: true,
        sandbox_denial_reason: Some(sandbox_err.reason_code.to_string()),
        sandbox_violation_kind: Some(sandbox_err.event_type.to_string()),
        sandbox_resource_kind: sandbox_err
            .resource_kind
            .as_ref()
            .map(|value| value.as_str().to_string()),
        sandbox_network_target: None,
    }))
}

/// Handles a `DaemonPidGuardBlocked` error by recording an audit event and
/// returning a successful (skipped) result so the step does not produce false
/// failure tickets.
#[allow(clippy::too_many_arguments)]
async fn handle_daemon_pid_guard_blocked(
    state: &Arc<InnerState>,
    err: &anyhow::Error,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    step_scope: agent_orchestrator::config::StepScope,
    agent_id: &str,
) -> Result<Option<agent_orchestrator::dto::RunResult>> {
    let Some(guard_err) = err.downcast_ref::<DaemonPidGuardBlocked>() else {
        return Ok(None);
    };
    let run_id = Uuid::new_v4().to_string();
    insert_event(
        state,
        task_id,
        Some(item_id),
        "daemon_pid_kill_blocked",
        json!({
            "step": phase,
            "step_id": step_id,
            "step_scope": match step_scope {
                agent_orchestrator::config::StepScope::Task => "task",
                agent_orchestrator::config::StepScope::Item => "item",
            },
            "agent_id": agent_id,
            "run_id": run_id,
            "daemon_pid": std::process::id(),
            "matched_pattern": guard_err.matched_pattern,
            "reason": guard_err.reason,
        }),
    )
    .await?;
    Ok(Some(agent_orchestrator::dto::RunResult {
        success: true,
        exit_code: 0,
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: Some(0),
        output: None,
        validation_status: "skipped".to_string(),
        agent_id: agent_id.to_string(),
        run_id,
        execution_profile: String::new(),
        execution_mode: "host".to_string(),
        sandbox_denied: true,
        sandbox_denial_reason: Some("daemon_pid_guard".to_string()),
        sandbox_violation_kind: Some("daemon_pid_kill_blocked".to_string()),
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    }))
}

/// Runs one phase command with the standard timeout and capture pipeline.
pub async fn run_phase(
    state: &Arc<InnerState>,
    request: PhaseRunRequest<'_>,
) -> Result<agent_orchestrator::dto::RunResult> {
    run_phase_with_timeout(state, request).await
}

/// Resolves the effective agent command by evaluating `command_rules` in order.
///
/// Each rule's `when` CEL expression is evaluated against a prehook-style context
/// that includes pipeline `vars`. The first rule that evaluates to `true` provides
/// the command template. If no rule matches, the default command is returned.
/// Returns `(effective_command, matched_rule_index)`.
/// `matched_rule_index` is `None` when the default command is used.
pub(crate) fn resolve_agent_command(
    default_command: &str,
    command_rules: &[agent_orchestrator::config::AgentCommandRule],
    pipeline_vars: Option<&agent_orchestrator::config::PipelineVariables>,
    task_id: &str,
    item_id: &str,
    cycle: u32,
    phase: &str,
) -> (String, Option<i32>) {
    if command_rules.is_empty() {
        return (default_command.to_string(), None);
    }

    // Build a minimal prehook context for CEL evaluation.
    // The key ingredient is `vars` — the pipeline variables map.
    let vars = pipeline_vars
        .map(|pv| pv.vars.clone())
        .unwrap_or_default();
    let ctx = agent_orchestrator::config::StepPrehookContext {
        task_id: task_id.to_string(),
        task_item_id: item_id.to_string(),
        cycle,
        step: phase.to_string(),
        vars,
        ..Default::default()
    };

    for (i, rule) in command_rules.iter().enumerate() {
        match agent_orchestrator::prehook::evaluate_step_prehook_expression(&rule.when, &ctx) {
            Ok(true) => {
                tracing::debug!(
                    task_id,
                    phase,
                    rule_index = i,
                    expression = %rule.when,
                    "command_rule matched"
                );
                return (rule.command.clone(), Some(i as i32));
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    task_id,
                    phase,
                    rule_index = i,
                    expression = %rule.when,
                    error = %e,
                    "command_rule CEL evaluation failed, skipping rule"
                );
            }
        }
    }

    (default_command.to_string(), None)
}

/// Runs one phase command while coordinating with self-restart rotation safeguards.
pub async fn run_phase_with_rotation(
    state: &Arc<InnerState>,
    request: RotatingPhaseRunRequest<'_>,
) -> Result<agent_orchestrator::dto::RunResult> {
    let RotatingPhaseRunRequest {
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        capability,
        rel_path,
        ticket_paths,
        workspace_root,
        workspace_id,
        cycle,
        runtime,
        pipeline_vars,
        step_timeout_secs,
        stall_timeout_secs,
        step_scope,
        step_template_prompt,
        project_id,
        execution_profile,
        self_referential,
    } = request;
    let effective_capability = capability.or(match phase {
        "qa" | "fix" | "retest" => Some(phase),
        _ => None,
    });

    let (agent_id, template, prompt_delivery, command_rules) = {
        let active = agent_orchestrator::config_load::read_active_config(state)?;
        let agents = agent_orchestrator::selection::resolve_effective_agents(
            project_id,
            &active.config,
            effective_capability,
        )
        .clone();

        if let Some(cap) = effective_capability {
            let health_map = state.agent_health.read().await;
            let metrics_map = state.agent_metrics.read().await;
            let lifecycle_map = state.agent_lifecycle.read().await;
            select_agent_advanced(
                cap,
                &agents,
                &health_map,
                &metrics_map,
                &HashSet::new(),
                &lifecycle_map,
            )?
        } else {
            select_agent_by_preference(&agents)?
        }
    };

    // Resolve effective command from command_rules (CEL-based conditional selection)
    let (template, command_rule_index) = resolve_agent_command(
        &template,
        &command_rules,
        pipeline_vars,
        task_id,
        item_id,
        cycle,
        phase,
    );

    {
        let mut metrics_map = state.agent_metrics.write().await;
        let metrics = metrics_map
            .entry(agent_id.clone())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        MetricsCollector::increment_load(metrics);
    }

    agent_orchestrator::agent_lifecycle::increment_in_flight(state, &agent_id).await;

    run_phase_with_selected_agent(
        state,
        SelectedPhaseRunRequest {
            task_id,
            item_id,
            step_id,
            phase,
            tty,
            agent_id: &agent_id,
            command_template: &template,
            prompt_delivery,
            rel_path,
            ticket_paths,
            workspace_root,
            workspace_id,
            cycle,
            runtime,
            pipeline_vars,
            step_timeout_secs,
            stall_timeout_secs,
            step_scope,
            step_template_prompt,
            project_id,
            execution_profile,
            self_referential,
            command_rule_index,
        },
    )
    .await
}

pub async fn run_phase_with_selected_agent(
    state: &Arc<InnerState>,
    request: SelectedPhaseRunRequest<'_>,
) -> Result<agent_orchestrator::dto::RunResult> {
    let SelectedPhaseRunRequest {
        task_id,
        item_id,
        step_id,
        phase,
        tty,
        agent_id,
        command_template,
        prompt_delivery,
        rel_path,
        ticket_paths,
        workspace_root,
        workspace_id,
        cycle,
        runtime,
        pipeline_vars,
        step_timeout_secs,
        stall_timeout_secs,
        step_scope,
        step_template_prompt,
        project_id,
        execution_profile,
        self_referential,
        command_rule_index,
    } = request;

    // Render template variables into the step template prompt, then inject into agent command
    let rendered_prompt = step_template_prompt.map(|prompt| {
        render_step_template_prompt(
            prompt,
            rel_path,
            phase,
            cycle,
            ticket_paths,
            task_id,
            item_id,
            workspace_root,
            workspace_id,
            pipeline_vars,
        )
    });

    // Dispatch prompt into command based on delivery mode
    let (mut command, prompt_payload) = apply_prompt_delivery(
        command_template,
        rendered_prompt.as_deref(),
        prompt_delivery,
        agent_id,
    );

    // Snapshot the command after prompt injection but before variable substitution.
    // This captures the set of template variables that should be expanded.
    let command_template_snapshot = command.clone();

    let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
    command = command
        .replace("{rel_path}", &shell_escape(rel_path))
        .replace("{ticket_paths}", &escaped_paths.join(" "))
        .replace("{phase}", phase)
        .replace("{cycle}", &cycle.to_string());

    if pipeline_vars.is_some()
        || command.contains("{source_tree}")
        || command.contains("{workspace_root}")
    {
        let ctx = agent_orchestrator::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            cycle,
            phase.to_string(),
            workspace_root.to_path_buf(),
            workspace_id.to_string(),
        );
        command = ctx.render_template_with_pipeline(&command, pipeline_vars);
    }

    run_phase_with_timeout(
        state,
        PhaseRunRequest {
            task_id,
            item_id,
            step_id,
            phase,
            tty,
            command,
            command_template: Some(command_template_snapshot),
            workspace_root,
            workspace_id,
            agent_id,
            runtime,
            step_timeout_secs,
            stall_timeout_secs,
            step_scope,
            prompt_delivery,
            prompt_payload,
            pipe_stdin: prompt_delivery == PromptDelivery::Stdin,
            project_id,
            execution_profile,
            self_referential,
            command_rule_index,
        },
    )
    .await
}
