#![allow(dead_code)]

use crate::config::{
    ItemFinalizeContext, LoopMode, PipelineVariables, StepPrehookContext, TaskExecutionStep,
    TaskRuntimeContext, WorkflowStepType,
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
use crate::runner::{redact_text, spawn_with_runner};
use crate::selection::{select_agent_advanced, select_agent_by_preference};
use crate::session_store;
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
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::Instant;
use uuid::Uuid;

/// Default step timeout: 30 minutes.  Can be overridden by
/// `safety.step_timeout_secs` in the workflow config.
const DEFAULT_STEP_TIMEOUT_SECS: u64 = 1800;
/// Interval between heartbeat events while a step is running.
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

struct LimitedOutput {
    text: String,
    truncated_prefix_bytes: u64,
}

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
    let redaction_patterns = {
        let active = read_active_config(state)?;
        active.config.runner.redaction_patterns.clone()
    };

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
            redact_text(&stdout_tail, &redaction_patterns),
            if stderr_tail.is_empty() {
                String::new()
            } else {
                format!(
                    "\n[stderr]\n{}",
                    redact_text(&stderr_tail, &redaction_patterns)
                )
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

/// Follow task logs in real-time by tailing the most recent step's stdout/stderr.
pub async fn follow_task_logs(state: &InnerState, task_id: &str) -> Result<()> {
    use tokio::io::AsyncSeekExt;
    let mut stdout_pos: u64 = 0;
    let mut stderr_pos: u64 = 0;
    let mut current_phase = String::new();

    loop {
        // Find the latest running step from events
        let latest = crate::events::query_latest_step_log_paths(
            &state.db_path,
            task_id,
        );
        let (phase, stdout_path, stderr_path) = match latest {
            Ok(Some(info)) => info,
            Ok(None) => {
                // Check if task is still running
                let repo = SqliteTaskRepository::new(state.db_path.clone());
                if let Ok(Some(status)) = repo.load_task_status(task_id) {
                    if status == "completed" || status == "failed" {
                        eprintln!("\n--- task {} ---", status);
                        return Ok(());
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        // Reset positions when step changes
        if phase != current_phase {
            if !current_phase.is_empty() {
                eprintln!("\n--- step changed: {} → {} ---", current_phase, phase);
            }
            current_phase = phase;
            stdout_pos = 0;
            stderr_pos = 0;
        }

        // Tail stdout
        if let Ok(mut f) = tokio::fs::File::open(&stdout_path).await {
            if let Ok(meta) = f.metadata().await {
                if meta.len() > stdout_pos {
                    let _ = f.seek(tokio::io::SeekFrom::Start(stdout_pos)).await;
                    let mut buf = vec![0u8; (meta.len() - stdout_pos) as usize];
                    if let Ok(n) = tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
                        if n > 0 {
                            print!("{}", String::from_utf8_lossy(&buf[..n]));
                            stdout_pos += n as u64;
                        }
                    }
                }
            }
        }

        // Tail stderr (with prefix)
        if let Ok(mut f) = tokio::fs::File::open(&stderr_path).await {
            if let Ok(meta) = f.metadata().await {
                if meta.len() > stderr_pos {
                    let _ = f.seek(tokio::io::SeekFrom::Start(stderr_pos)).await;
                    let mut buf = vec![0u8; (meta.len() - stderr_pos) as usize];
                    if let Ok(n) = tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
                        if n > 0 {
                            eprint!("{}", String::from_utf8_lossy(&buf[..n]));
                            stderr_pos += n as u64;
                        }
                    }
                }
            }
        }

        // Check if task ended
        let repo = SqliteTaskRepository::new(state.db_path.clone());
        if let Ok(Some(status)) = repo.load_task_status(task_id) {
            if status == "completed" || status == "failed" {
                // Final flush
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                eprintln!("\n--- task {} ---", status);
                return Ok(());
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Watch task execution with a real-time status panel.
pub async fn watch_task(state: &InnerState, task_id: &str, interval_secs: u64) -> Result<()> {
    let interval = std::time::Duration::from_secs(interval_secs);

    loop {
        // Clear screen
        print!("\x1b[2J\x1b[H");

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let task = repo.load_task_summary(task_id)?;

        // Header
        println!(
            "Task: {}  Status: {}  Workflow: {}",
            &task_id[..8.min(task_id.len())],
            colorize_status(&task.status),
            &task.workflow_id,
        );

        // Query events for this task
        let events = crate::events::query_step_events(&state.db_path, task_id)?;
        let active_tickets: i64 = 0; // TODO: implement ticket counting when workspace context is available

        // Count cycles
        let cycle_count = events
            .iter()
            .filter(|e| e.event_type == "cycle_started")
            .count();

        println!(
            "Cycle: {}  Tickets: {}",
            cycle_count,
            active_tickets
        );
        println!("{}", "━".repeat(72));
        println!(
            " {:<15} {:<12} {:<10} {:<9} {}",
            "Step", "Agent", "Status", "Duration", "Details"
        );
        println!(
            " {:<15} {:<12} {:<10} {:<9} {}",
            "───────────────",
            "────────────",
            "──────────",
            "─────────",
            "──────────────────"
        );

        // Build step status from events
        let mut step_states: Vec<StepWatchInfo> = Vec::new();
        for ev in &events {
            match ev.event_type.as_str() {
                "step_started" => {
                    let step = ev.step.clone().unwrap_or_default();
                    let agent = ev.agent_id.clone().unwrap_or_default();
                    // Update or create
                    if let Some(existing) = step_states.iter_mut().find(|s| s.step == step) {
                        existing.status = "running".to_string();
                        existing.agent_id = agent;
                        existing.started_at = Some(ev.created_at.clone());
                    } else {
                        step_states.push(StepWatchInfo {
                            step,
                            agent_id: agent,
                            status: "running".to_string(),
                            duration_ms: None,
                            details: String::new(),
                            started_at: Some(ev.created_at.clone()),
                        });
                    }
                }
                "step_finished" => {
                    let step = ev.step.clone().unwrap_or_default();
                    if let Some(existing) = step_states.iter_mut().find(|s| s.step == step) {
                        let success = ev.success.unwrap_or(false);
                        existing.status = if success { "done".to_string() } else { "failed".to_string() };
                        existing.duration_ms = ev.duration_ms;
                        existing.agent_id = ev.agent_id.clone().unwrap_or(existing.agent_id.clone());
                        if let Some(conf) = ev.confidence {
                            existing.details = format!("conf={:.2}", conf);
                        }
                    }
                }
                "step_skipped" => {
                    let step = ev.step.clone().unwrap_or_default();
                    step_states.push(StepWatchInfo {
                        step,
                        agent_id: String::new(),
                        status: "skipped".to_string(),
                        duration_ms: None,
                        details: ev.reason.clone().unwrap_or_default(),
                        started_at: None,
                    });
                }
                "step_heartbeat" => {
                    let step = ev.step.clone().unwrap_or_default();
                    if let Some(existing) = step_states.iter_mut().find(|s| s.step == step && s.status == "running") {
                        let stdout_b = ev.stdout_bytes.unwrap_or(0);
                        let pid = ev.pid.unwrap_or(0);
                        let alive = ev.pid_alive.unwrap_or(false);
                        existing.details = format!(
                            "pid={} {} stdout={}",
                            pid,
                            if alive { "alive" } else { "DEAD" },
                            format_bytes(stdout_b)
                        );
                    }
                }
                _ => {}
            }
        }

        for s in &step_states {
            let duration_str = match s.duration_ms {
                Some(ms) => format_duration(ms),
                None if s.status == "running" => {
                    // Calculate from started_at
                    if let Some(ref ts) = s.started_at {
                        format!("{}...", ts.chars().skip(11).take(8).collect::<String>())
                    } else {
                        "-".to_string()
                    }
                }
                _ => "-".to_string(),
            };
            let status_icon = match s.status.as_str() {
                "done" => "\x1b[32m✓ done\x1b[0m",
                "failed" => "\x1b[31m✗ fail\x1b[0m",
                "running" => "\x1b[33m● run\x1b[0m",
                "skipped" => "\x1b[90m○ skip\x1b[0m",
                _ => &s.status,
            };
            println!(
                " {:<15} {:<12} {:<18} {:<9} {}",
                s.step,
                if s.agent_id.is_empty() { "-" } else { &s.agent_id },
                status_icon,
                duration_str,
                s.details
            );
        }

        println!();

        if task.status == "completed" || task.status == "failed" {
            println!("Task finished: {}", colorize_status(&task.status));
            return Ok(());
        }

        tokio::time::sleep(interval).await;
    }
}

struct StepWatchInfo {
    step: String,
    agent_id: String,
    status: String,
    duration_ms: Option<u64>,
    details: String,
    started_at: Option<String>,
}

fn colorize_status(status: &str) -> String {
    match status {
        "completed" => format!("\x1b[32m{}\x1b[0m", status),
        "failed" => format!("\x1b[31m{}\x1b[0m", status),
        "running" => format!("\x1b[33m{}\x1b[0m", status),
        "paused" => format!("\x1b[90m{}\x1b[0m", status),
        _ => status.to_string(),
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m {}s", mins, secs)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn tail_lines(path: &Path, limit: usize) -> Result<String> {
    if limit == 0 {
        return Ok(String::new());
    }
    const CHUNK_SIZE: usize = 8192;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to read log file: {}", path.display()))?;
    let mut pos = file
        .seek(std::io::SeekFrom::End(0))
        .with_context(|| format!("failed to seek log file: {}", path.display()))?;
    if pos == 0 {
        return Ok(String::new());
    }

    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let mut newline_count = 0usize;
    while pos > 0 && newline_count <= limit {
        let chunk_len = (pos as usize).min(CHUNK_SIZE);
        let start = pos - chunk_len as u64;
        file.seek(std::io::SeekFrom::Start(start))
            .with_context(|| format!("failed to seek log file: {}", path.display()))?;
        let mut buf = vec![0u8; chunk_len];
        file.read_exact(&mut buf)
            .with_context(|| format!("failed to read log file: {}", path.display()))?;
        newline_count += buf.iter().filter(|b| **b == b'\n').count();
        chunks.push(buf);
        pos = start;
    }

    let mut data = Vec::new();
    for chunk in chunks.iter().rev() {
        data.extend_from_slice(chunk);
    }

    if newline_count > limit {
        let mut to_skip = newline_count - limit;
        let mut idx = 0usize;
        while idx < data.len() && to_skip > 0 {
            if data[idx] == b'\n' {
                to_skip -= 1;
            }
            idx += 1;
        }
        data = data[idx..].to_vec();
    }

    Ok(String::from_utf8_lossy(&data).trim_end().to_string())
}

fn persist_task_execution_metric(
    state: &InnerState,
    task_id: &str,
    status: &str,
    current_cycle: u32,
    unresolved_items: i64,
) -> Result<()> {
    let repo = SqliteTaskRepository::new(state.db_path.clone());
    let (total_items, _finished_items, failed_items) = repo.load_task_item_counts(task_id)?;
    let conn = crate::db::open_conn(&state.db_path)?;
    let command_runs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = ?1)",
        rusqlite::params![task_id],
        |row| row.get(0),
    )?;
    let metric = crate::db::TaskExecutionMetric {
        task_id: task_id.to_string(),
        status: status.to_string(),
        current_cycle,
        unresolved_items,
        total_items,
        failed_items,
        command_runs,
        created_at: now_ts(),
    };
    crate::db::insert_task_execution_metric(&state.db_path, &metric)
}

pub fn set_task_status(
    state: &InnerState,
    task_id: &str,
    status: &str,
    set_completed: bool,
) -> Result<()> {
    state
        .db_writer
        .set_task_status(task_id, status, set_completed)
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

    let task_goal = runtime_row.goal;

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
    let safety = workflow.safety.clone();
    let self_referential = active
        .config
        .workspaces
        .get(&workspace_id)
        .map(|ws| ws.self_referential)
        .unwrap_or(false);

    Ok(TaskRuntimeContext {
        workspace_id,
        workspace_root,
        ticket_dir,
        execution_plan,
        current_cycle: current_cycle.max(0) as u32,
        init_done: init_done == 1,
        dynamic_steps,
        pipeline_vars: {
            let mut pv = crate::config::PipelineVariables::default();
            if !task_goal.is_empty() {
                pv.vars.insert("goal".to_string(), task_goal);
            }
            pv
        },
        safety,
        self_referential,
        consecutive_failures: 0,
    })
}

pub async fn run_task_loop(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    set_task_status(&state, task_id, "running", false)?;
    let result = run_task_loop_core(state.clone(), task_id, runtime).await;
    if let Err(ref e) = result {
        let _ = set_task_status(&state, task_id, "failed", false);
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        );
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        );
        let unresolved = count_unresolved_items(&state, task_id).unwrap_or(0);
        let _ = persist_task_execution_metric(&state, task_id, "failed", 0, unresolved);
    }
    result
}

async fn run_task_loop_core(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
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
                    &step.id,
                    "init_once",
                    step.tty,
                    step.required_capability.as_deref(),
                    ".",
                    &[],
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    task_ctx.current_cycle,
                    &runtime,
                    None,
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
            let unresolved = count_unresolved_items(&state, task_id)?;
            persist_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )?;
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
            let unresolved = count_unresolved_items(&state, task_id)?;
            persist_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )?;
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

        // Create checkpoint at cycle start when safety config requires it
        if matches!(
            task_ctx.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        ) {
            let ws_path = Path::new(&task_ctx.workspace_root);
            match create_checkpoint(ws_path, task_id, task_ctx.current_cycle).await {
                Ok(tag) => {
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "checkpoint_created",
                        json!({"cycle": task_ctx.current_cycle, "tag": tag}),
                    )?;
                }
                Err(e) => {
                    eprintln!(
                        "[warn] failed to create checkpoint for cycle {}: {}",
                        task_ctx.current_cycle, e
                    );
                }
            }
        }

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

        // Track consecutive failures for auto-rollback
        let cycle_unresolved = count_unresolved_items(&state, task_id)?;
        if cycle_unresolved > 0 {
            task_ctx.consecutive_failures += 1;
        } else {
            task_ctx.consecutive_failures = 0;
        }

        // Auto-rollback when consecutive failures exceed threshold
        if task_ctx.safety.auto_rollback
            && task_ctx.consecutive_failures >= task_ctx.safety.max_consecutive_failures
            && matches!(
                task_ctx.safety.checkpoint_strategy,
                crate::config::CheckpointStrategy::GitTag
            )
        {
            let rollback_cycle =
                task_ctx.current_cycle.saturating_sub(task_ctx.consecutive_failures);
            let rollback_tag = format!("checkpoint/{}/{}", task_id, rollback_cycle.max(1));
            let ws_path = Path::new(&task_ctx.workspace_root);
            match rollback_to_checkpoint(ws_path, &rollback_tag).await {
                Ok(()) => {
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "auto_rollback",
                        json!({
                            "cycle": task_ctx.current_cycle,
                            "rollback_to": rollback_tag,
                            "consecutive_failures": task_ctx.consecutive_failures,
                        }),
                    )?;
                    state.emit_event(
                        task_id,
                        None,
                        "auto_rollback",
                        json!({"rollback_to": rollback_tag}),
                    );
                    task_ctx.consecutive_failures = 0;
                }
                Err(e) => {
                    eprintln!("[warn] auto-rollback failed: {}", e);
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "auto_rollback_failed",
                        json!({"error": e.to_string()}),
                    )?;
                }
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
                let unresolved = count_unresolved_items(&state, task_id)?;
                persist_task_execution_metric(
                    &state,
                    task_id,
                    "completed",
                    task_ctx.current_cycle,
                    unresolved,
                )?;
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
        persist_task_execution_metric(
            &state,
            task_id,
            "failed",
            task_ctx.current_cycle,
            unresolved,
        )?;
    } else {
        set_task_status(&state, task_id, "completed", true)?;
        insert_event(&state, task_id, None, "task_completed", json!({}))?;
        state.emit_event(task_id, None, "task_completed", json!({}));
        persist_task_execution_metric(
            &state,
            task_id,
            "completed",
            task_ctx.current_cycle,
            unresolved,
        )?;
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
    state
        .db_writer
        .update_task_cycle_state(task_id, current_cycle, init_done)
}

/// Execute a builtin build/test/lint step and produce structured output.
///
/// This runs the command from `step.command` (or falls back to the agent template),
/// captures stdout/stderr, parses build errors / test failures, and updates pipeline
/// variables so downstream steps can reference them via `{build_output}`, `{test_output}`,
/// `{build_errors}`, `{test_failures}`.
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

    // Use step.command if available (builtin), otherwise dispatch to agent
    let result = if let Some(ref _command) = step.command {
        // Render command with pipeline variables
        let ctx = crate::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            task_ctx.current_cycle,
            phase.to_string(),
            task_ctx.workspace_root.clone(),
            task_ctx.workspace_id.clone(),
        );
        let rendered_command =
            ctx.render_template_with_pipeline(_command, Some(&task_ctx.pipeline_vars));

        run_phase(
            state,
            task_id,
            item_id,
            &step.id,
            phase,
            step.tty,
            rendered_command,
            &task_ctx.workspace_root,
            &task_ctx.workspace_id,
            "builtin",
            runtime,
        )
        .await?
    } else {
        run_phase_with_rotation(
            state,
            task_id,
            item_id,
            &step.id,
            phase,
            step.tty,
            step.required_capability.as_deref(),
            ".",
            &[],
            &task_ctx.workspace_root,
            &task_ctx.workspace_id,
            task_ctx.current_cycle,
            runtime,
            Some(&task_ctx.pipeline_vars),
        )
        .await?
    };

    // Build pipeline variables from the result
    let mut pipeline = task_ctx.pipeline_vars.clone();
    if let Some(ref output) = result.output {
        pipeline.prev_stdout = output.stdout.clone();
        pipeline.prev_stderr = output.stderr.clone();
        pipeline.build_errors = output.build_errors.clone();
        pipeline.test_failures = output.test_failures.clone();

        // Store named step outputs for downstream template variables
        // e.g., plan step output → {plan_output}, build step → {build_output}
        let output_key = format!("{}_output", phase);
        if !output.stdout.is_empty() {
            pipeline.vars.insert(output_key, output.stdout.clone());
        }
    }

    // Capture git diff for the current cycle (full diff, not just cached)
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

/// Create a git checkpoint (tag) for rollback support.
/// Called at the start of each cycle when safety.checkpoint_strategy is GitTag.
pub async fn create_checkpoint(
    workspace_root: &Path,
    task_id: &str,
    cycle: u32,
) -> Result<String> {
    let tag_name = format!("checkpoint/{}/{}", task_id, cycle);
    let output = tokio::process::Command::new("git")
        .args(["tag", "-f", &tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to create git checkpoint tag")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag failed: {}", stderr);
    }
    Ok(tag_name)
}

/// Rollback to a previous checkpoint.
pub async fn rollback_to_checkpoint(
    workspace_root: &Path,
    tag_name: &str,
) -> Result<()> {
    let output = tokio::process::Command::new("git")
        .args(["reset", "--hard", tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to rollback to checkpoint")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr);
    }
    Ok(())
}

fn is_task_paused_in_db(state: &InnerState, task_id: &str) -> Result<bool> {
    let status = SqliteTaskRepository::new(state.db_path.clone()).load_task_status(task_id)?;
    Ok(matches!(status.as_deref(), Some("paused")))
}

async fn read_output_with_limit(path: &Path, max_bytes: u64) -> Result<LimitedOutput> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open output log: {}", path.display()))?;
    let file_len = file
        .metadata()
        .await
        .with_context(|| format!("failed to stat output log: {}", path.display()))?
        .len();

    let start = file_len.saturating_sub(max_bytes);
    if start > 0 {
        file.seek(SeekFrom::Start(start))
            .await
            .with_context(|| format!("failed to seek output log: {}", path.display()))?;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .await
        .with_context(|| format!("failed to read output log: {}", path.display()))?;
    Ok(LimitedOutput {
        text: String::from_utf8_lossy(&buf).into_owned(),
        truncated_prefix_bytes: start,
    })
}

pub async fn run_phase(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    tty: bool,
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
    let redaction_patterns = runner.redaction_patterns.clone();
    if !logs_dir.starts_with(&state.logs_dir) {
        return Err(anyhow::anyhow!(
            "logs dir escapes managed root: {}",
            logs_dir.display()
        ));
    }

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

    let mut session_id: Option<String> = None;
    let command_to_run = if tty {
        let sid = Uuid::new_v4().to_string();
        let session_dir = state.logs_dir.join("sessions").join(&sid);
        std::fs::create_dir_all(&session_dir).with_context(|| {
            format!("failed to create session dir: {}", session_dir.display())
        })?;
        let input_fifo = session_dir.join("input.fifo");
        let transcript_path = session_dir.join("transcript.log");
        let output_json_path = session_dir.join("output.json");
        if !input_fifo.exists() {
            let status = std::process::Command::new("mkfifo")
                .arg(&input_fifo)
                .status()
                .with_context(|| format!("failed to spawn mkfifo for {}", input_fifo.display()))?;
            if !status.success() {
                anyhow::bail!("mkfifo failed for {}", input_fifo.display());
            }
        }
        let inner = format!(
            "ORCH_OUTPUT_JSON_PATH={} ORCH_SESSION_ID={} ORCH_STEP_ID={} {}",
            shell_escape(&output_json_path.to_string_lossy()),
            shell_escape(&sid),
            shell_escape(step_id),
            command
        );
        let wrapped = format!(
            "{} < {}",
            inner,
            shell_escape(&input_fifo.to_string_lossy())
        );
        session_id = Some(sid.clone());
        session_store::insert_session(
            &state.db_path,
            &session_store::NewSession {
                id: &sid,
                task_id,
                task_item_id: Some(item_id),
                step_id,
                phase,
                agent_id,
                state: "active",
                pid: 0,
                pty_backend: "script",
                cwd: &workspace_root.to_string_lossy(),
                command: &command,
                input_fifo_path: &input_fifo.to_string_lossy(),
                stdout_path: &stdout_path.to_string_lossy(),
                stderr_path: &stderr_path.to_string_lossy(),
                transcript_path: &transcript_path.to_string_lossy(),
                output_json_path: Some(&output_json_path.to_string_lossy()),
            },
        )?;
        wrapped
    } else {
        command.clone()
    };
    let child =
        spawn_with_runner(&runner, &command_to_run, workspace_root, stdout_file, stderr_file)?;
    if let Some(sid) = session_id.as_deref() {
        if let Some(pid) = child.id() {
            let _ = session_store::update_session_pid(&state.db_path, sid, pid as i64);
        }
    }

    // TTY sessions run in the background – return immediately so the user
    // can attach via `exec -it`.  The child process reads from a FIFO and
    // would block indefinitely if we waited here.
    if tty && session_id.is_some() {
        // Leak the child handle to prevent kill_on_drop from sending SIGKILL
        // when the CLI process (and tokio runtime) exits.  The session process
        // PID is already persisted in agent_sessions and can be managed via
        // `task session close`.
        std::mem::forget(child);
        return Ok(crate::dto::RunResult {
            success: true,
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            timed_out: false,
            duration_ms: Some(0),
            output: None,
            validation_status: "passed".to_string(),
            validation_error: None,
            agent_id: agent_id.to_string(),
            run_id: run_id.clone(),
        });
    }

    let child_pid = child.id();

    // Emit step_spawned event with PID and command preview
    {
        let preview: String = command.chars().take(120).collect();
        insert_event(
            state,
            task_id,
            Some(item_id),
            "step_spawned",
            json!({
                "step": phase,
                "step_id": step_id,
                "agent_id": agent_id,
                "run_id": run_id,
                "pid": child_pid,
                "command_preview": preview,
            }),
        )?;
    }

    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = Some(child);
    }

    // Resolve step timeout from workflow safety config (fallback to default)
    let step_timeout_secs = {
        let active = read_active_config(state)?;
        active
            .config
            .workflows
            .values()
            .next()
            .and_then(|w| w.safety.step_timeout_secs)
            .unwrap_or(DEFAULT_STEP_TIMEOUT_SECS)
    };

    let start = Instant::now();
    let deadline = start + std::time::Duration::from_secs(step_timeout_secs);
    let heartbeat_interval = std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS);
    let mut timed_out = false;

    let exit_code: i32 = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // Timeout — kill the child process
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                let _ = child.kill().await;
            }
            timed_out = true;
            insert_event(
                state,
                task_id,
                Some(item_id),
                "step_timeout",
                json!({
                    "step": phase,
                    "step_id": step_id,
                    "timeout_secs": step_timeout_secs,
                    "pid": child_pid,
                }),
            )?;
            break -4;
        }

        let wait_duration = heartbeat_interval.min(remaining);
        let wait_result = {
            let mut child_lock = runtime.child.lock().await;
            if let Some(ref mut child) = *child_lock {
                tokio::time::timeout(wait_duration, child.wait()).await
            } else {
                break -3;
            }
        };

        match wait_result {
            Ok(Ok(status)) => {
                // Child exited normally
                break status.code().unwrap_or(-1);
            }
            Ok(Err(e)) => {
                // IO error waiting for child
                break if e.kind() == std::io::ErrorKind::NotFound {
                    -2
                } else {
                    -3
                };
            }
            Err(_) => {
                // Timeout on this interval → emit heartbeat
                let elapsed = start.elapsed();
                let stdout_bytes = tokio::fs::metadata(&stdout_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let stderr_bytes = tokio::fs::metadata(&stderr_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let pid_alive = child_pid
                    .map(|pid| {
                        // Check if process exists by sending signal 0
                        std::process::Command::new("kill")
                            .args(["-0", &pid.to_string()])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);

                insert_event(
                    state,
                    task_id,
                    Some(item_id),
                    "step_heartbeat",
                    json!({
                        "step": phase,
                        "step_id": step_id,
                        "elapsed_secs": elapsed.as_secs(),
                        "stdout_bytes": stdout_bytes,
                        "stderr_bytes": stderr_bytes,
                        "pid": child_pid,
                        "pid_alive": pid_alive,
                    }),
                )?;
            }
        }
    };

    let duration = start.elapsed();
    {
        let mut child_lock = runtime.child.lock().await;
        *child_lock = None;
    }

    const MAX_PHASE_OUTPUT_BYTES: u64 = 256 * 1024;
    let stdout_output = read_output_with_limit(&stdout_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stdout log: {}", stdout_path.display()))?;
    let stderr_output = read_output_with_limit(&stderr_path, MAX_PHASE_OUTPUT_BYTES)
        .await
        .with_context(|| format!("failed to read stderr log: {}", stderr_path.display()))?;
    let stdout_content = stdout_output.text;
    let stderr_content = stderr_output.text;

    let validation = validate_phase_output(
        phase,
        run_uuid,
        agent_id,
        exit_code as i64,
        &stdout_content,
        &stderr_content,
    )?;
    let mut success = exit_code == 0;
    let mut validation_event_payload_json: Option<String> = None;
    if validation.status == "failed" {
        success = false;
        validation_event_payload_json = Some(serde_json::to_string(&json!({
            "phase": phase,
            "run_id": run_id.clone(),
            "error": validation.error.clone(),
            "stdout_truncated_prefix_bytes": stdout_output.truncated_prefix_bytes,
            "stderr_truncated_prefix_bytes": stderr_output.truncated_prefix_bytes
        }))?);
    }

    let mut redacted_output = validation.output.clone();
    redacted_output.stdout = redact_text(&redacted_output.stdout, &redaction_patterns);
    redacted_output.stderr = redact_text(&redacted_output.stderr, &redaction_patterns);

    let writer = state.db_writer.clone();
    let task_id_owned = task_id.to_string();
    let item_id_owned = item_id.to_string();
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
        output_json: serde_json::to_string(&redacted_output)?,
        artifacts_json: serde_json::to_string(&redacted_output.artifacts)?,
        confidence: Some(redacted_output.confidence),
        quality_score: Some(redacted_output.quality_score),
        validation_status: validation.status.to_string(),
        session_id: session_id.clone(),
        machine_output_source: if tty {
            "output_json_path".to_string()
        } else {
            "stdout".to_string()
        },
        output_json_path: session_id
            .as_ref()
            .map(|sid| state.logs_dir.join("sessions").join(sid).join("output.json"))
            .map(|p| p.to_string_lossy().to_string()),
    };
    let sender = crate::collab::AgentEndpoint::for_task_item(agent_id, task_id, item_id);
    let msg = crate::collab::AgentMessage::publish(
        sender,
        crate::collab::MessagePayload::ExecutionResult(crate::collab::ExecutionResult {
            run_id: run_uuid,
            output: redacted_output.clone(),
            success,
            error: validation.error.clone(),
        }),
    );
    let (publish_event_type, publish_event_payload_json) = if let Err(err) =
        state.message_bus.publish(msg).await
    {
        (
            "bus_publish_failed",
            serde_json::to_string(&json!({"phase":phase,"run_id":run_id,"error":err.to_string()}))?,
        )
    } else {
        (
            "phase_output_published",
            serde_json::to_string(&json!({"phase":phase,"run_id":run_id}))?,
        )
    };

    tokio::task::spawn_blocking(move || {
        let mut events = Vec::with_capacity(2);
        if let Some(payload_json) = validation_event_payload_json.as_deref() {
            events.push(crate::db_write::DbEventRecord {
                task_id: task_id_owned.as_str(),
                task_item_id: Some(item_id_owned.as_str()),
                event_type: "output_validation_failed",
                payload_json,
            });
        }
        events.push(crate::db_write::DbEventRecord {
            task_id: task_id_owned.as_str(),
            task_item_id: Some(item_id_owned.as_str()),
            event_type: publish_event_type,
            payload_json: publish_event_payload_json.as_str(),
        });
        writer.persist_phase_result_with_events(&insert_payload, &events)
    })
    .await
    .context("command run insert worker failed")??;

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
    if let Some(sid) = session_id.as_deref() {
        let _ = session_store::update_session_state(
            &state.db_path,
            sid,
            "closed",
            Some(exit_code as i64),
            true,
        );
    }

    Ok(crate::dto::RunResult {
        success,
        exit_code: exit_code as i64,
        stdout_path: stdout_path.to_string_lossy().to_string(),
        stderr_path: stderr_path.to_string_lossy().to_string(),
        timed_out,
        duration_ms: Some(duration_ms),
        output: Some(redacted_output),
        validation_status: validation.status.to_string(),
        validation_error: validation
            .error
            .map(|value| redact_text(&value, &redaction_patterns)),
        agent_id: agent_id.to_string(),
        run_id,
    })
}

pub async fn run_phase_with_rotation(
    state: &Arc<InnerState>,
    task_id: &str,
    item_id: &str,
    step_id: &str,
    phase: &str,
    tty: bool,
    capability: Option<&str>,
    rel_path: &str,
    ticket_paths: &[String],
    workspace_root: &Path,
    workspace_id: &str,
    cycle: u32,
    runtime: &RunningTask,
    pipeline_vars: Option<&PipelineVariables>,
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
    let mut command = template
        .replace("{rel_path}", &shell_escape(rel_path))
        .replace("{ticket_paths}", &escaped_paths.join(" "))
        .replace("{phase}", phase)
        .replace("{cycle}", &cycle.to_string());

    // Render pipeline variables (source_tree, build_errors, goal, etc.) into agent template
    if pipeline_vars.is_some() || command.contains("{source_tree}") || command.contains("{workspace_root}") {
        let ctx = crate::collab::AgentContext::new(
            task_id.to_string(),
            item_id.to_string(),
            cycle,
            phase.to_string(),
            workspace_root.to_path_buf(),
            workspace_id.to_string(),
        );
        command = ctx.render_template_with_pipeline(&command, pipeline_vars);
    }

    run_phase(
        state,
        task_id,
        item_id,
        step_id,
        phase,
        tty,
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
        &step.id,
        "guard",
        step.tty,
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
                task_id,
                item_id,
                &plan_step.id,
                "plan",
                plan_step.tty,
                plan_step.required_capability.as_deref(),
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
                Some(&task_ctx.pipeline_vars),
            )
            .await?;
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
                &qa_step.id,
                "qa",
                qa_step.tty,
                qa_step.required_capability.as_deref(),
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
                None,
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
                    build_error_count: 0,
                    test_failure_count: 0,
                    build_exit_code: None,
                    test_exit_code: None,
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
                    &fix_step.id,
                    "fix",
                    fix_step.tty,
                    fix_step.required_capability.as_deref(),
                    &item.qa_file_path,
                    &active_tickets,
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    task_ctx.current_cycle,
                    runtime,
                    None,
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
                &retest_step.id,
                "retest",
                retest_step.tty,
                retest_step.required_capability.as_deref(),
                &item.qa_file_path,
                &retest_new_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
                None,
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

    // Process extended step types — any step that isn't handled by the standard
    // (init_once, plan, qa, ticket_scan, retest, loop_guard) path above.
    // This includes Build, Test, Lint, Implement, GitOps, Review, QaDocGen,
    // QaTesting, TicketFix, DocGovernance, AlignTests, and any future custom types.
    // Fix is also handled here when the ticket-based fix path didn't run.
    // Steps are iterated in execution plan order for pipeline variable propagation.
    for step in &task_ctx.execution_plan.steps {
        if step.is_guard {
            continue;
        }
        let step_type = step.step_type.as_ref().map(|t| t.as_str()).unwrap_or("");
        let is_standard_step = matches!(
            step_type,
            "" | "init_once" | "plan" | "qa" | "ticket_scan" | "retest" | "loop_guard"
        );
        // Handle fix in extended loop when it hasn't been handled by the ticket-based path.
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

        // Evaluate prehook if present
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

        // Build a temporary task_ctx with updated pipeline vars for this step
        let mut step_ctx = task_ctx.clone();
        step_ctx.pipeline_vars = pipeline_vars.clone();

        let (result, new_pipeline) =
            execute_builtin_step(state, task_id, item_id, step, &step_ctx, runtime).await?;

        // Update pipeline variables for downstream steps
        pipeline_vars = new_pipeline;

        // Track build/test exit codes for prehook evaluation
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

        {
            let confidence = result.output.as_ref().map(|o| o.confidence).unwrap_or(0.0);
            let quality = result.output.as_ref().map(|o| o.quality_score).unwrap_or(0.0);
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
                &ds.id,
                &ds.step_type,
                false,
                cap,
                &item.qa_file_path,
                &active_tickets,
                &task_ctx.workspace_root,
                &task_ctx.workspace_id,
                task_ctx.current_cycle,
                runtime,
                None,
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
