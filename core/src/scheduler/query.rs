use crate::config_load::read_active_config;
use crate::dto::{LogChunk, TaskDetail, TaskSummary};
use crate::runner::redact_text;
use crate::state::InnerState;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};
use std::io::{Read, Seek};
use std::path::Path;

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

pub async fn follow_task_logs(state: &InnerState, task_id: &str) -> Result<()> {
    use tokio::io::AsyncSeekExt;

    let mut stdout_pos: u64 = 0;
    let mut stderr_pos: u64 = 0;
    let mut current_phase = String::new();

    loop {
        let latest = crate::events::query_latest_step_log_paths(&state.db_path, task_id);
        let (phase, stdout_path, stderr_path) = match latest {
            Ok(Some(info)) => info,
            Ok(None) => {
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

        if phase != current_phase {
            if !current_phase.is_empty() {
                eprintln!("\n--- step changed: {} -> {} ---", current_phase, phase);
            }
            current_phase = phase;
            stdout_pos = 0;
            stderr_pos = 0;
        }

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

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        if let Ok(Some(status)) = repo.load_task_status(task_id) {
            if status == "completed" || status == "failed" {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                eprintln!("\n--- task {} ---", status);
                return Ok(());
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

pub async fn watch_task(state: &InnerState, task_id: &str, interval_secs: u64) -> Result<()> {
    let interval = std::time::Duration::from_secs(interval_secs);

    loop {
        print!("\x1b[2J\x1b[H");

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let task = repo.load_task_summary(task_id)?;

        println!(
            "Task: {}  Status: {}  Workflow: {}",
            &task_id[..8.min(task_id.len())],
            colorize_status(&task.status),
            &task.workflow_id,
        );

        let events = crate::events::query_step_events(&state.db_path, task_id)?;
        let active_tickets: i64 = 0;
        let cycle_count = events
            .iter()
            .filter(|e| e.event_type == "cycle_started")
            .count();

        println!("Cycle: {}  Tickets: {}", cycle_count, active_tickets);
        println!("{}", "━".repeat(72));
        println!(
            " {:<15} {:<12} {:<10} {:<9} Details",
            "Step", "Agent", "Status", "Duration"
        );
        println!(
            " {:<15} {:<12} {:<10} {:<9} ──────────────────",
            "───────────────", "────────────", "──────────", "─────────"
        );

        let mut step_states: Vec<StepWatchInfo> = Vec::new();
        for ev in &events {
            match ev.event_type.as_str() {
                "step_started" => {
                    let step = ev.step.clone().unwrap_or_default();
                    let agent = ev.agent_id.clone().unwrap_or_default();
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
                        existing.status = if success {
                            "done".to_string()
                        } else {
                            "failed".to_string()
                        };
                        existing.duration_ms = ev.duration_ms;
                        existing.agent_id =
                            ev.agent_id.clone().unwrap_or(existing.agent_id.clone());
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
                    if let Some(existing) = step_states
                        .iter_mut()
                        .find(|s| s.step == step && s.status == "running")
                    {
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
                if s.agent_id.is_empty() {
                    "-"
                } else {
                    &s.agent_id
                },
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
