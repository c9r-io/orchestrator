use crate::config::{
    ItemFinalizeContext, LoopMode, StepPrehookContext, TaskExecutionStep, TaskRuntimeContext,
    WorkflowStepType,
};
use crate::config_load::{
    build_execution_plan, now_ts, read_active_config, resolve_workspace_path,
};
use crate::dto::{LogChunk, TaskDetail, TaskSummary};
use crate::events::insert_event;
use crate::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use crate::metrics::MetricsCollector;
use crate::output_validation::validate_phase_output;
use crate::prehook::{emit_item_finalize_event, evaluate_step_prehook};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::state::{InnerState, TASK_SEMAPHORE};
use crate::task_repository::{NewCommandRun, SqliteTaskRepository, TaskRepository};
use crate::ticket::{
    create_ticket_for_qa_failure, list_existing_tickets_for_item,
    scan_active_tickets_for_task_items,
};
use anyhow::{Context, Result};

pub use crate::state::RunningTask;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::time::Instant;
use uuid::Uuid;

#[allow(dead_code)]
const IDLE_TIMEOUT_SECS: u64 = 600;

pub async fn kill_current_child(runtime: &RunningTask) {
    let mut child_lock = runtime.child.lock().await;
    if let Some(mut child) = child_lock.take() {
        let _ = child.kill().await;
    }
}

pub fn resolve_task_id(state: &InnerState, task_id: &str) -> Result<String> {
    SqliteTaskRepository::new(state.db_path.clone()).resolve_task_id(task_id)
}

pub fn load_task_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let mut summary = repo.load_task_summary(&resolved_id)?;
    let (total, finished, failed) = repo.load_task_item_counts(&resolved_id)?;

    summary.total_items = total;
    summary.finished_items = finished;
    summary.failed_items = failed;
    Ok(summary)
}

pub fn list_tasks_impl(state: &InnerState) -> Result<Vec<TaskSummary>> {
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let ids = repo.list_task_ids_ordered_by_created_desc()?;

    let mut result = Vec::new();
    for id in ids {
        result.push(load_task_summary(state, &id)?);
    }
    Ok(result)
}

pub fn get_task_details_impl(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    let task = load_task_summary(state, task_id)?;
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let (items, runs, events) = repo.load_task_detail_rows(&task.id)?;

    Ok(TaskDetail {
        task,
        items,
        runs,
        events,
    })
}

pub fn delete_task_impl(state: &InnerState, task_id: &str) -> Result<()> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let log_paths = repo.delete_task_and_collect_log_paths(&resolved_id)?;

    for path in log_paths {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}

pub fn stream_task_logs_impl(
    state: &InnerState,
    task_id: &str,
    tail_count: usize,
    show_timestamps: bool,
) -> Result<Vec<LogChunk>> {
    const PER_FILE_LINE_LIMIT: usize = 150;

    let resolved_id = resolve_task_id(state, task_id)?;
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let runs = repo.list_task_log_runs(&resolved_id, 14)?;

    let mut chunks = Vec::new();
    for row in runs {
        let run_id = row.run_id;
        let phase = row.phase;
        let stdout_path = row.stdout_path;
        let stderr_path = row.stderr_path;
        let started_at = row.started_at;
        let stdout_tail = tail_lines(Path::new(&stdout_path), PER_FILE_LINE_LIMIT)
            .with_context(|| format!("read stdout tail for run_id={run_id} path={stdout_path}"))?;
        let stderr_tail = tail_lines(Path::new(&stderr_path), PER_FILE_LINE_LIMIT)
            .with_context(|| format!("read stderr tail for run_id={run_id} path={stderr_path}"))?;

        let header = if show_timestamps {
            let ts = started_at.as_deref().unwrap_or("unknown");
            format!("[{}][{}][{}]", ts, run_id, phase)
        } else {
            format!("[{}][{}]", run_id, phase)
        };

        let content = format!(
            "{}\n{}{}",
            header,
            stdout_tail,
            if stderr_tail.is_empty() {
                String::new()
            } else {
                format!("\n[stderr]\n{}", stderr_tail)
            }
        );
        chunks.push(LogChunk {
            run_id,
            phase,
            content,
            stdout_path,
            stderr_path,
            started_at,
        });
    }
    chunks.reverse();

    if tail_count < chunks.len() {
        chunks = chunks.split_off(chunks.len() - tail_count);
    }

    Ok(chunks)
}

fn tail_lines(path: &Path, limit: usize) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read log file: {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].join("\n"))
}

pub fn set_task_status(
    state: &InnerState,
    task_id: &str,
    status: &str,
    set_completed: bool,
) -> Result<()> {
    SqliteTaskRepository::new(state.db_path.clone()).set_task_status(task_id, status, set_completed)
}

pub fn prepare_task_for_start(state: &InnerState, task_id: &str) -> Result<()> {
    SqliteTaskRepository::new(state.db_path.clone()).prepare_task_for_start_batch(task_id)?;
    insert_event(
        state,
        task_id,
        None,
        "task_started",
        json!({"reason":"manual_or_resume"}),
    )?;
    Ok(())
}

pub async fn spawn_task_runner(state: Arc<InnerState>, task_id: String) -> Result<()> {
    {
        let mut running = state.running.lock().await;
        if running.contains_key(&task_id) {
            return Ok(());
        }
        running.insert(task_id.clone(), RunningTask::new());
    }

    let permit = TASK_SEMAPHORE
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to acquire semaphore: {}", e))?;

    tokio::spawn(async move {
        let runtime = {
            let running = state.running.lock().await;
            running.get(&task_id).cloned()
        };

        if let Some(runtime) = runtime {
            let run_result = run_task_loop(state.clone(), &task_id, runtime.clone()).await;
            if let Err(err) = run_result {
                let _ = set_task_status(&state, &task_id, "failed", false);
                let _ = insert_event(
                    &state,
                    &task_id,
                    None,
                    "task_failed",
                    json!({"error": err.to_string()}),
                );
                state.emit_event(
                    &task_id,
                    None,
                    "task_failed",
                    json!({"error": err.to_string()}),
                );
            }
        }

        drop(permit);

        let mut running = state.running.lock().await;
        running.remove(&task_id);
    });

    Ok(())
}

pub async fn stop_task_runtime(state: Arc<InnerState>, task_id: &str, status: &str) -> Result<()> {
    let runtime = {
        let running = state.running.lock().await;
        running.get(task_id).cloned()
    };

    if let Some(runtime) = runtime {
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(&runtime).await;
    }

    set_task_status(&state, task_id, status, false)?;
    insert_event(
        &state,
        task_id,
        None,
        "task_control",
        json!({"status": status}),
    )?;
    Ok(())
}

pub async fn stop_task_runtime_for_delete(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    let runtime = {
        let mut running = state.running.lock().await;
        running.remove(task_id)
    };
    if let Some(runtime) = runtime {
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(&runtime).await;
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn shutdown_running_tasks(state: Arc<InnerState>) {
    let runtimes: Vec<(String, RunningTask)> = {
        let running = state.running.lock().await;
        running
            .iter()
            .map(|(task_id, runtime)| (task_id.clone(), runtime.clone()))
            .collect()
    };

    if runtimes.is_empty() {
        return;
    }

    for (_, runtime) in &runtimes {
        runtime.stop_flag.store(true, Ordering::SeqCst);
        kill_current_child(runtime).await;
    }

    for (task_id, _) in &runtimes {
        let _ = set_task_status(&state, task_id, "paused", false);
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_paused",
            json!({"reason":"app_shutdown"}),
        );
    }

    let mut running = state.running.lock().await;
    for (task_id, _) in runtimes {
        running.remove(&task_id);
    }
}

pub fn find_latest_resumable_task_id(
    state: &InnerState,
    include_pending: bool,
) -> Result<Option<String>> {
    SqliteTaskRepository::new(state.db_path.clone()).find_latest_resumable_task_id(include_pending)
}

pub fn load_task_runtime_context(state: &InnerState, task_id: &str) -> Result<TaskRuntimeContext> {
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let runtime_row = repo.load_task_runtime_row(task_id)?;
    let workspace_id = runtime_row.workspace_id;
    let workflow_id = runtime_row.workflow_id;
    let workspace_root_raw = runtime_row.workspace_root_raw;
    let ticket_dir = runtime_row.ticket_dir;
    let execution_plan_json = runtime_row.execution_plan_json;
    let current_cycle = runtime_row.current_cycle;
    let init_done = runtime_row.init_done;

    let active = read_active_config(state)?;
    let workflow = active
        .config
        .workflows
        .get(&workflow_id)
        .with_context(|| format!("workflow not found for task {}: {}", task_id, workflow_id))?;

    let mut execution_plan =
        serde_json::from_str::<crate::config::TaskExecutionPlan>(&execution_plan_json)
            .ok()
            .filter(|plan| !plan.steps.is_empty())
            .unwrap_or_else(|| {
                build_execution_plan(&active.config, workflow, &workflow_id).unwrap_or(
                    crate::config::TaskExecutionPlan {
                        steps: Vec::new(),
                        loop_policy: crate::config::WorkflowLoopConfig::default(),
                        finalize: crate::config::default_workflow_finalize_config(),
                    },
                )
            });
    if execution_plan.finalize.rules.is_empty() {
        execution_plan.finalize = crate::config::default_workflow_finalize_config();
    }
    if execution_plan.steps.is_empty() {
        anyhow::bail!("[EMPTY_PLAN] task '{}' has empty execution plan\n  category: runtime\n  suggested_fix: ensure the workflow has at least one enabled step", task_id);
    }

    let workspace_root = PathBuf::from(workspace_root_raw);
    if !workspace_root.exists() {
        anyhow::bail!(
            "workspace root does not exist for task {}: {}",
            task_id,
            workspace_root.display()
        );
    }
    let workspace_root = workspace_root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize workspace root for task {}", task_id))?;
    resolve_workspace_path(&workspace_root, &ticket_dir, "task.ticket_dir")?;

    let dynamic_steps = workflow.dynamic_steps.clone();

    Ok(TaskRuntimeContext {
        workspace_id,
        workspace_root,
        ticket_dir,
        execution_plan,
        current_cycle: current_cycle.max(0) as u32,
        init_done: init_done == 1,
        dynamic_steps,
    })
}

pub async fn run_task_loop(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    set_task_status(&state, task_id, "running", false)?;
    let mut task_ctx = load_task_runtime_context(&state, task_id)?;

    if !task_ctx.init_done {
        if let Some(step) = task_ctx.execution_plan.step(WorkflowStepType::InitOnce) {
            if let Some(anchor_item_id) = first_task_item_id(&state, task_id)? {
                insert_event(
                    &state,
                    task_id,
                    Some(&anchor_item_id),
                    "step_started",
                    json!({"step":"init_once"}),
                )?;
                let init_result = run_phase_with_rotation(
                    &state,
                    task_id,
                    &anchor_item_id,
                    "init_once",
                    step.required_capability.as_deref(),
                    ".",
                    &[],
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    task_ctx.current_cycle,
                    &runtime,
                )
                .await?;
                if !init_result.is_success() {
                    anyhow::bail!("init_once failed: exit={}", init_result.exit_code);
                }
                insert_event(
                    &state,
                    task_id,
                    Some(&anchor_item_id),
                    "step_finished",
                    json!({"step":"init_once","exit_code":init_result.exit_code}),
                )?;
            }
        }
        task_ctx.init_done = true;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, true)?;
    }

    'cycle: loop {
        if is_task_paused_in_db(&state, task_id)? {
            return Ok(());
        }

        if runtime.stop_flag.load(Ordering::SeqCst) {
            set_task_status(&state, task_id, "paused", false)?;
            insert_event(
                &state,
                task_id,
                None,
                "task_paused",
                json!({"reason":"stop_flag"}),
            )?;
            state.emit_event(task_id, None, "task_paused", json!({}));
            return Ok(());
        }

        task_ctx.current_cycle += 1;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, task_ctx.init_done)?;
        insert_event(
            &state,
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle}),
        )?;
        state.emit_event(
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle}),
        );

        let items = list_task_items_for_cycle(&state, task_id)?;
        let task_item_paths: Vec<String> =
            items.iter().map(|item| item.qa_file_path.clone()).collect();
        for item in items {
            process_item(
                &state,
                task_id,
                &item,
                &task_item_paths,
                &task_ctx,
                &runtime,
            )
            .await?;
            if runtime.stop_flag.load(Ordering::SeqCst) || is_task_paused_in_db(&state, task_id)? {
                continue 'cycle;
            }
        }

        for step in &task_ctx.execution_plan.steps {
            if !step.is_guard {
                continue;
            }

            if !step.repeatable && task_ctx.current_cycle > 1 {
                continue;
            }

            let guard_result =
                execute_guard_step(&state, task_id, step, &task_ctx, &runtime).await?;

            if guard_result.should_stop {
                insert_event(
                    &state,
                    task_id,
                    None,
                    "workflow_terminated",
                    json!({
                        "cycle": task_ctx.current_cycle,
                        "guard_step": step.id,
                        "reason": guard_result.reason
                    }),
                )?;
                state.emit_event(
                    task_id,
                    None,
                    "workflow_terminated",
                    json!({"guard_step": step.id}),
                );
                set_task_status(&state, task_id, "completed", true)?;
                insert_event(&state, task_id, None, "task_completed", json!({}))?;
                state.emit_event(task_id, None, "task_completed", json!({}));
                return Ok(());
            }
        }

        let unresolved = count_unresolved_items(&state, task_id)?;

        let loop_mode_check = evaluate_loop_guard_rules(
            &task_ctx.execution_plan.loop_policy,
            task_ctx.current_cycle,
            unresolved,
        );

        let should_continue = if let Some((continue_loop, _)) = loop_mode_check {
            continue_loop
        } else if task_ctx
            .execution_plan
            .loop_policy
            .guard
            .stop_when_no_unresolved
        {
            unresolved > 0
        } else {
            true
        };

        let reason = if let Some((_, reason)) = loop_mode_check {
            reason
        } else if !should_continue {
            "no_unresolved_items".to_string()
        } else {
            "continue".to_string()
        };
        insert_event(
            &state,
            task_id,
            None,
            "loop_guard_decision",
            json!({
                "cycle": task_ctx.current_cycle,
                "continue": should_continue,
                "reason": reason,
                "unresolved_items": unresolved
            }),
        )?;
        state.emit_event(
            task_id,
            None,
            "loop_guard_decision",
            json!({
                "cycle": task_ctx.current_cycle,
                "continue": should_continue,
                "reason": reason,
                "unresolved_items": unresolved
            }),
        );
        if !should_continue {
            break;
        }
    }

    let unresolved = count_unresolved_items(&state, task_id)?;

    if is_task_paused_in_db(&state, task_id)? {
        return Ok(());
    }

    if unresolved > 0 {
        set_task_status(&state, task_id, "failed", true)?;
        insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        )?;
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        );
    } else {
        set_task_status(&state, task_id, "completed", true)?;
        insert_event(&state, task_id, None, "task_completed", json!({}))?;
        state.emit_event(task_id, None, "task_completed", json!({}));
    }

    Ok(())
}

pub fn evaluate_loop_guard_rules(
    loop_policy: &crate::config::WorkflowLoopConfig,
    current_cycle: u32,
    _unresolved: i64,
) -> Option<(bool, String)> {
    match loop_policy.mode {
        LoopMode::Once => Some((false, "once_mode".to_string())),
        LoopMode::Infinite => {
            if let Some(max_cycles) = loop_policy.guard.max_cycles {
                if current_cycle >= max_cycles {
                    return Some((false, "max_cycles_reached".to_string()));
                }
            }
            if !loop_policy.guard.enabled {
                return Some((true, "guard_disabled".to_string()));
            }
            None
        }
    }
}

pub fn first_task_item_id(state: &InnerState, task_id: &str) -> Result<Option<String>> {
    SqliteTaskRepository::new(state.db_path.clone()).first_task_item_id(task_id)
}

pub fn count_unresolved_items(state: &InnerState, task_id: &str) -> Result<i64> {
    SqliteTaskRepository::new(state.db_path.clone()).count_unresolved_items(task_id)
}

pub fn list_task_items_for_cycle(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<crate::dto::TaskItemRow>> {
    SqliteTaskRepository::new(state.db_path.clone()).list_task_items_for_cycle(task_id)
}

pub fn update_task_cycle_state(
    state: &InnerState,
    task_id: &str,
    current_cycle: u32,
    init_done: bool,
) -> Result<()> {
    SqliteTaskRepository::new(state.db_path.clone()).update_task_cycle_state(
        task_id,
        current_cycle,
        init_done,
    )
}

fn is_task_paused_in_db(state: &InnerState, task_id: &str) -> Result<bool> {
    let status = SqliteTaskRepository::new(state.db_path.clone()).load_task_status(task_id)?;
    Ok(matches!(status.as_deref(), Some("paused")))
}

fn persist_structured_output(
    state: &InnerState,
    run_id: &str,
    output: &crate::collab::AgentOutput,
    validation_status: &str,
) -> Result<()> {
    let conn = crate::db::open_conn(&state.db_path)?;
    conn.execute(
        "UPDATE command_runs SET output_json = ?2, artifacts_json = ?3, confidence = ?4, quality_score = ?5, validation_status = ?6 WHERE id = ?1",
        rusqlite::params![
            run_id,
            serde_json::to_string(output)?,
            serde_json::to_string(&output.artifacts)?,
            output.confidence,
            output.quality_score,
            validation_status
        ],
    )?;
    Ok(())
}

pub async fn run_phase(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    command: String,
    workspace_root: &Path,
    workspace_id: &str,
    agent_id: &str,
    runtime: &RunningTask,
) -> Result<crate::dto::RunResult> {
    let now = now_ts();
    let run_uuid = Uuid::new_v4();
    let run_id = run_uuid.to_string();
    let logs_dir = state.logs_dir.join(task_id);
    let stdout_path = logs_dir.join(format!("{}_{}.stdout", phase, run_id));
    let stderr_path = logs_dir.join(format!("{}_{}.stderr", phase, run_id));

    let runner = {
        let active = read_active_config(state)?;
        active.config.runner.clone()
    };

    let logs_dir_for_create = logs_dir.clone();
    let stdout_path_for_create = stdout_path.clone();
    let stderr_path_for_create = stderr_path.clone();
    let (stdout_file, stderr_file) = tokio::task::spawn_blocking(move || -> Result<_> {
        std::fs::create_dir_all(&logs_dir_for_create).with_context(|| {
            format!(
                "failed to create logs dir: {}",
                logs_dir_for_create.display()
            )
        })?;
        let stdout_file = std::fs::File::create(&stdout_path_for_create).with_context(|| {
            format!(
                "failed to create stdout log: {}",
                stdout_path_for_create.display()
            )
        })?;
        let stderr_file = std::fs::File::create(&stderr_path_for_create).with_context(|| {
            format!(
                "failed to create stderr log: {}",
                stderr_path_for_create.display()
            )
        })?;
        Ok((stdout_file, stderr_file))
    })
    .await
    .context("log file setup worker failed")??;

    let child = tokio::process::Command::new(&runner.shell)
        .arg(&runner.shell_arg)
        .arg(command.clone())
        .current_dir(workspace_root)
        .stdout(stdout_file)
        .stderr(stderr_file)
        .kill_on_drop(true)
        .spawn()?;

    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = Some(child);
    }

    let start = Instant::now();
    let status = {
        let mut child_lock = runtime.child.lock().await;
        if let Some(ref mut child) = *child_lock {
            child.wait().await
        } else {
            return Err(anyhow::anyhow!("child process not found in runtime"));
        }
    };
    let duration = start.elapsed();

    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = None;
    }

    let exit_code = match status {
        Ok(s) => s.code().unwrap_or(-1),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                -2
            } else {
                -3
            }
        }
    };

    let stdout_content = tokio::fs::read_to_string(&stdout_path)
        .await
        .with_context(|| format!("failed to read stdout log: {}", stdout_path.display()))?;
    let stderr_content = tokio::fs::read_to_string(&stderr_path)
        .await
        .with_context(|| format!("failed to read stderr log: {}", stderr_path.display()))?;

    let validation = validate_phase_output(
        phase,
        run_uuid,
        agent_id,
        exit_code as i64,
        &stdout_content,
        &stderr_content,
    )?;
    let mut success = exit_code == 0;
    if validation.status == "failed" {
        success = false;
        insert_event(
            state,
            task_id,
            Some(item_id),
            "output_validation_failed",
            json!({"phase":phase,"run_id":run_id,"error":validation.error.clone()}),
        )?;
    }

    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let insert_payload = NewCommandRun {
        id: run_id.clone(),
        task_item_id: item_id.to_string(),
        phase: phase.to_string(),
        command: command.clone(),
        cwd: workspace_root.to_string_lossy().to_string(),
        workspace_id: workspace_id.to_string(),
        agent_id: agent_id.to_string(),
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        started_at: now,
        ended_at: now_ts(),
        interrupted: 0,
    };
    tokio::task::spawn_blocking(move || repo.insert_command_run(&insert_payload))
        .await
        .context("command run insert worker failed")??;
    persist_structured_output(state, &run_id, &validation.output, validation.status)?;

    let sender = crate::collab::AgentEndpoint::for_task_item(agent_id, task_id, item_id);
    let msg = crate::collab::AgentMessage::publish(
        sender,
        crate::collab::MessagePayload::ExecutionResult(crate::collab::ExecutionResult {
            run_id: run_uuid,
            output: validation.output.clone(),
            success,
            error: validation.error.clone(),
        }),
    );
    if let Err(err) = state.message_bus.publish(msg).await {
        insert_event(
            state,
            task_id,
            Some(item_id),
            "bus_publish_failed",
            json!({"phase":phase,"run_id":run_id,"error":err.to_string()}),
        )?;
    } else {
        insert_event(
            state,
            task_id,
            Some(item_id),
            "phase_output_published",
            json!({"phase":phase,"run_id":run_id}),
        )?;
    }

    update_capability_health(state, agent_id, Some(phase), success);

    let duration_ms = duration.as_millis() as u64;
    {
        let mut metrics_map = state.agent_metrics.write().unwrap();
        let metrics = metrics_map
            .entry(agent_id.to_string())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        if success {
            MetricsCollector::record_success(metrics, duration_ms);
        } else {
            MetricsCollector::record_failure(metrics);
        }
        MetricsCollector::decrement_load(metrics);
    }

    if !success {
        let errors = increment_consecutive_errors(state, agent_id);
        if errors >= 2 {
            mark_agent_diseased(state, agent_id);
        }
    } else {
        reset_consecutive_errors(state, agent_id);
    }

    Ok(crate::dto::RunResult {
        success,
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        timed_out: false,
        duration_ms: Some(duration_ms),
        output: Some(validation.output),
        validation_status: validation.status.to_string(),
        validation_error: validation.error,
    })
}

pub async fn run_phase_with_rotation(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    capability: Option<&str>,
    rel_path: &str,
    ticket_paths: &[String],
    workspace_root: &Path,
    workspace_id: &str,
    cycle: u32,
    runtime: &RunningTask,
) -> Result<crate::dto::RunResult> {
    let effective_capability = capability.or(match phase {
        "qa" | "fix" | "retest" => Some(phase),
        _ => None,
    });

    let (agent_id, template) = {
        let active = read_active_config(state)?;
        let agents = active.config.agents.clone();

        if let Some(cap) = effective_capability {
            let health_map = state.agent_health.read().unwrap();
            let metrics_map = state.agent_metrics.read().unwrap();
            select_agent_advanced(cap, &agents, &health_map, &metrics_map, &HashSet::new())?
        } else {
            select_agent_by_preference(&agents)?
        }
    };

    {
        let mut metrics_map = state.agent_metrics.write().unwrap();
        let metrics = metrics_map
            .entry(agent_id.clone())
            .or_insert_with(MetricsCollector::new_agent_metrics);
        MetricsCollector::increment_load(metrics);
    }

    let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
    let command = template
        .replace("{rel_path}", &shell_escape(rel_path))
        .replace("{ticket_paths}", &escaped_paths.join(" "))
        .replace("{phase}", phase)
        .replace("{cycle}", &cycle.to_string());

    run_phase(
        state,
        task_id,
        item_id,
        phase,
        command,
        workspace_root,
        workspace_id,
        &agent_id,
        runtime,
    )
    .await
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
        let active = read_active_config(state)?;
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
            .or_insert_with(MetricsCollector::new_agent_metrics);
        MetricsCollector::increment_load(metrics);
    }

    let command = template
        .replace("{task_id}", &shell_escape(task_id))
        .replace(
            "{cycle}",
            &shell_escape(&task_ctx.current_cycle.to_string()),
        );

    let result = run_phase(
        state,
        task_id,
        task_id,
        "guard",
        command,
        &task_ctx.workspace_root,
        &task_ctx.workspace_id,
        &agent_id,
        runtime,
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
                task_id,
                item_id,
                "qa",
                qa_step.required_capability.as_deref(),
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
            )
            .await?;
            qa_exit_code = Some(result.exit_code);
            qa_failed = result.exit_code != 0;

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

            if !result.is_success() {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step":"qa","exit_code":result.exit_code,"success":false}),
                )?;
            } else {
                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_finished",
                    json!({"step":"qa","exit_code":result.exit_code,"success":true}),
                )?;
            }
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
            let stdout_path = format!(
                "{}/data/runs/{}/{}/qa/stdout.log",
                task_ctx.workspace_root.display(),
                task_id,
                item_id
            );
            let stderr_path = format!(
                "{}/data/runs/{}/{}/qa/stderr.log",
                task_ctx.workspace_root.display(),
                task_id,
                item_id
            );
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
                    insert_event(
                        state,
                        task_id,
                        Some(item_id),
                        "ticket_created",
                        json!({"path": ticket_path, "qa_file": item.qa_file_path}),
                    )?;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[warn] failed to auto-create ticket: {e}");
                }
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
                    task_id,
                    item_id,
                    "fix",
                    fix_step.required_capability.as_deref(),
                    &item.qa_file_path,
                    &active_tickets,
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    task_ctx.current_cycle,
                    runtime,
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
                task_id,
                item_id,
                "retest",
                retest_step.required_capability.as_deref(),
                &item.qa_file_path,
                &retest_new_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
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
                task_id,
                item_id,
                &ds.step_type,
                cap,
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
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

    SqliteTaskRepository::new(state.db_path.clone())
        .update_task_item_status(item_id, &item_status)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use rusqlite::params;

    #[test]
    fn load_task_summary_maps_created_and_updated_at_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/mapping_check.md");
        std::fs::write(&qa_file, "# mapping check\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("mapping-check".to_string()),
                goal: Some("validate summary timestamps".to_string()),
                ..Default::default()
            },
        )
        .expect("task should be created");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (workflow_id, created_at, updated_at): (String, String, String) = conn
            .query_row(
                "SELECT workflow_id, created_at, updated_at FROM tasks WHERE id = ?1",
                params![created.id.clone()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("task row should exist");

        let summary = load_task_summary(&state, &created.id).expect("summary should load");
        assert_eq!(summary.workflow_id, workflow_id);
        assert_eq!(summary.created_at, created_at);
        assert_eq!(summary.updated_at, updated_at);
    }
}
