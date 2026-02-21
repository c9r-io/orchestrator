use crate::config::{
    ItemFinalizeContext, LoopMode, StepPrehookContext, TaskExecutionStep, TaskRuntimeContext,
    WorkflowFinalizeOutcome, WorkflowStepType,
};
use crate::config_load::{
    build_execution_plan, now_ts, read_active_config, resolve_workspace_path,
};
use crate::db::open_conn;
use crate::dto::{
    CommandRunDto, CreateTaskPayload, EventDto, LogChunk, TaskDetail, TaskItemDto, TaskSummary,
    TicketPreviewData, UNASSIGNED_QA_FILE_PATH,
};
use crate::events::{emit_event, insert_event};
use crate::health::{
    increment_consecutive_errors, mark_agent_diseased, reset_consecutive_errors,
    update_capability_health,
};
use crate::prehook::{
    emit_item_finalize_event, emit_step_prehook_event, evaluate_finalize_rule_expression,
    evaluate_step_prehook, evaluate_step_prehook_expression,
};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::state::{InnerState, TASK_SEMAPHORE};
use crate::ticket::{
    is_active_ticket_status, list_existing_tickets_for_item, read_ticket_preview,
    scan_active_tickets_for_task_items,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};

pub use crate::state::RunningTask;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};
use uuid::Uuid;

const IDLE_TIMEOUT_SECS: u64 = 600;

pub async fn kill_current_child(runtime: &RunningTask) {
    let mut child_lock = runtime.child.lock().await;
    if let Some(mut child) = child_lock.take() {
        let _ = child.kill().await;
    }
}

pub fn resolve_task_id(state: &InnerState, task_id: &str) -> Result<String> {
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE id = ?1")?;
    let exact_match: Option<String> = stmt
        .query_row(params![task_id], |row| row.get(0))
        .optional()?;

    if let Some(id) = exact_match {
        return Ok(id);
    }

    let pattern = format!("{}%", task_id);
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE id LIKE ?1")?;
    let matches: Vec<String> = stmt
        .query_map(params![pattern], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => anyhow::bail!("task not found: {}", task_id),
        _ => anyhow::bail!("multiple tasks match prefix '{}': {:?}", task_id, matches),
    }
}

pub fn load_task_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, name, status, started_at, completed_at, goal, target_files_json, project_id, workspace_id, workflow_id, created_at, updated_at FROM tasks WHERE id = ?1",
    )?;
    let mut summary = stmt.query_row(params![resolved_id], |row| {
        let target_raw: String = row.get(6)?;
        let target_files = serde_json::from_str::<Vec<String>>(&target_raw).unwrap_or_default();
        Ok(TaskSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            status: row.get(2)?,
            started_at: row.get(3)?,
            completed_at: row.get(4)?,
            goal: row.get(5)?,
            project_id: row.get(7)?,
            workspace_id: row.get(8)?,
            workflow_id: row.get(9)?,
            target_files,
            total_items: 0,
            finished_items: 0,
            failed_items: 0,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    let (total, finished, failed): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), SUM(CASE WHEN status IN ('qa_passed','fixed','verified','skipped','unresolved') THEN 1 ELSE 0 END), SUM(CASE WHEN status IN ('qa_failed','unresolved') THEN 1 ELSE 0 END) FROM task_items WHERE task_id = ?1",
        params![resolved_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            ))
        },
    )?;

    summary.total_items = total;
    summary.finished_items = finished;
    summary.failed_items = failed;
    Ok(summary)
}

pub fn list_tasks_impl(state: &InnerState) -> Result<Vec<TaskSummary>> {
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare("SELECT id FROM tasks ORDER BY created_at DESC")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut result = Vec::new();
    for id in ids {
        result.push(load_task_summary(state, &id)?);
    }
    Ok(result)
}

pub fn get_task_details_impl(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    let task = load_task_summary(state, task_id)?;
    let conn = open_conn(&state.db_path)?;
    let resolved_id = &task.id;

    let mut items_stmt = conn.prepare(
        "SELECT id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at FROM task_items WHERE task_id = ?1 ORDER BY order_no",
    )?;
    let items = items_stmt
        .query_map(params![resolved_id], |row| {
            let ticket_files_raw: String = row.get(5)?;
            let ticket_content_raw: String = row.get(6)?;
            Ok(TaskItemDto {
                id: row.get(0)?,
                task_id: row.get(1)?,
                order_no: row.get(2)?,
                qa_file_path: row.get(3)?,
                status: row.get(4)?,
                ticket_files: serde_json::from_str(&ticket_files_raw).unwrap_or_default(),
                ticket_content: serde_json::from_str(&ticket_content_raw).unwrap_or_default(),
                fix_required: row.get::<_, i64>(7)? == 1,
                fixed: row.get::<_, i64>(8)? == 1,
                last_error: row.get(9)?,
                started_at: row.get(10)?,
                completed_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut runs_stmt = conn.prepare(
        "SELECT cr.id, cr.task_item_id, cr.phase, cr.command, cr.cwd, cr.workspace_id, cr.agent_id, cr.exit_code, cr.stdout_path, cr.stderr_path, cr.started_at, cr.ended_at, cr.interrupted
         FROM command_runs cr
         JOIN task_items ti ON ti.id = cr.task_item_id
         WHERE ti.task_id = ?1
         ORDER BY cr.started_at DESC
         LIMIT 120",
    )?;
    let runs = runs_stmt
        .query_map(params![resolved_id], |row| {
            Ok(CommandRunDto {
                id: row.get(0)?,
                task_item_id: row.get(1)?,
                phase: row.get(2)?,
                command: row.get(3)?,
                cwd: row.get(4)?,
                workspace_id: row.get(5)?,
                agent_id: row.get(6)?,
                exit_code: row.get(7)?,
                stdout_path: row.get(8)?,
                stderr_path: row.get(9)?,
                started_at: row.get(10)?,
                ended_at: row.get(11)?,
                interrupted: row.get::<_, i64>(12)? == 1,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut events_stmt = conn.prepare(
        "SELECT id, task_id, task_item_id, event_type, payload_json, created_at FROM events WHERE task_id = ?1 ORDER BY id DESC LIMIT 200",
    )?;
    let events = events_stmt
        .query_map(params![resolved_id], |row| {
            let payload_raw: String = row.get(4)?;
            Ok(EventDto {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_item_id: row.get(2)?,
                event_type: row.get(3)?,
                payload: serde_json::from_str(&payload_raw).unwrap_or_else(|_| json!({})),
                created_at: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(TaskDetail {
        task,
        items,
        runs,
        events,
    })
}

pub fn delete_task_impl(state: &InnerState, task_id: &str) -> Result<()> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let conn = open_conn(&state.db_path)?;

    let exists = conn
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1",
            params![resolved_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if exists.is_none() {
        anyhow::bail!("task not found: {}", task_id);
    }

    let mut log_paths = HashSet::new();
    let mut runs_stmt = conn.prepare(
        "SELECT cr.stdout_path, cr.stderr_path
         FROM command_runs cr
         JOIN task_items ti ON ti.id = cr.task_item_id
         WHERE ti.task_id = ?1",
    )?;
    for row in runs_stmt.query_map(params![resolved_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (stdout_path, stderr_path) = row?;
        if !stdout_path.trim().is_empty() {
            log_paths.insert(stdout_path);
        }
        if !stderr_path.trim().is_empty() {
            log_paths.insert(stderr_path);
        }
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM events WHERE task_id = ?1",
        params![resolved_id],
    )?;
    tx.execute(
        "DELETE FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
        params![resolved_id],
    )?;
    tx.execute(
        "DELETE FROM task_items WHERE task_id = ?1",
        params![resolved_id],
    )?;
    tx.execute("DELETE FROM tasks WHERE id = ?1", params![resolved_id])?;
    tx.commit()?;

    for path in log_paths {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}

pub fn stream_task_logs_impl(
    state: &InnerState,
    task_id: &str,
    line_limit: usize,
) -> Result<Vec<LogChunk>> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT cr.id, cr.phase, cr.stdout_path, cr.stderr_path
         FROM command_runs cr
         JOIN task_items ti ON ti.id = cr.task_item_id
         WHERE ti.task_id = ?1
         ORDER BY cr.started_at DESC
         LIMIT 14",
    )?;

    let mut chunks = Vec::new();
    for row in stmt.query_map(params![resolved_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })? {
        let (run_id, phase, stdout_path, stderr_path) = row?;
        let stdout_tail = tail_lines(Path::new(&stdout_path), line_limit / 2).unwrap_or_default();
        let stderr_tail = tail_lines(Path::new(&stderr_path), line_limit / 2).unwrap_or_default();
        let content = format!(
            "[{}][{}]\n{}\n{}",
            run_id,
            phase,
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
        });
    }
    chunks.reverse();
    Ok(chunks)
}

fn tail_lines(path: &Path, limit: usize) -> Result<String> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
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
    let conn = open_conn(&state.db_path)?;
    let now = now_ts();
    if set_completed {
        conn.execute(
            "UPDATE tasks SET status = ?2, completed_at = ?3, updated_at = ?4 WHERE id = ?1",
            params![task_id, status, now.clone(), now],
        )?;
    } else if matches!(status, "pending" | "running" | "paused" | "interrupted") {
        conn.execute(
            "UPDATE tasks SET status = ?2, completed_at = NULL, updated_at = ?3 WHERE id = ?1",
            params![task_id, status, now],
        )?;
    } else {
        conn.execute(
            "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_id, status, now],
        )?;
    }
    Ok(())
}

pub fn prepare_task_for_start(state: &InnerState, task_id: &str) -> Result<()> {
    let conn = open_conn(&state.db_path)?;
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?;

    if status.is_none() {
        anyhow::bail!("task not found: {}", task_id);
    }

    if matches!(status.as_deref(), Some("failed")) {
        conn.execute(
            "UPDATE task_items SET status='pending', ticket_files_json='[]', ticket_content_json='[]', fix_required=0, fixed=0, last_error='', completed_at=NULL, updated_at=?2 WHERE task_id=?1 AND status='unresolved'",
            params![task_id, now_ts()],
        )?;
    }

    set_task_status(state, task_id, "running", false)?;
    insert_event(
        state,
        task_id,
        None,
        "task_started",
        json!({"reason":"manual_or_resume"}),
    )?;
    Ok(())
}

pub async fn spawn_task_runner(
    state: Arc<InnerState>,
    app: AppHandle,
    task_id: String,
) -> Result<()> {
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
            let run_result =
                run_task_loop(state.clone(), Some(&app), &task_id, runtime.clone()).await;
            if let Err(err) = run_result {
                let _ = set_task_status(&state, &task_id, "failed", false);
                let _ = insert_event(
                    &state,
                    &task_id,
                    None,
                    "task_failed",
                    json!({"error": err.to_string()}),
                );
                emit_event(
                    &app,
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
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare("SELECT id, status FROM tasks ORDER BY updated_at DESC")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for row in rows {
        let (id, status) = row?;
        let resumable = matches!(status.as_str(), "running" | "interrupted" | "paused")
            || (include_pending && status == "pending");
        if resumable {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

pub fn load_task_runtime_context(state: &InnerState, task_id: &str) -> Result<TaskRuntimeContext> {
    let conn = open_conn(&state.db_path)?;
    let (
        workspace_id,
        workflow_id,
        workspace_root_raw,
        ticket_dir,
        execution_plan_json,
        current_cycle,
        init_done,
    ): (String, String, String, String, String, i64, i64) = conn.query_row(
        "SELECT workspace_id, workflow_id, workspace_root, ticket_dir, execution_plan_json, current_cycle, init_done FROM tasks WHERE id = ?1",
        params![task_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    )?;

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
        anyhow::bail!("task '{}' has empty execution plan", task_id);
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

    Ok(TaskRuntimeContext {
        workspace_id,
        workspace_root,
        ticket_dir,
        execution_plan,
        current_cycle: current_cycle.max(0) as u32,
        init_done: init_done == 1,
    })
}

pub async fn run_task_loop(
    state: Arc<InnerState>,
    app: Option<&AppHandle>,
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
                    app,
                    task_id,
                    &anchor_item_id,
                    "init_once",
                    step.required_capability.as_deref(),
                    ".",
                    &[],
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
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
        if runtime.stop_flag.load(Ordering::SeqCst) {
            set_task_status(&state, task_id, "paused", false)?;
            insert_event(
                &state,
                task_id,
                None,
                "task_paused",
                json!({"reason":"stop_flag"}),
            )?;
            if let Some(app) = app {
                emit_event(app, task_id, None, "task_paused", json!({}));
            }
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
        if let Some(app) = app {
            emit_event(
                app,
                task_id,
                None,
                "cycle_started",
                json!({"cycle": task_ctx.current_cycle}),
            );
        }

        let items = list_task_items_for_cycle(&state, task_id)?;
        let task_item_paths: Vec<String> =
            items.iter().map(|item| item.qa_file_path.clone()).collect();
        for item in items {
            process_item(
                &state,
                app,
                task_id,
                &item,
                &task_item_paths,
                &task_ctx,
                &runtime,
            )
            .await?;
            if runtime.stop_flag.load(Ordering::SeqCst) {
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
                execute_guard_step(&state, app, task_id, step, &task_ctx, &runtime).await?;

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
                if let Some(app) = app {
                    emit_event(
                        app,
                        task_id,
                        None,
                        "workflow_terminated",
                        json!({"guard_step": step.id}),
                    );
                }
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
        if let Some(app) = app {
            emit_event(
                app,
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
        }
        if !should_continue {
            break;
        }
    }

    let unresolved = count_unresolved_items(&state, task_id)?;

    if unresolved > 0 {
        set_task_status(&state, task_id, "failed", true)?;
        insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        )?;
        if let Some(app) = app {
            emit_event(
                app,
                task_id,
                None,
                "task_failed",
                json!({"unresolved_items": unresolved}),
            );
        }
    } else {
        set_task_status(&state, task_id, "completed", true)?;
        insert_event(&state, task_id, None, "task_completed", json!({}))?;
        if let Some(app) = app {
            emit_event(app, task_id, None, "task_completed", json!({}));
        }
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
    let conn = open_conn(&state.db_path)?;
    conn.query_row(
        "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
        params![task_id],
        |row| row.get(0),
    )
    .optional()
    .context("query first task item")
}

pub fn count_unresolved_items(state: &InnerState, task_id: &str) -> Result<i64> {
    let conn = open_conn(&state.db_path)?;
    conn.query_row(
        "SELECT COUNT(*) FROM task_items WHERE task_id = ?1 AND status IN ('unresolved','qa_failed')",
        params![task_id],
        |row| row.get(0),
    )
    .context("count unresolved items")
}

pub fn list_task_items_for_cycle(
    state: &InnerState,
    task_id: &str,
) -> Result<Vec<crate::dto::TaskItemRow>> {
    let conn = open_conn(&state.db_path)?;
    let mut stmt = conn.prepare(
        "SELECT id, qa_file_path
         FROM task_items
         WHERE task_id = ?1
         ORDER BY order_no
        ",
    )?;

    let rows = stmt
        .query_map(params![task_id], |row| {
            Ok(crate::dto::TaskItemRow {
                id: row.get(0)?,
                qa_file_path: row.get(1)?,
            })
        })
        .context("query task items")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn update_task_cycle_state(
    state: &InnerState,
    task_id: &str,
    current_cycle: u32,
    init_done: bool,
) -> Result<()> {
    let conn = open_conn(&state.db_path)?;
    conn.execute(
        "UPDATE tasks SET current_cycle = ?2, init_done = ?3, updated_at = ?4 WHERE id = ?1",
        params![
            task_id,
            current_cycle as i64,
            if init_done { 1 } else { 0 },
            now_ts()
        ],
    )?;
    Ok(())
}

pub async fn run_phase(
    state: &Arc<InnerState>,
    app: Option<&AppHandle>,
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
    let run_id = Uuid::new_v4().to_string();
    let logs_dir = state.logs_dir.join(task_id);
    std::fs::create_dir_all(&logs_dir).ok();
    let stdout_path = logs_dir.join(format!("{}_{}.stdout", phase, run_id));
    let stderr_path = logs_dir.join(format!("{}_{}.stderr", phase, run_id));

    let runner = {
        let active = read_active_config(state)?;
        active.config.runner.clone()
    };

    let mut child = tokio::process::Command::new(&runner.shell)
        .arg(&runner.shell_arg)
        .arg(command.clone())
        .current_dir(workspace_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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

    let success = exit_code == 0;

    let conn = open_conn(&state.db_path)?;
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, started_at, ended_at, interrupted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            run_id,
            item_id,
            phase,
            command,
            workspace_root.to_string_lossy().to_string(),
            workspace_id,
            agent_id,
            exit_code,
            stdout_path.to_string_lossy().to_string(),
            stderr_path.to_string_lossy().to_string(),
            now,
            now_ts(),
            0
        ],
    )?;

    update_capability_health(state, agent_id, Some(phase), success);
    if !success {
        let errors = increment_consecutive_errors(state, app, agent_id);
        if errors >= 2 {
            mark_agent_diseased(state, app, agent_id);
        }
    } else {
        reset_consecutive_errors(state, app, agent_id);
    }

    Ok(crate::dto::RunResult {
        success,
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        timed_out: false,
        duration_ms: Some(duration.as_millis() as u64),
    })
}

pub async fn run_phase_with_rotation(
    state: &Arc<InnerState>,
    app: Option<&AppHandle>,
    task_id: &str,
    item_id: &str,
    phase: &str,
    capability: Option<&str>,
    rel_path: &str,
    ticket_paths: &[String],
    workspace_root: &Path,
    workspace_id: &str,
    runtime: &RunningTask,
) -> Result<crate::dto::RunResult> {
    let (agent_id, template) = {
        let active = read_active_config(state)?;
        let agents = active.config.agents.clone();

        if let Some(cap) = capability {
            let health_map = state.agent_health.read().unwrap();
            let metrics_map = state.agent_metrics.read().unwrap();
            select_agent_advanced(cap, &agents, &health_map, &metrics_map, &HashSet::new())?
        } else {
            select_agent_by_preference(&agents)?
        }
    };

    let escaped_paths: Vec<String> = ticket_paths.iter().map(|p| shell_escape(p)).collect();
    let command = template
        .replace("{rel_path}", &shell_escape(rel_path))
        .replace("{ticket_paths}", &escaped_paths.join(" "));

    run_phase(
        state,
        app,
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
    app: Option<&AppHandle>,
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

    let command = template
        .replace("{task_id}", &shell_escape(task_id))
        .replace(
            "{cycle}",
            &shell_escape(&task_ctx.current_cycle.to_string()),
        );

    let result = run_phase(
        state,
        app,
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

    let output = std::fs::read_to_string(&result.stdout_path).unwrap_or_default();

    let should_stop = output.trim().to_lowercase().starts_with("stop")
        || output.trim().to_lowercase().starts_with("false")
        || output.trim().to_lowercase().starts_with("no");

    Ok(GuardResult {
        should_stop,
        reason: output.trim().to_string(),
    })
}

pub async fn process_item(
    state: &Arc<InnerState>,
    app: Option<&AppHandle>,
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
    let mut retest_new_tickets: Vec<String> = Vec::new();
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

    if let Some(qa_step) = qa_step {
        let should_run_qa = evaluate_step_prehook(
            state,
            app,
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
                app,
                task_id,
                item_id,
                "qa",
                qa_step.required_capability.as_deref(),
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                runtime,
            )
            .await?;
            qa_exit_code = Some(result.exit_code);
            qa_failed = result.exit_code != 0;
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

    if let Some(fix_step) = fix_step {
        if fix_step.enabled && !active_tickets.is_empty() {
            let should_run_fix = evaluate_step_prehook(
                state,
                app,
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
                    app,
                    task_id,
                    item_id,
                    "fix",
                    fix_step.required_capability.as_deref(),
                    &item.qa_file_path,
                    &active_tickets,
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    runtime,
                )
                .await?;
                fix_exit_code = Some(result.exit_code);
                fix_success = result.is_success();
                if fix_success {
                    item_status = "fixed".to_string();
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
                app,
                task_id,
                item_id,
                "retest",
                retest_step.required_capability.as_deref(),
                &item.qa_file_path,
                &retest_new_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
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
        total_artifacts: 0,
        has_ticket_artifacts: false,
        has_code_change_artifacts: false,
    };

    if let Some(outcome) = crate::prehook::resolve_workflow_finalize_outcome(
        &task_ctx.execution_plan.finalize,
        &finalize_context,
    )? {
        item_status = outcome.status.clone();
        emit_item_finalize_event(state, app, &finalize_context, &outcome)?;
    }

    let conn = open_conn(&state.db_path)?;
    conn.execute(
        "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![item_id, item_status, now_ts()],
    )?;

    Ok(())
}
