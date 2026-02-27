use crate::config::{
    ItemFinalizeContext, PipelineVariables, StepPrehookContext, TaskExecutionStep,
    TaskRuntimeContext, WorkflowStepType,
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
use std::collections::HashSet;
use std::sync::Arc;

use super::phase_runner::{
    run_phase, run_phase_with_rotation, shell_escape, PhaseRunRequest, RotatingPhaseRunRequest,
};
use super::safety::execute_self_test_step;
use super::task_state::count_unresolved_items;
use super::RunningTask;

pub async fn execute_builtin_step(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step: &TaskExecutionStep,
    task_ctx: &TaskRuntimeContext,
    runtime: &RunningTask,
) -> Result<(crate::dto::RunResult, PipelineVariables)> {
    let phase = step
        .step_type
        .as_ref()
        .map(|t| t.as_str())
        .unwrap_or(&step.id);

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
                rel_path: ".",
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
        pipeline.build_errors = output.build_errors.clone();
        pipeline.test_failures = output.test_failures.clone();

        let output_key = format!("{}_output", phase);
        if !output.stdout.is_empty() {
            pipeline.vars.insert(output_key, output.stdout.clone());
        }
    }

    if let Ok(diff_output) = tokio::process::Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(&task_ctx.workspace_root)
        .output()
        .await
    {
        pipeline.diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
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
            let should_stop = unresolved == 0;
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
    let item_id = item.id.as_str();
    let qa_step = task_ctx.execution_plan.step(WorkflowStepType::Qa);
    let plan_step = task_ctx.execution_plan.step(WorkflowStepType::Plan);
    let ticket_scan_step = task_ctx.execution_plan.step(WorkflowStepType::TicketScan);
    let fix_step = task_ctx.execution_plan.step(WorkflowStepType::Fix);
    let retest_step = task_ctx.execution_plan.step(WorkflowStepType::Retest);
    let qa_enabled = qa_step.is_some();
    let fix_enabled = fix_step.is_some();
    let retest_enabled = retest_step.is_some();
    let mut active_tickets: Vec<String> = Vec::new();
    let retest_new_tickets: Vec<String> = Vec::new();
    let mut qa_failed = false;
    let mut qa_ran = false;
    let mut qa_skipped = false;
    let mut fix_ran = false;
    let mut fix_success = false;
    let mut retest_ran = false;
    let mut retest_success = false;
    let mut qa_exit_code: Option<i64> = None;
    let mut fix_exit_code: Option<i64> = None;
    let mut retest_exit_code: Option<i64> = None;
    let mut new_ticket_count = 0_i64;
    let mut item_status = "pending".to_string();
    let mut phase_artifacts: Vec<crate::collab::Artifact> = Vec::new();
    let mut qa_stdout_path: Option<String> = None;
    let mut qa_stderr_path: Option<String> = None;
    let mut created_ticket_files: Vec<String> = Vec::new();
    let mut pipeline_vars = task_ctx.pipeline_vars.clone();
    let mut build_exit_code: Option<i64> = None;
    let mut test_exit_code: Option<i64> = None;

    if let Some(plan_step) = plan_step {
        if plan_step.enabled && (plan_step.repeatable || task_ctx.current_cycle <= 1) {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_started",
                json!({"step":"plan"}),
            )?;
            let result = run_phase_with_rotation(
                state,
                RotatingPhaseRunRequest {
                    task_id,
                    item_id,
                    step_id: &plan_step.id,
                    phase: "plan",
                    tty: plan_step.tty,
                    capability: plan_step.required_capability.as_deref(),
                    rel_path: &item.qa_file_path,
                    ticket_paths: &active_tickets,
                    workspace_root: &task_ctx.workspace_root,
                    workspace_id: &task_ctx.workspace_id,
                    cycle: task_ctx.current_cycle,
                    runtime,
                    pipeline_vars: Some(&task_ctx.pipeline_vars),
                    step_timeout_secs: task_ctx.safety.step_timeout_secs,
                },
            )
            .await?;
            if let Some(ref output) = result.output {
                pipeline_vars.prev_stdout = output.stdout.clone();
                pipeline_vars.prev_stderr = output.stderr.clone();
                if !output.stdout.is_empty() {
                    pipeline_vars
                        .vars
                        .insert("plan_output".to_string(), output.stdout.clone());
                }
            }
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step":"plan","exit_code":result.exit_code,"success":result.exit_code == 0}),
            )?;
            if result.exit_code != 0 {
                item_status = "unresolved".to_string();
                state
                    .db_writer
                    .update_task_item_status(item_id, &item_status)?;
                return Ok(());
            }
        }
    }

    if let Some(qa_step) = qa_step {
        let should_run_qa = evaluate_step_prehook(
            state,
            qa_step.prehook.as_ref(),
            &StepPrehookContext {
                task_id: task_id.to_string(),
                task_item_id: item_id.to_string(),
                cycle: task_ctx.current_cycle,
                step: "qa".to_string(),
                qa_file_path: item.qa_file_path.clone(),
                item_status: item_status.clone(),
                task_status: "running".to_string(),
                qa_exit_code,
                fix_exit_code,
                retest_exit_code,
                active_ticket_count: active_tickets.len() as i64,
                new_ticket_count,
                qa_failed,
                fix_required: qa_failed || !active_tickets.is_empty(),
                qa_confidence: None,
                qa_quality_score: None,
                fix_has_changes: None,
                upstream_artifacts: vec![],
                build_error_count: 0,
                test_failure_count: 0,
                build_exit_code: None,
                test_exit_code: None,
                self_test_exit_code: None,
                self_test_passed: false,
            },
        )?;

        if should_run_qa {
            qa_ran = true;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_started",
                json!({"step":"qa"}),
            )?;
            let result = run_phase_with_rotation(
                state,
                RotatingPhaseRunRequest {
                    task_id,
                    item_id,
                    step_id: &qa_step.id,
                    phase: "qa",
                    tty: qa_step.tty,
                    capability: qa_step.required_capability.as_deref(),
                    rel_path: &item.qa_file_path,
                    ticket_paths: &active_tickets,
                    workspace_root: &task_ctx.workspace_root,
                    workspace_id: &task_ctx.workspace_id,
                    cycle: task_ctx.current_cycle,
                    runtime,
                    pipeline_vars: None,
                    step_timeout_secs: task_ctx.safety.step_timeout_secs,
                },
            )
            .await?;
            qa_exit_code = Some(result.exit_code);
            qa_failed = result.exit_code != 0;
            qa_stdout_path = Some(result.stdout_path.clone());
            qa_stderr_path = Some(result.stderr_path.clone());

            let qa_artifacts = result
                .output
                .as_ref()
                .map(|o| o.artifacts.clone())
                .unwrap_or_default();
            if !qa_artifacts.is_empty() {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "artifacts_parsed",
                    json!({"step":"qa","count":qa_artifacts.len()}),
                )?;
                phase_artifacts.extend(qa_artifacts);
            }

            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step":"qa","exit_code":result.exit_code,"success":result.is_success()}),
            )?;
        } else {
            qa_skipped = true;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                json!({"step":"qa"}),
            )?;
        }
    }

    if qa_failed || (!active_tickets.is_empty() && qa_enabled) {
        item_status = "qa_failed".to_string();
    }

    if qa_failed {
        if let Some(qa_exit) = qa_exit_code {
            let stdout_path = qa_stdout_path.clone().unwrap_or_default();
            let stderr_path = qa_stderr_path.clone().unwrap_or_default();
            let task_name = SqliteTaskRepository::new(state.db_path.clone())
                .load_task_name(task_id)?
                .unwrap_or_else(|| task_id.to_string());
            match create_ticket_for_qa_failure(
                &task_ctx.workspace_root,
                &task_ctx.ticket_dir,
                &task_name,
                &item.qa_file_path,
                qa_exit,
                &stdout_path,
                &stderr_path,
            ) {
                Ok(Some(ticket_path)) => {
                    created_ticket_files.push(ticket_path.clone());
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

    if let Some(scan_step) = ticket_scan_step {
        if scan_step.enabled {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_started",
                json!({"step":"ticket_scan"}),
            )?;
            let tickets = scan_active_tickets_for_task_items(task_ctx, task_item_paths)?;
            active_tickets = tickets.get(&item.qa_file_path).cloned().unwrap_or_default();
            new_ticket_count = active_tickets.len() as i64;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step":"ticket_scan","tickets":active_tickets.len()}),
            )?;
        }
    } else {
        active_tickets = list_existing_tickets_for_item(task_ctx, &item.qa_file_path)?;
        new_ticket_count = active_tickets.len() as i64;
    }

    if active_tickets.is_empty() {
        let ticket_artifacts = phase_artifacts
            .iter()
            .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
            .count();
        if ticket_artifacts > 0 {
            active_tickets = (0..ticket_artifacts)
                .map(|idx| format!("artifact://ticket/{}", idx))
                .collect();
            new_ticket_count = active_tickets.len() as i64;
        }
    }

    if let Some(fix_step) = fix_step {
        if fix_step.enabled && !active_tickets.is_empty() {
            let should_run_fix = evaluate_step_prehook(
                state,
                fix_step.prehook.as_ref(),
                &StepPrehookContext {
                    task_id: task_id.to_string(),
                    task_item_id: item_id.to_string(),
                    cycle: task_ctx.current_cycle,
                    step: "fix".to_string(),
                    qa_file_path: item.qa_file_path.clone(),
                    item_status: item_status.clone(),
                    task_status: "running".to_string(),
                    qa_exit_code,
                    fix_exit_code,
                    retest_exit_code,
                    active_ticket_count: active_tickets.len() as i64,
                    new_ticket_count,
                    qa_failed,
                    fix_required: qa_failed || !active_tickets.is_empty(),
                    qa_confidence: None,
                    qa_quality_score: None,
                    fix_has_changes: None,
                    upstream_artifacts: vec![],
                    build_error_count: 0,
                    test_failure_count: 0,
                    build_exit_code: None,
                    test_exit_code: None,
                    self_test_exit_code: None,
                    self_test_passed: false,
                },
            )?;

            if should_run_fix {
                fix_ran = true;
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_started",
                    json!({"step":"fix"}),
                )?;
                let result = run_phase_with_rotation(
                    state,
                    RotatingPhaseRunRequest {
                        task_id,
                        item_id,
                        step_id: &fix_step.id,
                        phase: "fix",
                        tty: fix_step.tty,
                        capability: fix_step.required_capability.as_deref(),
                        rel_path: &item.qa_file_path,
                        ticket_paths: &active_tickets,
                        workspace_root: &task_ctx.workspace_root,
                        workspace_id: &task_ctx.workspace_id,
                        cycle: task_ctx.current_cycle,
                        runtime,
                        pipeline_vars: None,
                        step_timeout_secs: task_ctx.safety.step_timeout_secs,
                    },
                )
                .await?;
                fix_exit_code = Some(result.exit_code);
                fix_success = result.is_success();
                if fix_success {
                    item_status = "fixed".to_string();
                }

                let fix_artifacts = result
                    .output
                    .as_ref()
                    .map(|o| o.artifacts.clone())
                    .unwrap_or_default();
                if !fix_artifacts.is_empty() {
                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "artifacts_parsed",
                        json!({"step":"fix","count":fix_artifacts.len()}),
                    )?;
                    phase_artifacts.extend(fix_artifacts);
                }

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step":"fix","exit_code":result.exit_code,"success":fix_success}),
                )?;
            }
        }
    }

    if let Some(retest_step) = retest_step {
        if retest_step.enabled && fix_success {
            retest_ran = true;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_started",
                json!({"step":"retest"}),
            )?;
            let result = run_phase_with_rotation(
                state,
                RotatingPhaseRunRequest {
                    task_id,
                    item_id,
                    step_id: &retest_step.id,
                    phase: "retest",
                    tty: retest_step.tty,
                    capability: retest_step.required_capability.as_deref(),
                    rel_path: &item.qa_file_path,
                    ticket_paths: &retest_new_tickets,
                    workspace_root: &task_ctx.workspace_root,
                    workspace_id: &task_ctx.workspace_id,
                    cycle: task_ctx.current_cycle,
                    runtime,
                    pipeline_vars: None,
                    step_timeout_secs: task_ctx.safety.step_timeout_secs,
                },
            )
            .await?;
            retest_exit_code = Some(result.exit_code);
            retest_success = result.is_success();
            if retest_success {
                item_status = "verified".to_string();
            }
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step":"retest","exit_code":result.exit_code,"success":retest_success}),
            )?;
        }
    }

    for step in &task_ctx.execution_plan.steps {
        if step.is_guard {
            continue;
        }
        let step_type = step.step_type.as_ref().map(|t| t.as_str()).unwrap_or("");
        let is_standard_step = matches!(
            step_type,
            "" | "init_once" | "plan" | "qa" | "ticket_scan" | "retest" | "loop_guard"
        );
        let is_fix_in_pipeline = step_type == "fix" && !fix_ran;
        if is_standard_step && !is_fix_in_pipeline {
            continue;
        }
        if !step.enabled {
            continue;
        }
        if !step.repeatable && task_ctx.current_cycle > 1 {
            continue;
        }

        let should_run = evaluate_step_prehook(
            state,
            step.prehook.as_ref(),
            &StepPrehookContext {
                task_id: task_id.to_string(),
                task_item_id: item_id.to_string(),
                cycle: task_ctx.current_cycle,
                step: step_type.to_string(),
                qa_file_path: item.qa_file_path.clone(),
                item_status: item_status.clone(),
                task_status: "running".to_string(),
                qa_exit_code,
                fix_exit_code,
                retest_exit_code,
                active_ticket_count: active_tickets.len() as i64,
                new_ticket_count,
                qa_failed,
                fix_required: qa_failed || !active_tickets.is_empty(),
                qa_confidence: None,
                qa_quality_score: None,
                fix_has_changes: None,
                upstream_artifacts: vec![],
                build_error_count: pipeline_vars.build_errors.len() as i64,
                test_failure_count: pipeline_vars.test_failures.len() as i64,
                build_exit_code,
                test_exit_code,
                self_test_exit_code: pipeline_vars
                    .vars
                    .get("self_test_exit_code")
                    .and_then(|v| v.parse::<i64>().ok()),
                self_test_passed: pipeline_vars
                    .vars
                    .get("self_test_passed")
                    .map(|v| v == "true")
                    .unwrap_or(false),
            },
        )?;

        if !should_run {
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_skipped",
                json!({"step": step_type, "reason": "prehook_false"}),
            )?;
            continue;
        }

        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_started",
            json!({"step": step_type, "step_id": step.id}),
        )?;

        if step.builtin.as_deref() == Some("self_test") {
            let exit_code =
                execute_self_test_step(&task_ctx.workspace_root, state, task_id, item_id)
                    .await
                    .unwrap_or(1);

            let passed = exit_code == 0;
            pipeline_vars
                .vars
                .insert("self_test_exit_code".to_string(), exit_code.to_string());
            pipeline_vars
                .vars
                .insert("self_test_passed".to_string(), passed.to_string());

            if !passed {
                item_status = "self_test_failed".to_string();
            }

            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": "self_test", "exit_code": exit_code, "success": passed}),
            )?;
            continue;
        }

        if step.step_type == Some(WorkflowStepType::SmokeChain) && !step.chain_steps.is_empty() {
            let mut smoke_chain_passed = true;
            for chain_step in &step.chain_steps {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "chain_step_started",
                    json!({"step": step_type, "chain_step": chain_step.id}),
                )?;

                let mut step_ctx = task_ctx.clone();
                step_ctx.pipeline_vars = pipeline_vars.clone();

                let (result, new_pipeline) = execute_builtin_step(
                    state,
                    task_id,
                    item_id,
                    chain_step,
                    &step_ctx,
                    runtime,
                )
                .await?;
                pipeline_vars = new_pipeline;

                if let Some(ref output) = result.output {
                    if !output.stdout.is_empty() {
                        pipeline_vars
                            .vars
                            .insert("plan_output".to_string(), output.stdout.clone());
                    }
                }

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "chain_step_finished",
                    json!({
                        "step": step_type,
                        "chain_step": chain_step.id,
                        "exit_code": result.exit_code,
                        "success": result.is_success()
                    }),
                )?;

                if !result.is_success() {
                    smoke_chain_passed = false;
                    item_status = format!("{}_failed", chain_step.id);
                    break;
                }
            }

            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_finished",
                json!({"step": "smoke_chain", "success": smoke_chain_passed}),
            )?;
            continue;
        }

        let mut step_ctx = task_ctx.clone();
        step_ctx.pipeline_vars = pipeline_vars.clone();

        let (result, new_pipeline) =
            execute_builtin_step(state, task_id, item_id, step, &step_ctx, runtime).await?;
        pipeline_vars = new_pipeline;

        match step_type {
            "build" => {
                build_exit_code = Some(result.exit_code);
                if !result.is_success() {
                    item_status = "build_failed".to_string();
                }
            }
            "test" => {
                test_exit_code = Some(result.exit_code);
                if !result.is_success() {
                    item_status = "test_failed".to_string();
                }
            }
            "implement" => {
                if !result.is_success() {
                    item_status = "implement_failed".to_string();
                }
            }
            _ => {}
        }

        let confidence = result.output.as_ref().map(|o| o.confidence).unwrap_or(0.0);
        let quality = result
            .output
            .as_ref()
            .map(|o| o.quality_score)
            .unwrap_or(0.0);
        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_finished",
            json!({
                "step": step_type,
                "step_id": step.id,
                "agent_id": result.agent_id,
                "run_id": result.run_id,
                "exit_code": result.exit_code,
                "success": result.is_success(),
                "timed_out": result.timed_out,
                "duration_ms": result.duration_ms,
                "build_errors": pipeline_vars.build_errors.len(),
                "test_failures": pipeline_vars.test_failures.len(),
                "confidence": confidence,
                "quality_score": quality,
                "validation_status": result.validation_status,
            }),
        )?;
    }

    if !task_ctx.dynamic_steps.is_empty() {
        let pool = {
            let mut p = crate::dynamic_orchestration::DynamicStepPool::new();
            for ds in &task_ctx.dynamic_steps {
                p.add_step(ds.clone());
            }
            p
        };
        let dyn_ctx = crate::dynamic_orchestration::StepPrehookContext {
            task_id: task_id.to_string(),
            task_item_id: item_id.to_string(),
            cycle: task_ctx.current_cycle,
            step: "dynamic".to_string(),
            qa_file_path: item.qa_file_path.clone(),
            item_status: item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code,
            fix_exit_code,
            retest_exit_code,
            active_ticket_count: active_tickets.len() as i64,
            new_ticket_count,
            qa_failed,
            fix_required: !active_tickets.is_empty(),
            qa_confidence: None,
            qa_quality_score: None,
            fix_has_changes: None,
            upstream_artifacts: vec![],
            build_error_count: 0,
            test_failure_count: 0,
            build_exit_code: None,
            test_exit_code: None,
            self_test_exit_code: None,
            self_test_passed: false,
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
                    ticket_paths: &active_tickets,
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

    if item_status == "pending" {
        if qa_failed {
            item_status = "qa_failed".to_string();
        } else if !active_tickets.is_empty() && !fix_ran {
            item_status = "unresolved".to_string();
        } else if fix_success && !retest_ran {
            item_status = "fixed".to_string();
        } else if fix_success && retest_success {
            item_status = "verified".to_string();
        } else if !active_tickets.is_empty() {
            item_status = "unresolved".to_string();
        } else if qa_skipped || !qa_enabled {
            item_status = "skipped".to_string();
        } else {
            item_status = "qa_passed".to_string();
        }
    }

    let finalize_context = ItemFinalizeContext {
        task_id: task_id.to_string(),
        task_item_id: item_id.to_string(),
        cycle: task_ctx.current_cycle,
        qa_file_path: item.qa_file_path.clone(),
        item_status: item_status.clone(),
        task_status: "running".to_string(),
        qa_exit_code,
        fix_exit_code,
        retest_exit_code,
        active_ticket_count: active_tickets.len() as i64,
        new_ticket_count,
        retest_new_ticket_count: retest_new_tickets.len() as i64,
        qa_failed,
        fix_required: !active_tickets.is_empty(),
        qa_enabled,
        qa_ran,
        qa_skipped,
        fix_enabled,
        fix_ran,
        fix_success,
        retest_enabled,
        retest_ran,
        retest_success,
        qa_confidence: None,
        qa_quality_score: None,
        fix_confidence: None,
        fix_quality_score: None,
        total_artifacts: phase_artifacts.len() as i64,
        has_ticket_artifacts: phase_artifacts
            .iter()
            .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. })),
        has_code_change_artifacts: phase_artifacts
            .iter()
            .any(|a| matches!(a.kind, crate::collab::ArtifactKind::CodeChange { .. })),
    };

    if let Some(outcome) = crate::prehook::resolve_workflow_finalize_outcome(
        &task_ctx.execution_plan.finalize,
        &finalize_context,
    )? {
        item_status = outcome.status.clone();
        emit_item_finalize_event(state, &finalize_context, &outcome)?;
    }

    let has_ticket_artifacts_for_persist = !created_ticket_files.is_empty()
        || phase_artifacts
            .iter()
            .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }));
    if has_ticket_artifacts_for_persist {
        let ticket_content: Vec<&serde_json::Value> = phase_artifacts
            .iter()
            .filter(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. }))
            .filter_map(|a| a.content.as_ref())
            .collect();
        let files_json =
            serde_json::to_string(&created_ticket_files).unwrap_or_else(|_| "[]".to_string());
        let content_json =
            serde_json::to_string(&ticket_content).unwrap_or_else(|_| "[]".to_string());
        state
            .db_writer
            .update_task_item_tickets(item_id, &files_json, &content_json)?;
    }

    state
        .db_writer
        .update_task_item_status(item_id, &item_status)?;
    Ok(())
}
