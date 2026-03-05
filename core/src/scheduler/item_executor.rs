use crate::config::{
    CaptureSource, ExecutionMode, ItemFinalizeContext, OnFailureAction, OnSuccessAction,
    PipelineVariables, PostAction, StepPrehookContext, TaskExecutionStep, TaskRuntimeContext,
    PIPELINE_VAR_INLINE_LIMIT,
};
use crate::events::insert_event;
use crate::prehook::{emit_item_finalize_event, evaluate_step_prehook};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::state::{read_agent_health, read_agent_metrics, write_agent_metrics, InnerState};
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
use tracing::warn;

use super::phase_runner::{
    run_phase, run_phase_with_rotation, shell_escape, PhaseRunRequest, RotatingPhaseRunRequest,
};
use super::safety::{execute_self_restart_step, execute_self_test_step, EXIT_RESTART};
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
    pub terminal: bool,
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
            terminal: false,
        }
    }

    pub fn merge_task_pipeline_vars(&mut self, task_pipeline_vars: &PipelineVariables) {
        for (key, value) in &task_pipeline_vars.vars {
            self.pipeline_vars
                .vars
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        if self.pipeline_vars.build_errors.is_empty() {
            self.pipeline_vars.build_errors = task_pipeline_vars.build_errors.clone();
        }
        if self.pipeline_vars.test_failures.is_empty() {
            self.pipeline_vars.test_failures = task_pipeline_vars.test_failures.clone();
        }
    }

    /// Collect all step IDs that match a capability, including canonical aliases.
    fn step_ids_for_capability(
        task_ctx: &TaskRuntimeContext,
        capability: &str,
        canonical_ids: &[&str],
    ) -> Vec<String> {
        let mut ids: Vec<String> = canonical_ids.iter().map(|s| s.to_string()).collect();
        for step in &task_ctx.execution_plan.steps {
            if step.required_capability.as_deref() == Some(capability) && !ids.contains(&step.id) {
                ids.push(step.id.clone());
            }
        }
        ids
    }

    /// Build a StepPrehookContext from accumulated state.
    pub fn to_prehook_context(
        &self,
        task_id: &str,
        item: &crate::dto::TaskItemRow,
        task_ctx: &TaskRuntimeContext,
        step_id: &str,
    ) -> StepPrehookContext {
        let qa_step_ids = Self::step_ids_for_capability(task_ctx, "qa", &["qa", "qa_testing"]);
        let fix_step_ids = Self::step_ids_for_capability(task_ctx, "fix", &["fix", "ticket_fix"]);
        let retest_step_ids = Self::step_ids_for_capability(task_ctx, "retest", &["retest"]);
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
            qa_exit_code: qa_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            fix_exit_code: fix_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            retest_exit_code: retest_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
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
        let qa_step_ids = Self::step_ids_for_capability(task_ctx, "qa", &["qa", "qa_testing"]);
        let fix_step_ids = Self::step_ids_for_capability(task_ctx, "fix", &["fix", "ticket_fix"]);
        let retest_step_ids = Self::step_ids_for_capability(task_ctx, "retest", &["retest"]);

        let qa_ran = qa_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let qa_skipped = qa_step_ids
            .iter()
            .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false));
        let qa_configured = task_ctx.execution_plan.steps.iter().any(|s| {
            qa_step_ids.contains(&s.id)
                && s.enabled
                && (s.repeatable || task_ctx.current_cycle <= 1)
        });
        let qa_observed = qa_ran || qa_skipped;
        let qa_enabled = qa_configured;
        let fix_ran = fix_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let fix_success = self.flags.get("fix_success").copied().unwrap_or(false);
        let fix_configured = task_ctx.execution_plan.steps.iter().any(|s| {
            fix_step_ids.contains(&s.id)
                && s.enabled
                && (s.repeatable || task_ctx.current_cycle <= 1)
        });
        let fix_enabled = fix_ran
            || fix_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false))
            || fix_configured;
        let retest_ran = retest_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let retest_success = self.flags.get("retest_success").copied().unwrap_or(false);
        let retest_enabled = retest_ran
            || retest_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false))
            || task_ctx
                .execution_plan
                .steps
                .iter()
                .any(|s| retest_step_ids.contains(&s.id) && s.enabled);

        ItemFinalizeContext {
            task_id: task_id.to_string(),
            task_item_id: item.id.clone(),
            cycle: task_ctx.current_cycle,
            qa_file_path: item.qa_file_path.clone(),
            item_status: self.item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code: qa_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            fix_exit_code: fix_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            retest_exit_code: retest_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            active_ticket_count: self.active_tickets.len() as i64,
            new_ticket_count: self.new_ticket_count,
            retest_new_ticket_count: 0,
            qa_failed: self.flags.get("qa_failed").copied().unwrap_or(false),
            fix_required: !self.active_tickets.is_empty(),
            qa_configured,
            qa_observed,
            qa_enabled,
            qa_ran,
            qa_skipped,
            fix_configured,
            fix_enabled,
            fix_ran,
            fix_skipped: fix_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false)),
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
                    self.exit_codes
                        .insert(step_id.to_string(), result.exit_code);
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

fn is_execution_hard_failure(result: &crate::dto::RunResult) -> bool {
    result.validation_status == "failed"
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
                step_scope: step.resolved_scope(),
                prompt_delivery: crate::config::PromptDelivery::Arg,
                prompt_payload: None,
                pipe_stdin: false,
            },
        )
        .await?
    } else {
        let resolved_prompt = step.template.as_ref().and_then(|tmpl_name| {
            let cfg = state.active_config.read().ok()?;
            cfg.config
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
                workspace_root: &task_ctx.workspace_root,
                workspace_id: &task_ctx.workspace_id,
                cycle: task_ctx.current_cycle,
                runtime,
                pipeline_vars: Some(&task_ctx.pipeline_vars),
                step_timeout_secs: task_ctx.safety.step_timeout_secs,
                step_scope: step.resolved_scope(),
                step_template_prompt: resolved_prompt.as_deref(),
                project_id: &task_ctx.project_id,
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
    if let ExecutionMode::Builtin { name } = step.effective_execution_mode().as_ref() {
        if name == "loop_guard" {
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

pub struct ProcessItemRequest<'a> {
    pub task_id: &'a str,
    pub item: &'a crate::dto::TaskItemRow,
    pub task_item_paths: &'a [String],
    pub task_ctx: &'a TaskRuntimeContext,
    pub runtime: &'a RunningTask,
    pub step_filter: Option<&'a HashSet<String>>,
}

pub async fn process_item(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_item_paths: &[String],
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<()> {
    let mut acc = StepExecutionAccumulator::new(task_ctx.pipeline_vars.clone());
    process_item_filtered(
        state,
        ProcessItemRequest {
            task_id,
            item,
            task_item_paths,
            task_ctx,
            runtime,
            step_filter: None,
        },
        &mut acc,
    )
    .await?;
    finalize_item_execution(state, task_id, item, task_ctx, &mut acc)?;
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
    request: ProcessItemRequest<'_>,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let ProcessItemRequest {
        task_id,
        item,
        task_item_paths,
        task_ctx,
        runtime,
        step_filter,
    } = request;
    let item_id = item.id.as_str();
    let should_run_step =
        |step_id: &str| -> bool { step_filter.map_or(true, |f| f.contains(step_id)) };
    acc.merge_task_pipeline_vars(&task_ctx.pipeline_vars);
    // ── Unified step loop ────────────────────────────────────────────

    for step in &task_ctx.execution_plan.steps {
        // Check for pause/stop between steps
        if runtime.stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if super::task_state::is_task_paused_in_db(state, task_id)? {
            return Ok(());
        }
        if acc.terminal {
            return Ok(());
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
                json!({"step": phase, "step_id": &step.id, "step_scope": step.resolved_scope(), "reason": "prehook_false"}),
            )?;
            continue;
        }

        // 2. Execute
        if acc.step_ran.is_empty() {
            state.db_writer.mark_task_item_running(item_id)?;
        }
        let pipeline_var_keys: Vec<&String> = acc.pipeline_vars.vars.keys().collect();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_started",
            json!({"step": phase, "step_id": &step.id, "step_scope": step.resolved_scope(), "cycle": task_ctx.current_cycle, "pipeline_var_keys": pipeline_var_keys}),
        )?;

        // Layer 2 defense: delegate to the consolidated method on TaskExecutionStep.
        // If `step.builtin` names a known builtin, the method returns Builtin regardless
        // of what `behavior.execution` says, making dispatch robust against stale JSON.
        let effective_execution = step.effective_execution_mode();

        let result = match effective_execution.as_ref() {
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
                    json!({"step": phase, "step_scope": step.resolved_scope(), "exit_code": exit_code, "success": passed}),
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
                            acc.terminal = true;
                            return Ok(());
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

            ExecutionMode::Builtin { name } if name == "self_restart" => {
                // Self-restart builtin: rebuild, verify, snapshot, then exit for relaunch
                let ws_root = std::path::Path::new(&task_ctx.workspace_root);
                let exit_code =
                    execute_self_restart_step(ws_root, state, task_id, item_id)
                        .await
                        .unwrap_or(1);

                acc.pipeline_vars
                    .vars
                    .insert("self_restart_exit_code".to_string(), exit_code.to_string());

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": phase, "step_scope": step.resolved_scope(), "exit_code": exit_code, "restart": exit_code == EXIT_RESTART}),
                )?;

                if exit_code == EXIT_RESTART {
                    // All state is persisted (restart_pending set by execute_self_restart_step).
                    // Exit process so the wrapper script (orchestrator.sh) relaunches the new binary.
                    std::process::exit(EXIT_RESTART as i32);
                }

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
                            return Ok(());
                        }
                    }
                }
                acc.step_ran.insert(step.id.clone(), true);
                acc.exit_codes.insert(step.id.clone(), exit_code as i64);
                continue;
            }

            ExecutionMode::Builtin { name } if name == "ticket_scan" => {
                // Ticket scan builtin (step_started already emitted above)
                let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                acc.active_tickets = tickets.get(&item.qa_file_path).cloned().unwrap_or_default();
                acc.new_ticket_count = acc.active_tickets.len() as i64;
                acc.step_ran.insert(step.id.clone(), true);
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step": "ticket_scan", "step_scope": step.resolved_scope(), "tickets": acc.active_tickets.len()}),
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
                        json!({"step": phase, "step_scope": step.resolved_scope(), "chain_step": chain_step.id}),
                    )?;

                    let mut step_ctx = task_ctx.clone();
                    step_ctx.pipeline_vars = acc.pipeline_vars.clone();

                    let chain_exec = execute_builtin_step(
                        state,
                        task_id,
                        item_id,
                        chain_step,
                        &step_ctx,
                        runtime,
                        &item.qa_file_path,
                    )
                    .await;

                    let (chain_result, new_pipeline) = match chain_exec {
                        Ok(val) => val,
                        Err(e) => {
                            let _ = insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "chain_step_finished",
                                json!({"step": phase, "step_scope": step.resolved_scope(), "chain_step": chain_step.id, "error": e.to_string(), "success": false}),
                            );
                            let _ = insert_event(
                                state,
                                task_id,
                                Some(item_id),
                                "step_finished",
                                json!({"step": phase, "step_scope": step.resolved_scope(), "error": e.to_string(), "success": false}),
                            );
                            return Err(e);
                        }
                    };
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
                            "step_scope": step.resolved_scope(),
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
                    json!({"step": phase, "step_scope": step.resolved_scope(), "success": chain_passed}),
                )?;
                continue;
            }

            // ExecutionMode::Agent or ExecutionMode::Builtin for generic builtins
            _ => {
                let mut step_ctx = task_ctx.clone();
                step_ctx.pipeline_vars = acc.pipeline_vars.clone();

                let exec_result = execute_builtin_step(
                    state,
                    task_id,
                    item_id,
                    step,
                    &step_ctx,
                    runtime,
                    &item.qa_file_path,
                )
                .await;

                let (result, new_pipeline) = match exec_result {
                    Ok(val) => val,
                    Err(e) => {
                        let _ = insert_event(
                            state,
                            task_id,
                            Some(item_id),
                            "step_finished",
                            json!({"step": phase, "step_id": step.id, "step_scope": step.resolved_scope(), "error": e.to_string(), "success": false}),
                        );
                        return Err(e);
                    }
                };
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
                    acc.terminal = true;
                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "step_finished",
                        json!({"step": phase, "step_id": step.id, "step_scope": step.resolved_scope(), "early_return": true, "exit_code": result.exit_code, "success": false}),
                    )?;
                    return Ok(());
                }
            }
        }

        // 5. Post-actions
        for action in &step.behavior.post_actions {
            match action {
                PostAction::CreateTicket if !result.is_success() => {
                    if let Some(exit_code) = acc.exit_codes.get(&step.id) {
                        let task_name = SqliteTaskRepository::new(state.database.clone())
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
                            Err(e) => warn!(error = %e, "failed to auto-create ticket"),
                        }
                    }
                }
                PostAction::ScanTickets => {
                    let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
                    acc.active_tickets =
                        tickets.get(&item.qa_file_path).cloned().unwrap_or_default();
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
                    "step_scope": step.resolved_scope(),
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

        if is_execution_hard_failure(&result) {
            acc.item_status = "unresolved".to_string();
            acc.flags.insert("execution_failed".to_string(), true);
            acc.terminal = true;
            return Ok(());
        }
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
                json!({"step_id": ds.id, "step_type": ds.step_type, "step_scope": "item", "priority": ds.priority}),
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
                    step_scope: crate::config::StepScope::Item,
                    step_template_prompt: None,
                    project_id: &task_ctx.project_id,
                },
            )
            .await?;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "dynamic_step_finished",
                json!({"step_id": ds.id, "step_scope": "item", "exit_code": result.exit_code, "success": result.is_success()}),
            )?;
        }
    }

    Ok(())
}

pub fn finalize_item_execution(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &crate::dto::TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let item_id = item.id.as_str();

    // Seed active tickets from existing ticket files if no scan step ran
    if acc.active_tickets.is_empty() && !acc.step_ran.contains_key("ticket_scan") {
        acc.active_tickets = list_existing_tickets_for_item(task_ctx, &item.qa_file_path)?;
        acc.new_ticket_count = acc.active_tickets.len() as i64;
    }

    let finalize_context = acc.to_finalize_context(task_id, item, task_ctx);
    if finalize_context.is_last_cycle
        && finalize_context.qa_configured
        && !finalize_context.qa_observed
    {
        acc.item_status = "unresolved".to_string();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "item_validation_missing",
            json!({
                "step": "qa_testing",
                "reason": "configured qa step was neither run nor skipped in final cycle"
            }),
        )?;
    } else if acc.flags.get("execution_failed").copied().unwrap_or(false) {
        acc.item_status = "unresolved".to_string();
    } else if let Some(outcome) = crate::prehook::resolve_workflow_finalize_outcome(
        &task_ctx.execution_plan.finalize,
        &finalize_context,
    )? {
        acc.item_status = outcome.status.clone();
        emit_item_finalize_event(state, &finalize_context, &outcome)?;
    }

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
        .set_task_item_terminal_status(item_id, &acc.item_status)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CaptureDecl, CaptureSource, ExecutionMode, PipelineVariables, StepBehavior};
    use std::collections::HashMap;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("item-exec-test-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create item executor temp dir");
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

    #[test]
    fn execution_hard_failure_detects_failed_validation_status() {
        let result = crate::dto::RunResult {
            success: false,
            exit_code: -6,
            stdout_path: String::new(),
            stderr_path: String::new(),
            timed_out: false,
            duration_ms: None,
            output: None,
            validation_status: "failed".to_string(),
            agent_id: "agent".to_string(),
            run_id: "run".to_string(),
        };

        assert!(is_execution_hard_failure(&result));
    }

    #[test]
    fn execution_hard_failure_ignores_non_validation_failures() {
        let result = crate::dto::RunResult {
            success: false,
            exit_code: 1,
            stdout_path: String::new(),
            stderr_path: String::new(),
            timed_out: false,
            duration_ms: None,
            output: None,
            validation_status: "passed".to_string(),
            agent_id: "agent".to_string(),
            run_id: "run".to_string(),
        };

        assert!(!is_execution_hard_failure(&result));
    }

    // ── spill_large_var tests ────────────────────────────────────────

    #[test]
    fn spill_large_var_small_value_inserts_inline() {
        let dir = temp_dir("slv-small");
        let mut pipeline = empty_pipeline();
        let value = "hello world".to_string();

        spill_large_var(&dir, "task1", "stdout", value.clone(), &mut pipeline);

        assert_eq!(
            pipeline.vars.get("stdout").expect("stdout should be set"),
            "hello world"
        );
        // _path is always set now (even for small values)
        let p = pipeline
            .vars
            .get("stdout_path")
            .expect("stdout_path must be set");
        assert_eq!(
            std::fs::read_to_string(p).expect("read stdout spill file"),
            "hello world"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_exactly_at_limit_inserts_inline() {
        let dir = temp_dir("slv-exact");
        let mut pipeline = empty_pipeline();
        let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT);

        spill_large_var(&dir, "task1", "out", value.clone(), &mut pipeline);

        assert_eq!(pipeline.vars.get("out").expect("out should be set"), &value);
        // _path is always set now (even for small values)
        let p = pipeline.vars.get("out_path").expect("out_path must be set");
        assert_eq!(
            std::fs::read_to_string(p).expect("read out spill file"),
            value
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_one_byte_over_limit_spills_to_file() {
        let dir = temp_dir("slv-over");
        let mut pipeline = empty_pipeline();
        let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT + 1);

        spill_large_var(&dir, "task1", "big", value.clone(), &mut pipeline);

        // Inline value should be truncated with the marker
        let inline = pipeline.vars.get("big").expect("big should be set");
        assert!(inline.contains("...\n[truncated — full content at "));
        // The inline prefix (before the marker) should be at most PIPELINE_VAR_INLINE_LIMIT bytes
        let prefix_end = inline
            .find("...\n[truncated")
            .expect("truncation marker should exist");
        assert!(prefix_end <= PIPELINE_VAR_INLINE_LIMIT);

        // Companion path variable should exist
        let path_str = pipeline
            .vars
            .get("big_path")
            .expect("big_path should be set");
        let spill_path = std::path::Path::new(path_str);
        assert!(spill_path.exists());

        // File should contain the full original value
        let on_disk = std::fs::read_to_string(spill_path).expect("read spilled big value");
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_large_var_large_value_sets_correct_path_key() {
        let dir = temp_dir("slv-pathkey");
        let mut pipeline = empty_pipeline();
        let value = "y".repeat(PIPELINE_VAR_INLINE_LIMIT + 100);

        spill_large_var(&dir, "t42", "my_key", value, &mut pipeline);

        let path_str = pipeline
            .vars
            .get("my_key_path")
            .expect("my_key_path should be set");
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

        let inline = pipeline.vars.get("mb").expect("mb should be set");
        // The truncated portion must be valid UTF-8 (guaranteed by safe_end logic)
        assert!(inline.contains("...\n[truncated"));

        // Verify the full file content is intact
        let path_str = pipeline.vars.get("mb_path").expect("mb_path should be set");
        let on_disk = std::fs::read_to_string(path_str).expect("read multibyte spill file");
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

        let (truncated, path_str) = result.expect("spill should occur");
        assert!(truncated.starts_with("zzzz"));
        assert!(truncated.contains("...\n[truncated — full content at "));
        assert!(path_str.ends_with("key.txt"));

        // Verify file on disk
        let on_disk = std::fs::read_to_string(&path_str).expect("read spilled file");
        assert_eq!(on_disk, value);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn spill_to_file_large_value_truncated_format() {
        let dir = temp_dir("stf-fmt");
        let value = "A".repeat(PIPELINE_VAR_INLINE_LIMIT + 500);

        let (truncated, path_str) =
            spill_to_file(&dir, "task1", "output", &value).expect("spill should occur");

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

        let (truncated, _path_str) = result.expect("spill should occur");
        // The truncated text should be valid UTF-8 (it is a String, so guaranteed)
        // and should NOT split a multi-byte character
        let prefix_end = truncated
            .find("...\n[truncated")
            .expect("truncation marker should exist");
        let prefix = &truncated[..prefix_end];
        // The prefix should end before the multi-byte char since it can't fit
        // within the limit without splitting
        assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 1);
        assert!(prefix.chars().all(|c| c == 'b'));

        // Full content on disk should be intact
        let on_disk = std::fs::read_to_string(&_path_str).expect("read spilled multibyte file");
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

        let (truncated, _) = spill_to_file(&dir, "task1", "k", &value).expect("spill should occur");
        let prefix_end = truncated
            .find("...\n[truncated")
            .expect("truncation marker should exist");
        let prefix = &truncated[..prefix_end];
        // safe_end should back up to the start of the multibyte char
        // 4094 bytes of 'c', then "你" starts at 4094 and needs bytes 4094..4097
        // which exceeds the 4096 limit, so safe_end = 4094
        assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Layer-2 dispatch guard tests ─────────────────────────────────

    fn make_step(
        id: &str,
        builtin: Option<&str>,
        execution: ExecutionMode,
    ) -> crate::config::TaskExecutionStep {
        crate::config::TaskExecutionStep {
            id: id.to_string(),
            builtin: builtin.map(|s| s.to_string()),
            required_capability: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior {
                execution,
                ..StepBehavior::default()
            },
        }
    }

    #[test]
    fn builtin_guard_routes_self_test_regardless_of_execution_mode() {
        // Step has stale Agent execution mode but builtin field is authoritative.
        let step = make_step("self_test", Some("self_test"), ExecutionMode::Agent);
        assert_eq!(
            step.effective_execution_mode().as_ref(),
            &ExecutionMode::Builtin {
                name: "self_test".to_string()
            },
            "dispatch guard must resolve self_test builtin even when behavior.execution is Agent"
        );
    }

    #[test]
    fn builtin_guard_noop_for_agent_step() {
        // Pure agent step (no builtin field) stays as Agent.
        let step = make_step("plan", None, ExecutionMode::Agent);
        assert_eq!(
            step.effective_execution_mode().as_ref(),
            &ExecutionMode::Agent
        );
    }

    #[test]
    fn builtin_guard_noop_when_already_correct() {
        // Step already has correct Builtin execution mode — guard is a no-op.
        let step = make_step(
            "self_test",
            Some("self_test"),
            ExecutionMode::Builtin {
                name: "self_test".to_string(),
            },
        );
        assert_eq!(
            step.effective_execution_mode().as_ref(),
            &ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
    }

    // ── StepExecutionAccumulator tests ──────────────────────────────

    fn make_task_ctx(steps: Vec<crate::config::TaskExecutionStep>, max_cycles: Option<u32>, current_cycle: u32) -> crate::config::TaskRuntimeContext {
        crate::config::TaskRuntimeContext {
            workspace_id: "default".to_string(),
            workspace_root: std::path::PathBuf::from("/tmp/test"),
            ticket_dir: "/tmp/test/docs/ticket".to_string(),
            execution_plan: crate::config::TaskExecutionPlan {
                steps,
                loop_policy: crate::config::WorkflowLoopConfig {
                    mode: crate::config::LoopMode::Fixed,
                    guard: crate::config::WorkflowLoopGuardConfig {
                        enabled: true,
                        stop_when_no_unresolved: true,
                        max_cycles,
                        agent_template: None,
                    },
                },
                finalize: Default::default(),
            },
            current_cycle,
            init_done: false,
            dynamic_steps: vec![],
            pipeline_vars: empty_pipeline(),
            safety: Default::default(),
            self_referential: false,
            consecutive_failures: 0,
            project_id: String::new(),
        }
    }

    fn make_item(id: &str, qa_file: &str) -> crate::dto::TaskItemRow {
        crate::dto::TaskItemRow {
            id: id.to_string(),
            qa_file_path: qa_file.to_string(),
        }
    }

    fn make_run_result(exit_code: i64, success: bool, output: Option<crate::collab::AgentOutput>) -> crate::dto::RunResult {
        crate::dto::RunResult {
            success,
            exit_code,
            stdout_path: String::new(),
            stderr_path: String::new(),
            timed_out: false,
            duration_ms: None,
            output,
            validation_status: if success { "passed" } else { "failed" }.to_string(),
            agent_id: "test-agent".to_string(),
            run_id: "run-1".to_string(),
        }
    }

    // ── new() ────────────────────────────────

    #[test]
    fn accumulator_new_initializes_with_pending_status() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        assert_eq!(acc.item_status, "pending");
        assert!(acc.active_tickets.is_empty());
        assert!(acc.flags.is_empty());
        assert!(acc.exit_codes.is_empty());
        assert!(!acc.terminal);
        assert_eq!(acc.new_ticket_count, 0);
        assert!(acc.qa_confidence.is_none());
    }

    // ── merge_task_pipeline_vars() ───────────

    #[test]
    fn merge_task_pipeline_vars_does_not_overwrite_existing() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.pipeline_vars.vars.insert("key".to_string(), "item_value".to_string());

        let mut task_vars = empty_pipeline();
        task_vars.vars.insert("key".to_string(), "task_value".to_string());
        task_vars.vars.insert("new_key".to_string(), "new_value".to_string());

        acc.merge_task_pipeline_vars(&task_vars);

        assert_eq!(acc.pipeline_vars.vars.get("key").unwrap(), "item_value");
        assert_eq!(acc.pipeline_vars.vars.get("new_key").unwrap(), "new_value");
    }

    #[test]
    fn merge_task_pipeline_vars_copies_build_errors_when_empty() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let mut task_vars = empty_pipeline();
        task_vars.build_errors = vec![crate::config::BuildError {
            file: Some("main.rs".to_string()),
            line: Some(10),
            column: None,
            message: "error".to_string(),
            level: crate::config::BuildErrorLevel::Error,
        }];
        task_vars.test_failures = vec![crate::config::TestFailure {
            test_name: "test1".to_string(),
            message: "failed".to_string(),
            file: None,
            line: None,
            stdout: None,
        }];

        acc.merge_task_pipeline_vars(&task_vars);

        assert_eq!(acc.pipeline_vars.build_errors.len(), 1);
        assert_eq!(acc.pipeline_vars.test_failures.len(), 1);
    }

    #[test]
    fn merge_task_pipeline_vars_preserves_existing_build_errors() {
        let mut pipeline = empty_pipeline();
        pipeline.build_errors = vec![crate::config::BuildError {
            file: Some("existing.rs".to_string()),
            line: None,
            column: None,
            message: "existing error".to_string(),
            level: crate::config::BuildErrorLevel::Error,
        }];
        let mut acc = StepExecutionAccumulator::new(pipeline);

        let mut task_vars = empty_pipeline();
        task_vars.build_errors = vec![crate::config::BuildError {
            file: Some("new.rs".to_string()),
            line: None,
            column: None,
            message: "new error".to_string(),
            level: crate::config::BuildErrorLevel::Error,
        }];

        acc.merge_task_pipeline_vars(&task_vars);

        assert_eq!(acc.pipeline_vars.build_errors.len(), 1);
        assert_eq!(acc.pipeline_vars.build_errors[0].file, Some("existing.rs".to_string()));
    }

    // ── apply_captures() ─────────────────────

    #[test]
    fn apply_captures_exit_code() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "qa_exit".to_string(),
            source: CaptureSource::ExitCode,
        }];
        let result = make_run_result(42, false, None);

        acc.apply_captures(&captures, "qa_testing", &result);

        assert_eq!(*acc.exit_codes.get("qa_testing").unwrap(), 42);
        assert_eq!(acc.pipeline_vars.vars.get("qa_exit").unwrap(), "42");
    }

    #[test]
    fn apply_captures_failed_flag() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "qa_failed".to_string(),
            source: CaptureSource::FailedFlag,
        }];
        let result = make_run_result(1, false, None);

        acc.apply_captures(&captures, "qa", &result);

        assert_eq!(*acc.flags.get("qa_failed").unwrap(), true);
        assert_eq!(acc.pipeline_vars.vars.get("qa_failed").unwrap(), "true");
    }

    #[test]
    fn apply_captures_success_flag() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "fix_success".to_string(),
            source: CaptureSource::SuccessFlag,
        }];
        let result = make_run_result(0, true, None);

        acc.apply_captures(&captures, "fix", &result);

        assert_eq!(*acc.flags.get("fix_success").unwrap(), true);
        assert_eq!(acc.pipeline_vars.vars.get("fix_success").unwrap(), "true");
    }

    #[test]
    fn apply_captures_success_flag_on_failure() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "fix_success".to_string(),
            source: CaptureSource::SuccessFlag,
        }];
        let result = make_run_result(1, false, None);

        acc.apply_captures(&captures, "fix", &result);

        assert_eq!(*acc.flags.get("fix_success").unwrap(), false);
    }

    #[test]
    fn apply_captures_stderr() {
        let output = crate::collab::AgentOutput {
            run_id: uuid::Uuid::new_v4(),
            agent_id: "a".to_string(),
            phase: "qa".to_string(),
            exit_code: 0,
            stdout: "out content".to_string(),
            stderr: "err content".to_string(),
            artifacts: vec![],
            metrics: Default::default(),
            confidence: 0.0,
            quality_score: 0.0,
            created_at: chrono::Utc::now(),
            build_errors: vec![],
            test_failures: vec![],
        };
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "qa_stderr".to_string(),
            source: CaptureSource::Stderr,
        }];
        let result = make_run_result(0, true, Some(output));

        acc.apply_captures(&captures, "qa", &result);

        assert_eq!(acc.pipeline_vars.vars.get("qa_stderr").unwrap(), "err content");
    }

    #[test]
    fn apply_captures_stdout_no_output_is_noop() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "qa_stdout".to_string(),
            source: CaptureSource::Stdout,
        }];
        let result = make_run_result(0, true, None);

        acc.apply_captures(&captures, "qa", &result);

        assert!(!acc.pipeline_vars.vars.contains_key("qa_stdout"));
    }

    #[test]
    fn apply_captures_stderr_no_output_is_noop() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![CaptureDecl {
            var: "qa_stderr".to_string(),
            source: CaptureSource::Stderr,
        }];
        let result = make_run_result(0, true, None);

        acc.apply_captures(&captures, "qa", &result);

        assert!(!acc.pipeline_vars.vars.contains_key("qa_stderr"));
    }

    #[test]
    fn apply_captures_multiple() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        let captures = vec![
            CaptureDecl { var: "exit".to_string(), source: CaptureSource::ExitCode },
            CaptureDecl { var: "failed".to_string(), source: CaptureSource::FailedFlag },
            CaptureDecl { var: "ok".to_string(), source: CaptureSource::SuccessFlag },
        ];
        let result = make_run_result(0, true, None);

        acc.apply_captures(&captures, "step1", &result);

        assert_eq!(acc.pipeline_vars.vars.get("exit").unwrap(), "0");
        assert_eq!(*acc.flags.get("failed").unwrap(), false);
        assert_eq!(*acc.flags.get("ok").unwrap(), true);
    }

    // ── to_prehook_context() ─────────────────

    #[test]
    fn to_prehook_context_basic_fields() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "docs/qa/test.md");
        let ctx = make_task_ctx(vec![], Some(2), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "qa_testing");

        assert_eq!(phc.task_id, "task-1");
        assert_eq!(phc.task_item_id, "item-1");
        assert_eq!(phc.cycle, 1);
        assert_eq!(phc.step, "qa_testing");
        assert_eq!(phc.qa_file_path, "docs/qa/test.md");
        assert_eq!(phc.item_status, "pending");
        assert_eq!(phc.task_status, "running");
        assert_eq!(phc.max_cycles, 2);
        assert!(!phc.is_last_cycle);
    }

    #[test]
    fn to_prehook_context_is_last_cycle_when_current_equals_max() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "docs/qa/test.md");
        let ctx = make_task_ctx(vec![], Some(2), 2);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert!(phc.is_last_cycle);
    }

    #[test]
    fn to_prehook_context_exit_codes_from_canonical_step_ids() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.exit_codes.insert("qa_testing".to_string(), 1);
        acc.exit_codes.insert("ticket_fix".to_string(), 0);
        acc.exit_codes.insert("retest".to_string(), 2);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert_eq!(phc.qa_exit_code, Some(1));
        assert_eq!(phc.fix_exit_code, Some(0));
        assert_eq!(phc.retest_exit_code, Some(2));
    }

    #[test]
    fn to_prehook_context_exit_codes_use_first_alias_match() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        // "qa" is the first canonical alias for qa capability
        acc.exit_codes.insert("qa".to_string(), 5);
        acc.exit_codes.insert("qa_testing".to_string(), 10);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert_eq!(phc.qa_exit_code, Some(5));
    }

    #[test]
    fn to_prehook_context_qa_failed_and_fix_required() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.flags.insert("qa_failed".to_string(), true);
        acc.active_tickets.push("ticket1.md".to_string());

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert!(phc.qa_failed);
        assert!(phc.fix_required);
        assert_eq!(phc.active_ticket_count, 1);
    }

    #[test]
    fn to_prehook_context_fix_required_from_tickets_even_without_qa_failed() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.active_tickets.push("ticket1.md".to_string());

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert!(!phc.qa_failed);
        assert!(phc.fix_required); // fix_required = qa_failed || !active_tickets.is_empty()
    }

    #[test]
    fn to_prehook_context_self_test_vars() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.pipeline_vars.vars.insert("self_test_exit_code".to_string(), "0".to_string());
        acc.pipeline_vars.vars.insert("self_test_passed".to_string(), "true".to_string());

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert_eq!(phc.self_test_exit_code, Some(0));
        assert!(phc.self_test_passed);
    }

    #[test]
    fn to_prehook_context_build_test_counts() {
        let mut pipeline = empty_pipeline();
        pipeline.build_errors = vec![crate::config::BuildError {
            file: Some("f.rs".to_string()), line: None, column: None,
            message: "err".to_string(), level: crate::config::BuildErrorLevel::Error,
        }];
        pipeline.test_failures = vec![
            crate::config::TestFailure { test_name: "t1".to_string(), message: "fail".to_string(), file: None, line: None, stdout: None },
            crate::config::TestFailure { test_name: "t2".to_string(), message: "fail".to_string(), file: None, line: None, stdout: None },
        ];
        let acc = StepExecutionAccumulator::new(pipeline);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert_eq!(phc.build_error_count, 1);
        assert_eq!(phc.test_failure_count, 2);
    }

    #[test]
    fn to_prehook_context_capability_based_step_ids() {
        let steps = vec![
            make_step("custom_qa", None, ExecutionMode::Agent),
        ];
        // Give the step a qa capability
        let mut steps = steps;
        steps[0].required_capability = Some("qa".to_string());

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.exit_codes.insert("custom_qa".to_string(), 3);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        // canonical aliases "qa", "qa_testing" come first, then capability match "custom_qa"
        // Since neither "qa" nor "qa_testing" have exit codes, it falls through to "custom_qa"
        assert_eq!(phc.qa_exit_code, Some(3));
    }

    #[test]
    fn to_prehook_context_max_cycles_defaults_to_1() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], None, 1);

        let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

        assert_eq!(phc.max_cycles, 1);
        assert!(phc.is_last_cycle);
    }

    // ── to_finalize_context() ────────────────

    #[test]
    fn to_finalize_context_basic_fields() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "docs/qa/test.md");
        let ctx = make_task_ctx(vec![], Some(2), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert_eq!(fc.task_id, "task-1");
        assert_eq!(fc.task_item_id, "item-1");
        assert_eq!(fc.cycle, 1);
        assert_eq!(fc.qa_file_path, "docs/qa/test.md");
        assert_eq!(fc.item_status, "pending");
        assert!(!fc.is_last_cycle);
    }

    #[test]
    fn to_finalize_context_qa_ran_and_configured() {
        let steps = vec![{
            let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s
        }];

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.step_ran.insert("qa_testing".to_string(), true);
        acc.exit_codes.insert("qa_testing".to_string(), 0);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(fc.qa_ran);
        assert!(fc.qa_configured);
        assert!(fc.qa_observed);
        assert!(fc.qa_enabled);
        assert!(!fc.qa_skipped);
    }

    #[test]
    fn to_finalize_context_qa_skipped() {
        let steps = vec![{
            let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s
        }];

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.step_skipped.insert("qa_testing".to_string(), true);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(!fc.qa_ran);
        assert!(fc.qa_configured);
        assert!(fc.qa_observed);
        assert!(fc.qa_skipped);
    }

    #[test]
    fn to_finalize_context_fix_ran_and_success() {
        let steps = vec![{
            let mut s = make_step("ticket_fix", None, ExecutionMode::Agent);
            s.required_capability = Some("fix".to_string());
            s
        }];

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.step_ran.insert("ticket_fix".to_string(), true);
        acc.flags.insert("fix_success".to_string(), true);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(fc.fix_ran);
        assert!(fc.fix_success);
        assert!(fc.fix_configured);
        assert!(fc.fix_enabled);
    }

    #[test]
    fn to_finalize_context_fix_skipped() {
        let steps = vec![{
            let mut s = make_step("ticket_fix", None, ExecutionMode::Agent);
            s.required_capability = Some("fix".to_string());
            s
        }];

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.step_skipped.insert("ticket_fix".to_string(), true);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(!fc.fix_ran);
        assert!(fc.fix_skipped);
        assert!(fc.fix_enabled);
    }

    #[test]
    fn to_finalize_context_retest() {
        let steps = vec![{
            let mut s = make_step("retest", None, ExecutionMode::Agent);
            s.required_capability = Some("retest".to_string());
            s
        }];

        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.step_ran.insert("retest".to_string(), true);
        acc.flags.insert("retest_success".to_string(), true);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(fc.retest_ran);
        assert!(fc.retest_success);
        assert!(fc.retest_enabled);
    }

    #[test]
    fn to_finalize_context_not_repeatable_in_cycle_2() {
        let steps = vec![{
            let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s.repeatable = false;
            s
        }];

        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(2), 2);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        // Non-repeatable step in cycle 2 is not qa_configured
        assert!(!fc.qa_configured);
    }

    #[test]
    fn to_finalize_context_disabled_step_not_configured() {
        let steps = vec![{
            let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s.enabled = false;
            s
        }];

        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(steps, Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(!fc.qa_configured);
    }

    #[test]
    fn to_finalize_context_tickets_set_fix_required() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.active_tickets = vec!["ticket1.md".to_string(), "ticket2.md".to_string()];
        acc.new_ticket_count = 2;

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert!(fc.fix_required);
        assert_eq!(fc.active_ticket_count, 2);
        assert_eq!(fc.new_ticket_count, 2);
    }

    #[test]
    fn to_finalize_context_confidence_and_quality() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.qa_confidence = Some(0.85);
        acc.qa_quality_score = Some(0.9);
        acc.fix_confidence = Some(0.7);
        acc.fix_quality_score = Some(0.8);

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert_eq!(fc.qa_confidence, Some(0.85));
        assert_eq!(fc.qa_quality_score, Some(0.9));
        assert_eq!(fc.fix_confidence, Some(0.7));
        assert_eq!(fc.fix_quality_score, Some(0.8));
    }

    #[test]
    fn to_finalize_context_artifacts() {
        let mut acc = StepExecutionAccumulator::new(empty_pipeline());
        acc.phase_artifacts.push(crate::collab::Artifact::new(
            crate::collab::ArtifactKind::Ticket {
                severity: crate::collab::artifact::Severity::Medium,
                category: "qa".to_string(),
            },
        ));
        acc.phase_artifacts.push(crate::collab::Artifact::new(
            crate::collab::ArtifactKind::CodeChange {
                files: vec!["f.rs".to_string()],
            },
        ));

        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(1), 1);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);

        assert_eq!(fc.total_artifacts, 2);
        assert!(fc.has_ticket_artifacts);
        assert!(fc.has_code_change_artifacts);
    }

    #[test]
    fn to_finalize_context_is_last_cycle() {
        let acc = StepExecutionAccumulator::new(empty_pipeline());
        let item = make_item("item-1", "test.md");
        let ctx = make_task_ctx(vec![], Some(2), 2);

        let fc = acc.to_finalize_context("task-1", &item, &ctx);
        assert!(fc.is_last_cycle);

        let ctx2 = make_task_ctx(vec![], Some(2), 1);
        let fc2 = acc.to_finalize_context("task-1", &item, &ctx2);
        assert!(!fc2.is_last_cycle);
    }

    // ── step_ids_for_capability() ────────────

    #[test]
    fn step_ids_for_capability_includes_canonical_and_custom() {
        let steps = vec![
            {
                let mut s = make_step("my_qa_step", None, ExecutionMode::Agent);
                s.required_capability = Some("qa".to_string());
                s
            },
            {
                let mut s = make_step("unrelated", None, ExecutionMode::Agent);
                s.required_capability = Some("fix".to_string());
                s
            },
        ];
        let ctx = make_task_ctx(steps, Some(1), 1);

        let ids = StepExecutionAccumulator::step_ids_for_capability(
            &ctx, "qa", &["qa", "qa_testing"],
        );

        assert_eq!(ids, vec!["qa", "qa_testing", "my_qa_step"]);
    }

    #[test]
    fn step_ids_for_capability_no_duplicates_for_canonical_names() {
        let steps = vec![{
            let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s
        }];
        let ctx = make_task_ctx(steps, Some(1), 1);

        let ids = StepExecutionAccumulator::step_ids_for_capability(
            &ctx, "qa", &["qa", "qa_testing"],
        );

        // "qa_testing" is already in canonical list, should not be duplicated
        assert_eq!(ids, vec!["qa", "qa_testing"]);
    }
}
