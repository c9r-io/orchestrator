use agent_orchestrator::config::{
    ExecutionMode, OnFailureAction, PipelineVariables, TaskExecutionStep, TaskRuntimeContext,
};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::state::InnerState;
use agent_orchestrator::ticket::scan_active_tickets_for_task_items;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

use super::super::RunningTask;
use super::super::phase_runner::{
    PhaseRunRequest, RotatingPhaseRunRequest, run_phase, run_phase_with_rotation,
};
use super::super::safety::{SelfRestartOutcome, execute_self_restart_step, execute_self_test_step};
use super::accumulator::StepExecutionAccumulator;
use super::spill::{spill_large_var, spill_to_file};

/// Outcome of dispatching a builtin step (self_test, self_restart, ticket_scan).
pub(super) enum BuiltinStepOutcome {
    /// The builtin was recognized and fully handled; caller should `continue`.
    Handled { success: bool },
    /// The builtin triggered an early return from the outer function.
    EarlyReturn,
    /// Not a recognized builtin dispatch; fall through to agent/generic execution.
    NotBuiltin,
    /// Self-restart succeeded; daemon should exec the new binary.
    RestartRequested { binary_path: std::path::PathBuf },
}

/// Dispatch self_test, self_restart, and ticket_scan builtin steps.
/// These builtins handle their own result capture and event emission.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_builtin_step_dispatch(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    step: &TaskExecutionStep,
    effective_execution: &ExecutionMode,
    finish_event_type: &str,
    parent_step: Option<&str>,
    task_ctx: &TaskRuntimeContext,
    task_item_paths: &[String],
    qa_file_path: &str,
    acc: &mut StepExecutionAccumulator,
) -> Result<BuiltinStepOutcome> {
    match effective_execution {
        ExecutionMode::Builtin { name } if name == "self_test" => {
            // Self-test uses a specialized builtin
            use crate::scheduler::safety::SelfTestResult;
            let self_test_result = execute_self_test_step(
                &task_ctx.workspace_root,
                state,
                task_id,
                item_id,
                Some(task_ctx.project_id.as_str()),
            )
            .await
            .unwrap_or(SelfTestResult {
                exit_code: 1,
                error_output: String::new(),
            });
            let exit_code = self_test_result.exit_code;
            let passed = exit_code == 0;
            acc.pipeline_vars
                .vars
                .insert("self_test_exit_code".to_string(), exit_code.to_string());
            acc.pipeline_vars
                .vars
                .insert("self_test_passed".to_string(), passed.to_string());
            if !self_test_result.error_output.is_empty() {
                crate::scheduler::item_executor::spill::spill_large_var(
                    &task_ctx.artifacts_dir,
                    task_id,
                    "self_test_errors",
                    self_test_result.error_output,
                    &mut acc.pipeline_vars,
                );
            }

            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "exit_code": exit_code,
                "success": passed,
                "cycle": task_ctx.current_cycle
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

            // Apply behavior-driven status transitions for self_test
            if !passed {
                match &step.behavior.on_failure {
                    OnFailureAction::Continue => {}
                    OnFailureAction::SetStatus { status } => {
                        acc.item_status = status.clone();
                    }
                    OnFailureAction::EarlyReturn { status } => {
                        acc.item_status = status.clone();
                        acc.terminal = true;
                        return Ok(BuiltinStepOutcome::EarlyReturn);
                    }
                }
            }
            acc.step_ran.insert(step.id.clone(), true);
            acc.exit_codes.insert(step.id.clone(), exit_code as i64);
            // Apply captures
            let synth_result = agent_orchestrator::dto::RunResult {
                success: passed,
                exit_code: exit_code as i64,
                stdout_path: String::new(),
                stderr_path: String::new(),
                timed_out: false,
                duration_ms: None,
                output: None,
                validation_status: "passed".to_string(),
                agent_id: "builtin".to_string(),
                run_id: String::new(),
                execution_profile: "host".to_string(),
                execution_mode: "host".to_string(),
                sandbox_denied: false,
                sandbox_denial_reason: None,
                sandbox_violation_kind: None,
                sandbox_resource_kind: None,
                sandbox_network_target: None,
            };
            let _captures_missing = acc.apply_captures(
                &step.behavior.captures,
                &task_ctx.artifacts_dir,
                task_id,
                &step.id,
                &synth_result,
            );
            Ok(BuiltinStepOutcome::Handled { success: passed })
        }

        ExecutionMode::Builtin { name } if name == "self_restart" => {
            // Self-restart builtin: rebuild, verify, snapshot, then signal restart
            let ws_root = std::path::Path::new(&task_ctx.workspace_root);
            let outcome = execute_self_restart_step(ws_root, state, task_id, item_id)
                .await
                .unwrap_or(SelfRestartOutcome::Failed(1));

            match outcome {
                SelfRestartOutcome::RestartReady { binary_path } => {
                    let exit_code: i64 = 75; // EXIT_RESTART for event/vars compat
                    acc.pipeline_vars
                        .vars
                        .insert("self_restart_exit_code".to_string(), exit_code.to_string());

                    let mut payload = json!({
                        "step": phase,
                        "step_id": step.id,
                        "step_scope": step.resolved_scope(),
                        "exit_code": exit_code,
                        "restart": true,
                        "cycle": task_ctx.current_cycle
                    });
                    if let Some(parent_step) = parent_step {
                        payload["parent_step"] = json!(parent_step);
                    }
                    insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

                    // Invariant checkpoint: before_restart
                    let inv_results = crate::scheduler::invariant::evaluate_invariants(
                        &task_ctx.pinned_invariants,
                        agent_orchestrator::config::InvariantCheckPoint::BeforeRestart,
                        &task_ctx.workspace_root,
                    );
                    if let Ok(ref results) = inv_results {
                        for r in results {
                            let event_type = if r.passed {
                                "invariant_passed"
                            } else {
                                "invariant_violated"
                            };
                            let _ = insert_event(
                                state, task_id, Some(item_id), event_type,
                                json!({"invariant": r.name, "checkpoint": "BeforeRestart", "passed": r.passed, "message": r.message}),
                            ).await;
                        }
                        if crate::scheduler::invariant::has_halting_violation(results) {
                            tracing::warn!("invariant halt at before_restart — aborting restart");
                            acc.step_ran.insert(step.id.clone(), true);
                            acc.exit_codes.insert(step.id.clone(), exit_code);
                            return Ok(BuiltinStepOutcome::EarlyReturn);
                        }
                    }

                    // Persist pipeline vars to DB before restart so the new process recovers them.
                    if let Ok(json) = serde_json::to_string(&acc.pipeline_vars) {
                        if let Err(e) = state
                            .db_writer
                            .update_task_pipeline_vars(task_id, &json)
                            .await
                        {
                            tracing::warn!("failed to persist pipeline_vars before restart: {e}");
                        }
                    }

                    acc.step_ran.insert(step.id.clone(), true);
                    acc.exit_codes.insert(step.id.clone(), exit_code);
                    // Signal restart up the call stack — daemon layer handles exec()
                    Ok(BuiltinStepOutcome::RestartRequested { binary_path })
                }
                SelfRestartOutcome::Failed(exit_code) => {
                    acc.pipeline_vars
                        .vars
                        .insert("self_restart_exit_code".to_string(), exit_code.to_string());

                    let mut payload = json!({
                        "step": phase,
                        "step_id": step.id,
                        "step_scope": step.resolved_scope(),
                        "exit_code": exit_code,
                        "restart": false,
                        "cycle": task_ctx.current_cycle
                    });
                    if let Some(parent_step) = parent_step {
                        payload["parent_step"] = json!(parent_step);
                    }
                    insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;

                    // Build or verification failed — apply on_failure behavior
                    if exit_code != 0 {
                        match &step.behavior.on_failure {
                            OnFailureAction::Continue => {}
                            OnFailureAction::SetStatus { status } => {
                                acc.item_status = status.clone();
                            }
                            OnFailureAction::EarlyReturn { status } => {
                                acc.item_status = status.clone();
                                acc.terminal = true;
                                return Ok(BuiltinStepOutcome::EarlyReturn);
                            }
                        }
                    }
                    acc.step_ran.insert(step.id.clone(), true);
                    acc.exit_codes.insert(step.id.clone(), exit_code);
                    Ok(BuiltinStepOutcome::Handled {
                        success: exit_code == 0,
                    })
                }
            }
        }

        ExecutionMode::Builtin { name } if name == "ticket_scan" => {
            // Ticket scan builtin (step_started already emitted above)
            let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
            acc.active_tickets = tickets.get(qa_file_path).cloned().unwrap_or_default();
            acc.new_ticket_count = acc.active_tickets.len() as i64;
            acc.step_ran.insert(step.id.clone(), true);
            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "tickets": acc.active_tickets.len(),
                "cycle": task_ctx.current_cycle
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;
            Ok(BuiltinStepOutcome::Handled { success: true })
        }

        ExecutionMode::Builtin { name } if name == "item_select" => {
            // Selection orchestrated at loop_engine level; this is a marker step
            acc.step_ran.insert(step.id.clone(), true);
            let mut payload = json!({
                "step": phase,
                "step_id": step.id,
                "step_scope": step.resolved_scope(),
                "builtin": "item_select",
                "cycle": task_ctx.current_cycle
            });
            if let Some(parent_step) = parent_step {
                payload["parent_step"] = json!(parent_step);
            }
            insert_event(state, task_id, Some(item_id), finish_event_type, payload).await?;
            Ok(BuiltinStepOutcome::Handled { success: true })
        }

        _ => Ok(BuiltinStepOutcome::NotBuiltin),
    }
}

pub(crate) struct BuiltinStepContext<'a> {
    pub(super) task_ctx: &'a TaskRuntimeContext,
    pub(super) pipeline_vars: &'a PipelineVariables,
    pub(super) runtime: &'a RunningTask,
    pub(super) rel_path: &'a str,
    pub(super) workspace_root: std::path::PathBuf,
}

pub(crate) async fn execute_builtin_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step: &TaskExecutionStep,
    ctx: BuiltinStepContext<'_>,
) -> Result<(agent_orchestrator::dto::RunResult, PipelineVariables)> {
    let phase = &step.id;
    let task_ctx = ctx.task_ctx;
    let pipeline_vars = ctx.pipeline_vars;
    let runtime = ctx.runtime;
    let rel_path = ctx.rel_path;
    let workspace_root = ctx.workspace_root;

    let result = if let Some(ref command) = step.command {
        let ctx = agent_orchestrator::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            task_ctx.current_cycle,
            phase.to_string(),
            workspace_root.clone(),
            task_ctx.workspace_id.clone(),
        );
        let rendered_command = ctx.render_template_with_pipeline(command, Some(pipeline_vars));

        run_phase(
            state,
            PhaseRunRequest {
                task_id,
                item_id,
                step_id: &step.id,
                phase,
                tty: step.tty,
                command: rendered_command,
                command_template: Some(command.to_string()),
                workspace_root: &workspace_root,
                workspace_id: &task_ctx.workspace_id,
                agent_id: "builtin",
                runtime,
                step_timeout_secs: step.timeout_secs,
                stall_timeout_secs: step
                    .stall_timeout_secs
                    .or(task_ctx.safety.stall_timeout_secs),
                step_scope: step.resolved_scope(),
                prompt_delivery: agent_orchestrator::config::PromptDelivery::Arg,
                prompt_payload: None,
                pipe_stdin: false,
                project_id: &task_ctx.project_id,
                execution_profile: None,
                self_referential: task_ctx.self_referential,
                command_rule_index: None,
            },
        )
        .await?
    } else {
        let resolved_prompt = step.template.as_ref().and_then(|tmpl_name| {
            let cfg = agent_orchestrator::config_load::read_loaded_config(state).ok()?;
            cfg.config
                .project(Some(&task_ctx.project_id))?
                .step_templates
                .get(tmpl_name)
                .map(|t| t.prompt.clone())
        });
        run_phase_with_rotation(
            state,
            RotatingPhaseRunRequest {
                task_id,
                item_id,
                step_id: &step.id,
                phase,
                tty: step.tty,
                capability: step.required_capability.as_deref(),
                rel_path,
                ticket_paths: &[],
                workspace_root: &workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: Some(pipeline_vars),
                step_timeout_secs: step.timeout_secs.or(task_ctx.safety.step_timeout_secs),
                stall_timeout_secs: step
                    .stall_timeout_secs
                    .or(task_ctx.safety.stall_timeout_secs),
                step_scope: step.resolved_scope(),
                step_template_prompt: resolved_prompt.as_deref(),
                project_id: &task_ctx.project_id,
                execution_profile: step.execution_profile.as_deref(),
                self_referential: task_ctx.self_referential,
            },
        )
        .await?
    };

    let mut pipeline = pipeline_vars.clone();
    if let Some(ref output) = result.output {
        pipeline.prev_stdout = output.stdout.clone();
        pipeline.prev_stderr = output.stderr.clone();
        if let Some((trunc, path)) = spill_to_file(
            &task_ctx.artifacts_dir,
            task_id,
            "prev_stdout",
            &pipeline.prev_stdout,
        ) {
            pipeline.prev_stdout = trunc;
            pipeline.vars.insert("prev_stdout_path".to_string(), path);
        }
        if let Some((trunc, path)) = spill_to_file(
            &task_ctx.artifacts_dir,
            task_id,
            "prev_stderr",
            &pipeline.prev_stderr,
        ) {
            pipeline.prev_stderr = trunc;
            pipeline.vars.insert("prev_stderr_path".to_string(), path);
        }
        pipeline.build_errors = output.build_errors.clone();
        pipeline.test_failures = output.test_failures.clone();

        let output_key = format!("{}_output", phase);
        if !output.stdout.is_empty() {
            spill_large_var(
                &task_ctx.artifacts_dir,
                task_id,
                &output_key,
                output.stdout.clone(),
                &mut pipeline,
            );
        }
    }

    if let Ok(diff_output) = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(&workspace_root)
        .output()
        .await
    {
        pipeline.diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
        if let Some((trunc, path)) = spill_to_file(&task_ctx.artifacts_dir, task_id, "diff", &pipeline.diff)
        {
            pipeline.diff = trunc;
            pipeline.vars.insert("diff_path".to_string(), path);
        }
    }

    Ok((result, pipeline))
}

pub(super) fn is_execution_hard_failure(result: &agent_orchestrator::dto::RunResult) -> bool {
    result.validation_status == "failed"
}
