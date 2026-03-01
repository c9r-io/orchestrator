use crate::config::{
    CaptureSource, ExecutionMode, ItemFinalizeContext, OnFailureAction, OnSuccessAction,
    PipelineVariables, PostAction, StepPrehookContext, TaskExecutionStep,
    TaskRuntimeContext, PIPELINE_VAR_INLINE_LIMIT,
};
use crate::events::insert_event;
use crate::prehook::{emit_item_finalize_event, evaluate_step_prehook};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::state::InnerState;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use crate::ticket::{
    create_ticket_for_qa_failure, list_existing_tickets_for_item,
    scan_active_tickets_for_task_items,
};
use anyhow::Result;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use super::phase_runner::{
    run_phase, run_phase_with_rotation, shell_escape, PhaseRunRequest, RotatingPhaseRunRequest,
};
use super::safety::execute_self_test_step;
use super::task_state::count_unresolved_items;
use super::RunningTask;

/// Insert a pipeline variable, always writing the full content to a file and
/// setting a companion `{key}_path` variable.  When the value exceeds
/// [`PIPELINE_VAR_INLINE_LIMIT`] the inline value is truncated; otherwise the
/// full value is kept inline as well.
pub(crate) fn spill_large_var(
    logs_dir: &Path,
    task_id: &str,
    key: &str,
    value: String,
    pipeline: &mut PipelineVariables,
) {
    // Always write to file so downstream steps can reference {key}_path
    let dir = logs_dir.join(task_id);
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("{}.txt", key));
    std::fs::write(&path, &value).ok();
    pipeline
        .vars
        .insert(format!("{}_path", key), path.to_string_lossy().to_string());

    if value.len() <= PIPELINE_VAR_INLINE_LIMIT {
        pipeline.vars.insert(key.to_string(), value);
    } else {
        let safe_end = {
            let limit = PIPELINE_VAR_INLINE_LIMIT.min(value.len());
            let mut end = limit;
            while end > 0 && !value.is_char_boundary(end) {
                end -= 1;
            }
            end
        };
        let truncated = format!(
            "{}...\n[truncated — full content at {}]",
            &value[..safe_end],
            path.display()
        );
        pipeline.vars.insert(key.to_string(), truncated);
    }
}

/// Write a large value to a spill file and return `(truncated_value, path_string)`.
/// Returns `None` if the value fits within the inline limit.
pub(crate) fn spill_to_file(
    logs_dir: &Path,
    task_id: &str,
    key: &str,
    value: &str,
) -> Option<(String, String)> {
    if value.len() <= PIPELINE_VAR_INLINE_LIMIT {
        return None;
    }
    let dir = logs_dir.join(task_id);
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("{}.txt", key));
    std::fs::write(&path, value.as_bytes()).ok();

    let safe_end = {
        let limit = PIPELINE_VAR_INLINE_LIMIT.min(value.len());
        let mut end = limit;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        end
    };
    let truncated = format!(
        "{}...\n[truncated — full content at {}]",
        &value[..safe_end],
        path.display()
    );
    Some((truncated, path.to_string_lossy().to_string()))
}

// ── StepExecutionAccumulator ─────────────────────────────────────

/// Accumulator that tracks state across steps in the unified execution loop.
pub struct StepExecutionAccumulator {
    pub item_status: String,
    pub pipeline_vars: PipelineVariables,
    pub active_tickets: Vec<String>,
    pub created_ticket_files: Vec<String>,
    pub phase_artifacts: Vec<crate::collab::Artifact>,
    pub flags: HashMap<String, bool>,
    pub exit_codes: HashMap<String, i64>,
    pub step_ran: HashMap<String, bool>,
    pub step_skipped: HashMap<String, bool>,
    pub new_ticket_count: i64,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_confidence: Option<f32>,
    pub fix_quality_score: Option<f32>,
}

impl StepExecutionAccumulator {
    pub fn new(pipeline_vars: PipelineVariables) -> Self {
        Self {
            item_status: "pending".to_string(),
            pipeline_vars,
            active_tickets: Vec::new(),
            created_ticket_files: Vec::new(),
            phase_artifacts: Vec::new(),
            flags: HashMap::new(),
            exit_codes: HashMap::new(),
            step_ran: HashMap::new(),
            step_skipped: HashMap::new(),
            new_ticket_count: 0,
            qa_confidence: None,
            qa_quality_score: None,
            fix_confidence: None,
            fix_quality_score: None,
        }
    }

    /// Build a StepPrehookContext from accumulated state.
    pub fn to_prehook_context(
        &self,
        task_id: &str,
        item: &crate::dto::TaskItemRow,
        task_ctx: &TaskRuntimeContext,
        step_id: &str,
    ) -> StepPrehookContext {
        let max_cycles = task_ctx
            .execution_plan
            .loop_policy
            .guard
            .max_cycles
            .unwrap_or(1);
        StepPrehookContext {
            task_id: task_id.to_string(),
            task_item_id: item.id.clone(),
            cycle: task_ctx.current_cycle,
            step: step_id.to_string(),
            qa_file_path: item.qa_file_path.clone(),
            item_status: self.item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code: self.exit_codes.get("qa").or(self.exit_codes.get("qa_testing")).copied(),
            fix_exit_code: self.exit_codes.get("fix").or(self.exit_codes.get("ticket_fix")).copied(),
            retest_exit_code: self.exit_codes.get("retest").copied(),
            active_ticket_count: self.active_tickets.len() as i64,
            new_ticket_count: self.new_ticket_count,
            qa_failed: self.flags.get("qa_failed").copied().unwrap_or(false),
            fix_required: self.flags.get("qa_failed").copied().unwrap_or(false)
                || !self.active_tickets.is_empty(),
            qa_confidence: self.qa_confidence,
            qa_quality_score: self.qa_quality_score,
            fix_has_changes: None,
            upstream_artifacts: vec![],
            build_error_count: self.pipeline_vars.build_errors.len() as i64,
            test_failure_count: self.pipeline_vars.test_failures.len() as i64,
            build_exit_code: self.exit_codes.get("build").copied(),
            test_exit_code: self.exit_codes.get("test").copied(),
            self_test_exit_code: self
                .pipeline_vars
                .vars
                .get("self_test_exit_code")
                .and_then(|v| v.parse::<i64>().ok()),
            self_test_passed: self
                .pipeline_vars
                .vars
                .get("self_test_passed")
                .map(|v| v == "true")
                .unwrap_or(false),
            max_cycles,
            is_last_cycle: task_ctx.current_cycle >= max_cycles,
        }
    }

    /// Build an ItemFinalizeContext from accumulated state.
    pub fn to_finalize_context(
        &self,
        task_id: &str,
        item: &crate::dto::TaskItemRow,
        task_ctx: &TaskRuntimeContext,
    ) -> ItemFinalizeContext {
        let qa_ran = self.step_ran.get("qa").or(self.step_ran.get("qa_testing")).copied().unwrap_or(false);
        let qa_skipped = self.step_skipped.get("qa").or(self.step_skipped.get("qa_testing")).copied().unwrap_or(false);
        let qa_enabled = qa_ran || qa_skipped;
        let fix_ran = self.step_ran.get("fix").or(self.step_ran.get("ticket_fix")).copied().unwrap_or(false);
        let fix_success = self.flags.get("fix_success").copied().unwrap_or(false);
        let fix_enabled = fix_ran
            || self.step_skipped.get("fix").or(self.step_skipped.get("ticket_fix")).copied().unwrap_or(false)
            || task_ctx.execution_plan.steps.iter().any(|s| {
                (s.id == "fix" || s.id == "ticket_fix") && s.enabled
            });
        let retest_ran = self.step_ran.get("retest").copied().unwrap_or(false);
        let retest_success = self.flags.get("retest_success").copied().unwrap_or(false);
        let retest_enabled = retest_ran
            || self.step_skipped.get("retest").copied().unwrap_or(false)
            || task_ctx.execution_plan.steps.iter().any(|s| s.id == "retest" && s.enabled);

        ItemFinalizeContext {
            task_id: task_id.to_string(),
            task_item_id: item.id.clone(),
            cycle: task_ctx.current_cycle,
            qa_file_path: item.qa_file_path.clone(),
            item_status: self.item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code: self.exit_codes.get("qa").or(self.exit_codes.get("qa_testing")).copied(),
            fix_exit_code: self.exit_codes.get("fix").or(self.exit_codes.get("ticket_fix")).copied(),
            retest_exit_code: self.exit_codes.get("retest").copied(),
            active_ticket_count: self.active_tickets.len() as i64,
            new_ticket_count: self.new_ticket_count,
            retest_new_ticket_count: 0,
            qa_failed: self.flags.get("qa_failed").copied().unwrap_or(false),
            fix_required: !self.active_tickets.is_empty(),
            qa_enabled,
            qa_ran,
            qa_skipped,
            fix_enabled,
            fix_ran,
            fix_success,
            retest_enabled,
            retest_ran,
            retest_success,
            qa_confidence: self.qa_confidence,
            qa_quality_score: self.qa_quality_score,
            fix_confidence: self.fix_confidence,
            fix_quality_score: self.fix_quality_score,
            total_artifacts: self.phase_artifacts.len() as i64,
            has_ticket_artifacts: self
                .phase_artifacts
                .iter()
                .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. })),
            has_code_change_artifacts: self
                .phase_artifacts
                .iter()
                .any(|a| matches!(a.kind, crate::collab::ArtifactKind::CodeChange { .. })),
            is_last_cycle: task_ctx.current_cycle
                >= task_ctx
                    .execution_plan
                    .loop_policy
                    .guard
                    .max_cycles
                    .unwrap_or(1),
        }
    }

    /// Apply capture declarations from a step result into the accumulator.
    pub fn apply_captures(
        &mut self,
        captures: &[crate::config::CaptureDecl],
        step_id: &str,
        result: &crate::dto::RunResult,
    ) {
        for cap in captures {
            match cap.source {
                CaptureSource::ExitCode => {
                    self.exit_codes.insert(step_id.to_string(), result.exit_code);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), result.exit_code.to_string());
                }
                CaptureSource::FailedFlag => {
                    let failed = !result.is_success();
                    self.flags.insert(cap.var.clone(), failed);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), failed.to_string());
                }
                CaptureSource::SuccessFlag => {
                    let success = result.is_success();
                    self.flags.insert(cap.var.clone(), success);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), success.to_string());
                }
                CaptureSource::Stdout => {
                    if let Some(ref output) = result.output {
                        spill_large_var(
                            Path::new(""),
                            "",
                            &cap.var,
                            output.stdout.clone(),
                            &mut self.pipeline_vars,
                        );
                    }
                }
                CaptureSource::Stderr => {
                    if let Some(ref output) = result.output {
                        self.pipeline_vars
                            .vars
                            .insert(cap.var.clone(), output.stderr.clone());
                    }
                }
            }
        }
    }
}

pub async fn execute_builtin_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step: &TaskExecutionStep,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    rel_path: &str,
) -> Result<(crate::dto::RunResult, PipelineVariables)> {
    let phase = &step.id;

    let result = if let Some(ref command) = step.command {
        let ctx = crate::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            task_ctx.current_cycle,
            phase.to_string(),
            task_ctx.workspace_root.clone(),
            task_ctx.workspace_id.clone(),
        );
        let rendered_command =
            ctx.render_template_with_pipeline(command, Some(&task_ctx.pipeline_vars));

        run_phase(
            state,
            PhaseRunRequest {
                task_id,
                item_id,
                step_id: &step.id,
                phase,
                tty: step.tty,
                command: rendered_command,
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                agent_id: "builtin",
                runtime,
                step_timeout_secs: None,
            },
        )
        .await?
    } else {
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
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: Some(&task_ctx.pipeline_vars),
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
            },
        )
        .await?
    };

    let mut pipeline = task_ctx.pipeline_vars.clone();
    if let Some(ref output) = result.output {
        pipeline.prev_stdout = output.stdout.clone();
        pipeline.prev_stderr = output.stderr.clone();
        if let Some((trunc, path)) = spill_to_file(
            &state.logs_dir,
            task_id,
            "prev_stdout",
            &pipeline.prev_stdout,
        ) {
            pipeline.prev_stdout = trunc;
            pipeline.vars.insert("prev_stdout_path".to_string(), path);
        }
        if let Some((trunc, path)) = spill_to_file(
            &state.logs_dir,
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
                &state.logs_dir,
                task_id,
                &output_key,
                output.stdout.clone(),
                &mut pipeline,
            );
        }
    }

    if let Ok(diff_output) = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(&task_ctx.workspace_root)
        .output()
        .await
    {
        pipeline.diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
        if let Some((trunc, path)) = spill_to_file(&state.logs_dir, task_id, "diff", &pipeline.diff)
        {
            pipeline.diff = trunc;
            pipeline.vars.insert("diff_path".to_string(), path);
        }
    }

    Ok((result, pipeline))
}

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
    if let Some(builtin) = &step.builtin {
        if builtin.as_str() == "loop_guard" {
            let unresolved = count_unresolved_items(state, task_id)?;
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

    let (agent_id, template) = {
        let active = crate::config_load::read_active_config(state)?;
        let health_map = state.agent_health.read().unwrap();
        let metrics_map = state.agent_metrics.read().unwrap();
        if let Some(capability) = &step.required_capability {
            select_agent_advanced(
                capability,
                &active.config.agents,
                &health_map,
                &metrics_map,
                &HashSet::new(),
            )?
        } else {
            select_agent_by_preference(&active.config.agents)?
        }
    };

    {
        let mut metrics_map = state.agent_metrics.write().unwrap();
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

pub async fn process_item(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_item_paths: &[String],
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<()> {
    process_item_filtered(state, task_id, item, task_item_paths, task_ctx, runtime, None).await?;
    Ok(())
}

/// Process an item, optionally filtering to only run steps whose id is in `step_filter`.
/// When `step_filter` is `None`, all steps run.
/// Returns updated pipeline variables so callers can propagate task-scoped vars.
///
/// # Unified execution loop
/// Every step goes through the same path: prehook → execute → capture → status → post_actions.
/// Step-specific behaviors (on_failure, captures, post_actions) are declared as data in `StepBehavior`.
pub async fn process_item_filtered(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_item_paths: &[String],
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
    step_filter: Option<&HashSet<String>>,
) -> Result<PipelineVariables> {
    let item_id = item.id.as_str();
    let should_run_step = |step_id: &str| -> bool {
        step_filter.map_or(true, |f| f.contains(step_id))
    };
    let mut acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
    // ── Unified step loop ────────────────────────────────────────────

    for step in &task_ctx.execution_plan.steps {
        // Check for pause/stop between steps
        if runtime.stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(acc.pipeline_vars);
        }
        if super::task_state::is_task_paused_in_db(state, task_id)? {
            return Ok(acc.pipeline_vars);
        }

        // Skip guards (handled separately in loop_engine), disabled, and filtered-out steps
        if step.is_guard || !step.enabled || !should_run_step(&step.id) {
            continue;
        }
        if !step.repeatable && task_ctx.current_cycle > 1 {
            continue;
        }

        let phase = &step.id;

        // 1. Evaluate prehook
        let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, &step.id);
        let should_run = evaluate_step_prehook(state, step.prehook.as_ref(), &prehook_ctx)?;
        if !should_run {
            acc.step_skipped.insert(step.id.clone(), true);
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                json!({"step": phase, "step_id": &step.id, "reason": "prehook_false"}),
            )?;
            continue;
        }

        // 2. Execute
        let pipeline_var_keys: Vec<&String> = acc.pipeline_vars.vars.keys().collect();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_started",
            json!({"step": phase, "step_id": &step.id, "cycle": task_ctx.current_cycle, "pipeline_var_keys": pipeline_var_keys}),
        )?;

        let result = match &step.behavior.execution {
            ExecutionMode::Builtin { name } if name == "self_test" => {
                // Self-test uses a specialized builtin
                let exit_code =
                    execute_self_test_step(&task_ctx.workspace_root, state, task_id, item_id)
                        .await
                        .unwrap_or(1);
                let passed = exit_code == 0;
                acc.pipeline_vars
                    .vars
                    .insert("self_test_exit_code".to_string(), exit_code.to_string());
                acc.pipeline_vars
                    .vars
                    .insert("self_test_passed".to_string(), passed.to_string());

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": phase, "exit_code": exit_code, "success": passed}),
                )?;

                // Apply behavior-driven status transitions for self_test
                if !passed {
                    match &step.behavior.on_failure {
                        OnFailureAction::Continue => {}
                        OnFailureAction::SetStatus { status } => {
                            acc.item_status = status.clone();
                        }
                        OnFailureAction::EarlyReturn { status } => {
                            acc.item_status = status.clone();
                            state
                                .db_writer
                                .update_task_item_status(item_id, &acc.item_status)?;
                            return Ok(acc.pipeline_vars);
                        }
                    }
                }
                acc.step_ran.insert(step.id.clone(), true);
                acc.exit_codes.insert(step.id.clone(), exit_code as i64);
                // Apply captures
                let synth_result = crate::dto::RunResult {
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
                };
                acc.apply_captures(&step.behavior.captures, &step.id, &synth_result);
                continue;
            }

            ExecutionMode::Builtin { name } if name == "ticket_scan" => {
                // Ticket scan builtin
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_started",
                    json!({"step": "ticket_scan"}),
                )?;
                let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                acc.active_tickets = tickets
                    .get(&item.qa_file_path)
                    .cloned()
                    .unwrap_or_default();
                acc.new_ticket_count = acc.active_tickets.len() as i64;
                acc.step_ran.insert(step.id.clone(), true);
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": "ticket_scan", "tickets": acc.active_tickets.len()}),
                )?;
                continue;
            }

            ExecutionMode::Chain => {
                // Chain execution: run sub-steps in sequence
                let mut chain_passed = true;
                for chain_step in &step.chain_steps {
                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "chain_step_started",
                        json!({"step": phase, "chain_step": chain_step.id}),
                    )?;

                    let mut step_ctx = task_ctx.clone();
                    step_ctx.pipeline_vars = acc.pipeline_vars.clone();

                    let (chain_result, new_pipeline) = execute_builtin_step(
                        state,
                        task_id,
                        item_id,
                        chain_step,
                        &step_ctx,
                        runtime,
                        &item.qa_file_path,
                    )
                    .await?;
                    acc.pipeline_vars = new_pipeline;

                    if let Some(ref output) = chain_result.output {
                        if !output.stdout.is_empty() {
                            spill_large_var(
                                &state.logs_dir,
                                task_id,
                                "plan_output",
                                output.stdout.clone(),
                                &mut acc.pipeline_vars,
                            );
                        }
                    }

                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "chain_step_finished",
                        json!({
                            "step": phase,
                            "chain_step": chain_step.id,
                            "exit_code": chain_result.exit_code,
                            "success": chain_result.is_success()
                        }),
                    )?;

                    if !chain_result.is_success() {
                        chain_passed = false;
                        acc.item_status = format!("{}_failed", chain_step.id);
                        break;
                    }
                }
                acc.step_ran.insert(step.id.clone(), true);
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": phase, "success": chain_passed}),
                )?;
                continue;
            }

            // ExecutionMode::Agent or ExecutionMode::Builtin for generic builtins
            _ => {
                let mut step_ctx = task_ctx.clone();
                step_ctx.pipeline_vars = acc.pipeline_vars.clone();

                let (result, new_pipeline) = execute_builtin_step(
                    state,
                    task_id,
                    item_id,
                    step,
                    &step_ctx,
                    runtime,
                    &item.qa_file_path,
                )
                .await?;
                acc.pipeline_vars = new_pipeline;

                if let Some(ref output) = result.output {
                    if !output.stdout.is_empty() {
                        let output_key = format!("{}_output", phase);
                        spill_large_var(
                            &state.logs_dir,
                            task_id,
                            &output_key,
                            output.stdout.clone(),
                            &mut acc.pipeline_vars,
                        );
                    }
                }

                result
            }
        };

        // 3. Capture outputs
        acc.exit_codes.insert(step.id.clone(), result.exit_code);
        acc.apply_captures(&step.behavior.captures, &step.id, &result);
        acc.step_ran.insert(step.id.clone(), true);

        // 4. Status transitions
        if result.is_success() {
            if let OnSuccessAction::SetStatus { status } = &step.behavior.on_success {
                acc.item_status = status.clone();
            }
        } else {
            match &step.behavior.on_failure {
                OnFailureAction::Continue => {}
                OnFailureAction::SetStatus { status } => {
                    acc.item_status = status.clone();
                }
                OnFailureAction::EarlyReturn { status } => {
                    acc.item_status = status.clone();
                    state
                        .db_writer
                        .update_task_item_status(item_id, &acc.item_status)?;
                    return Ok(acc.pipeline_vars);
                }
            }
        }

        // 5. Post-actions
        for action in &step.behavior.post_actions {
            match action {
                PostAction::CreateTicket if !result.is_success() => {
                    if let Some(exit_code) = acc.exit_codes.get(&step.id) {
                        let task_name = SqliteTaskRepository::new(state.db_path.clone())
                            .load_task_name(task_id)?
                            .unwrap_or_else(|| task_id.to_string());
                        match create_ticket_for_qa_failure(
                            &task_ctx.workspace_root,
                            &task_ctx.ticket_dir,
                            &task_name,
                            &item.qa_file_path,
                            *exit_code,
                            &result.stdout_path,
                            &result.stderr_path,
                        ) {
                            Ok(Some(ticket_path)) => {
                                acc.created_ticket_files.push(ticket_path.clone());
                                insert_event(
                                    state,
                                    task_id,
                                    Some(item_id),
                                    "ticket_created",
                                    json!({"path": ticket_path, "qa_file": item.qa_file_path}),
                                )?;
                            }
                            Ok(None) => {}
                            Err(e) => eprintln!("[warn] failed to auto-create ticket: {e}"),
                        }
                    }
                }
                PostAction::ScanTickets => {
                    let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                    acc.active_tickets = tickets
                        .get(&item.qa_file_path)
                        .cloned()
                        .unwrap_or_default();
                    acc.new_ticket_count = acc.active_tickets.len() as i64;
                }
                _ => {}
            }
        }

        // 6. Collect artifacts
        if step.behavior.collect_artifacts {
            let step_artifacts = result
                .output
                .as_ref()
                .map(|o| o.artifacts.clone())
                .unwrap_or_default();
            if !step_artifacts.is_empty() {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "artifacts_parsed",
                    json!({"step": phase, "count": step_artifacts.len()}),
                )?;
                acc.phase_artifacts.extend(step_artifacts);
            }
        }

        // Also check for ticket artifacts that may seed active_tickets
        if acc.active_tickets.is_empty() {
            let ticket_artifact_count = acc
                .phase_artifacts
                .iter()
                .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
                .count();
            if ticket_artifact_count > 0 {
                acc.active_tickets = (0..ticket_artifact_count)
                    .map(|idx| format!("artifact://ticket/{}", idx))
                    .collect();
                acc.new_ticket_count = acc.active_tickets.len() as i64;
            }
        }

        let confidence = result.output.as_ref().map(|o| o.confidence).unwrap_or(0.0);
        let quality = result
            .output
            .as_ref()
            .map(|o| o.quality_score)
            .unwrap_or(0.0);

        match phase.as_str() {
            "qa" | "qa_testing" => {
                acc.qa_confidence = Some(confidence);
                acc.qa_quality_score = Some(quality);
            }
            "fix" | "ticket_fix" => {
                acc.fix_confidence = Some(confidence);
                acc.fix_quality_score = Some(quality);
            }
            _ => {}
        }

        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_finished",
            json!({
                "step": phase,
                "step_id": step.id,
                "agent_id": result.agent_id,
                "run_id": result.run_id,
                "exit_code": result.exit_code,
                "success": result.is_success(),
                "timed_out": result.timed_out,
                "duration_ms": result.duration_ms,
                "build_errors": acc.pipeline_vars.build_errors.len(),
                "test_failures": acc.pipeline_vars.test_failures.len(),
                "confidence": confidence,
                "quality_score": quality,
                "validation_status": result.validation_status,
            }),
        )?;
    }

    // Dynamic steps (only in full/legacy mode, not in segment-filtered mode)
    if !task_ctx.dynamic_steps.is_empty() && step_filter.is_none() {
        let pool = {
            let mut p = crate::dynamic_orchestration::DynamicStepPool::new();
            for ds in &task_ctx.dynamic_steps {
                p.add_step(ds.clone());
            }
            p
        };
        let prehook_ctx = acc.to_prehook_context(task_id, item, task_ctx, "dynamic");
        let dyn_ctx = crate::dynamic_orchestration::StepPrehookContext {
            task_id: prehook_ctx.task_id,
            task_item_id: prehook_ctx.task_item_id,
            cycle: prehook_ctx.cycle,
            step: "dynamic".to_string(),
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
        };
        let matched = pool.find_matching_steps(&dyn_ctx);
        for ds in matched {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "dynamic_step_started",
                json!({"step_id": ds.id, "step_type": ds.step_type, "priority": ds.priority}),
            )?;
            let cap = Some(ds.step_type.as_str());
            let result = run_phase_with_rotation(
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
                    workspace_root: &task_ctx.workspace_root,
                    workspace_id: &task_ctx.workspace_id,
                    cycle: task_ctx.current_cycle,
                    runtime,
                    pipeline_vars: None,
                    step_timeout_secs: task_ctx.safety.step_timeout_secs,
                },
            )
            .await?;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "dynamic_step_finished",
                json!({"step_id": ds.id, "exit_code": result.exit_code, "success": result.is_success()}),
            )?;
        }
    }

    // Seed active tickets from existing ticket files if no scan step ran
    if acc.active_tickets.is_empty()
        && !acc.step_ran.contains_key("ticket_scan")
    {
        acc.active_tickets = list_existing_tickets_for_item(task_ctx, &item.qa_file_path)?;
        acc.new_ticket_count = acc.active_tickets.len() as i64;
    }

    // ── Finalize via CEL rules ──────────────────────────────────────
    let finalize_context = acc.to_finalize_context(task_id, item, task_ctx);

    if let Some(outcome) = crate::prehook::resolve_workflow_finalize_outcome(
        &task_ctx.execution_plan.finalize,
        &finalize_context,
    )? {
        acc.item_status = outcome.status.clone();
        emit_item_finalize_event(state, &finalize_context, &outcome)?;
    }

    // Persist ticket artifacts
    let has_ticket_artifacts = !acc.created_ticket_files.is_empty()
        || acc
            .phase_artifacts
            .iter()
            .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }));
    if has_ticket_artifacts {
        let ticket_content: Vec<&serde_json::Value> = acc
            .phase_artifacts
            .iter()
            .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
            .filter_map(|a| a.content.as_ref())
            .collect();
        let files_json =
            serde_json::to_string(&acc.created_ticket_files).unwrap_or_else(|_| "[]".to_string());
        let content_json =
            serde_json::to_string(&ticket_content).unwrap_or_else(|_| "[]".to_string());
        state
            .db_writer
            .update_task_item_tickets(item_id, &files_json, &content_json)?;
    }

    state
        .db_writer
        .update_task_item_status(item_id, &acc.item_status)?;
    Ok(acc.pipeline_vars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PipelineVariables;
    use std::collections::HashMap;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "item-exec-test-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn empty_pipeline() -> PipelineVariables {
        PipelineVariables {
            prev_stdout: String::new(),
            prev_stderr: String::new(),
            diff: String::new(),
            build_errors: Vec::new(),
            test_failures: Vec::new(),
            vars: HashMap::new(),
        }
    }

    // ── spill_large_var tests ────────────────────────────────────────

    #[test]
    fn spill_large_var_small_value_inserts_inline() {
        let dir = temp_dir("slv-small");
        let mut pipeline = empty_pipeline();
        let value = "hello world".to_string();

        spill_large_var(&dir, "task1", "stdout", value.clone(), &mut pipeline);

        assert_eq!(pipeline.vars.get("stdout").unwrap(), "hello world");
        // _path is always set now (even for small values)
        let p = pipeline.vars.get("stdout_path").expect("stdout_path must be set");
        assert_eq!(std::fs::read_to_string(p).unwrap(), "hello world");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_exactly_at_limit_inserts_inline() {
        let dir = temp_dir("slv-exact");
        let mut pipeline = empty_pipeline();
        let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT);

        spill_large_var(&dir, "task1", "out", value.clone(), &mut pipeline);

        assert_eq!(pipeline.vars.get("out").unwrap(), &value);
        // _path is always set now (even for small values)
        let p = pipeline.vars.get("out_path").expect("out_path must be set");
        assert_eq!(std::fs::read_to_string(p).unwrap(), value);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_one_byte_over_limit_spills_to_file() {
        let dir = temp_dir("slv-over");
        let mut pipeline = empty_pipeline();
        let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT + 1);

        spill_large_var(&dir, "task1", "big", value.clone(), &mut pipeline);

        // Inline value should be truncated with the marker
        let inline = pipeline.vars.get("big").unwrap();
        assert!(inline.contains("...\n[truncated — full content at "));
        // The inline prefix (before the marker) should be at most PIPELINE_VAR_INLINE_LIMIT bytes
        let prefix_end = inline.find("...\n[truncated").unwrap();
        assert!(prefix_end <= PIPELINE_VAR_INLINE_LIMIT);

        // Companion path variable should exist
        let path_str = pipeline.vars.get("big_path").unwrap();
        let spill_path = std::path::Path::new(path_str);
        assert!(spill_path.exists());

        // File should contain the full original value
        let on_disk = std::fs::read_to_string(spill_path).unwrap();
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_large_value_sets_correct_path_key() {
        let dir = temp_dir("slv-pathkey");
        let mut pipeline = empty_pipeline();
        let value = "y".repeat(PIPELINE_VAR_INLINE_LIMIT + 100);

        spill_large_var(&dir, "t42", "my_key", value, &mut pipeline);

        let path_str = pipeline.vars.get("my_key_path").unwrap();
        assert!(path_str.contains("t42"));
        assert!(path_str.ends_with("my_key.txt"));
    }

    #[test]
    fn spill_large_var_multibyte_boundary() {
        let dir = temp_dir("slv-mb");
        let mut pipeline = empty_pipeline();
        // Build a string that puts a multi-byte char right at the 4096 boundary.
        // Chinese chars are 3 bytes each. Fill up to just before the limit, then
        // add a char whose encoding would straddle the boundary.
        let prefix_len = PIPELINE_VAR_INLINE_LIMIT - 1; // 4095 ASCII bytes
        let mut value = "a".repeat(prefix_len);
        // Append multi-byte chars so total exceeds limit
        value.push_str("你好世界"); // 12 bytes of UTF-8
        assert!(value.len() > PIPELINE_VAR_INLINE_LIMIT);

        spill_large_var(&dir, "task1", "mb", value.clone(), &mut pipeline);

        let inline = pipeline.vars.get("mb").unwrap();
        // The truncated portion must be valid UTF-8 (guaranteed by safe_end logic)
        assert!(inline.contains("...\n[truncated"));

        // Verify the full file content is intact
        let path_str = pipeline.vars.get("mb_path").unwrap();
        let on_disk = std::fs::read_to_string(path_str).unwrap();
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── spill_to_file tests ──────────────────────────────────────────

    #[test]
    fn spill_to_file_small_value_returns_none() {
        let dir = temp_dir("stf-small");
        let value = "short string";

        let result = spill_to_file(&dir, "task1", "key", value);
        assert!(result.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_exactly_at_limit_returns_none() {
        let dir = temp_dir("stf-exact");
        let value = "z".repeat(PIPELINE_VAR_INLINE_LIMIT);

        let result = spill_to_file(&dir, "task1", "key", &value);
        assert!(result.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_one_byte_over_returns_some() {
        let dir = temp_dir("stf-over");
        let value = "z".repeat(PIPELINE_VAR_INLINE_LIMIT + 1);

        let result = spill_to_file(&dir, "task1", "key", &value);
        assert!(result.is_some());

        let (truncated, path_str) = result.unwrap();
        assert!(truncated.starts_with("zzzz"));
        assert!(truncated.contains("...\n[truncated — full content at "));
        assert!(path_str.ends_with("key.txt"));

        // Verify file on disk
        let on_disk = std::fs::read_to_string(&path_str).unwrap();
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_large_value_truncated_format() {
        let dir = temp_dir("stf-fmt");
        let value = "A".repeat(PIPELINE_VAR_INLINE_LIMIT + 500);

        let (truncated, path_str) = spill_to_file(&dir, "task1", "output", &value).unwrap();

        // The truncated string should contain the marker text
        assert!(truncated.contains("...\n[truncated — full content at "));
        // The path in the truncated message should match the returned path
        assert!(truncated.contains(&path_str));
        // The truncated prefix should be exactly PIPELINE_VAR_INLINE_LIMIT bytes of 'A'
        let prefix = &truncated[..PIPELINE_VAR_INLINE_LIMIT];
        assert!(prefix.chars().all(|c| c == 'A'));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_multibyte_at_boundary() {
        let dir = temp_dir("stf-mb");
        // Create a value where a 3-byte UTF-8 char straddles the 4096 boundary.
        // 4095 ASCII bytes + "你好" (6 bytes) = 4101 total, exceeding the limit.
        // The char "你" starts at byte 4095 and ends at 4097, straddling the boundary.
        let mut value = "b".repeat(PIPELINE_VAR_INLINE_LIMIT - 1);
        value.push_str("你好世界你好世界"); // 24 more bytes

        let result = spill_to_file(&dir, "task1", "key", &value);
        assert!(result.is_some());

        let (truncated, _path_str) = result.unwrap();
        // The truncated text should be valid UTF-8 (it is a String, so guaranteed)
        // and should NOT split a multi-byte character
        let prefix_end = truncated.find("...\n[truncated").unwrap();
        let prefix = &truncated[..prefix_end];
        // The prefix should end before the multi-byte char since it can't fit
        // within the limit without splitting
        assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 1);
        assert!(prefix.chars().all(|c| c == 'b'));

        // Full content on disk should be intact
        let on_disk = std::fs::read_to_string(&_path_str).unwrap();
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_multibyte_fully_within_limit() {
        let dir = temp_dir("stf-mb2");
        // 4094 ASCII bytes + "你" (3 bytes) = 4097, just over the limit.
        // But the char boundary at 4094+3=4097 > 4096, so safe_end backs down to 4094.
        let mut value = "c".repeat(PIPELINE_VAR_INLINE_LIMIT - 2);
        value.push_str("你好世界"); // 12 bytes, total = 4094 + 12 = 4106

        let (truncated, _) = spill_to_file(&dir, "task1", "k", &value).unwrap();
        let prefix_end = truncated.find("...\n[truncated").unwrap();
        let prefix = &truncated[..prefix_end];
        // safe_end should back up to the start of the multibyte char
        // 4094 bytes of 'c', then "你" starts at 4094 and needs bytes 4094..4097
        // which exceeds the 4096 limit, so safe_end = 4094
        assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
