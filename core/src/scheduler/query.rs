use crate::anomaly::AnomalyRule;
use crate::config_load::read_loaded_config;
use crate::dto::{LogChunk, TaskDetail, TaskSummary};
use crate::runner::redact_text;
use crate::state::InnerState;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::io::{Read, Seek};
use std::path::Path;
use std::time::{Duration, Instant};

const QUERY_RETRY_ATTEMPTS: usize = 3;
const QUERY_RETRY_DELAY_MS: u64 = 75;
const FOLLOW_POLL_MS: u64 = 500;
const FOLLOW_WARNING_THROTTLE_SECS: u64 = 5;
const LOG_UNAVAILABLE_MARKER: &str = "[log unavailable]";

fn is_transient_query_error(err: &anyhow::Error) -> bool {
    let message = err.to_string();
    [
        "database is locked",
        "failed to open sqlite db",
        "failed to read log file",
        "failed to seek log file",
        "read stdout tail",
        "read stderr tail",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn retry_query<T, F>(label: &str, f: F) -> Result<T>
where
    F: Fn() -> Result<T>,
{
    let mut last_err = None;
    for attempt in 0..QUERY_RETRY_ATTEMPTS {
        match f() {
            Ok(value) => return Ok(value),
            Err(err) if is_transient_query_error(&err) && attempt + 1 < QUERY_RETRY_ATTEMPTS => {
                last_err = Some(err);
                std::thread::sleep(Duration::from_millis(QUERY_RETRY_DELAY_MS));
            }
            Err(err) => {
                return Err(err).with_context(|| format!("{label} failed"));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{label} failed")))
        .with_context(|| format!("{label} failed"))
}

pub fn resolve_task_id(state: &InnerState, task_id: &str) -> Result<String> {
    SqliteTaskRepository::new(state.db_path.clone()).resolve_task_id(task_id)
}

pub fn load_task_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    retry_query("load task summary", || {
        let resolved_id = resolve_task_id(state, task_id)?;
        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let mut summary = repo.load_task_summary(&resolved_id)?;
        let (total, finished, failed) = repo.load_task_item_counts(&resolved_id)?;

        summary.total_items = total;
        summary.finished_items = finished;
        summary.failed_items = failed;
        Ok(summary)
    })
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
    load_task_detail_snapshot(state, task_id)
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
    let runs = retry_query("list task log runs", || {
        repo.list_task_log_runs(&resolved_id, 14)
    })?;
    let redaction_patterns = {
        let active = read_loaded_config(state)?;
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
            .with_context(|| format!("read stdout tail for run_id={run_id} path={stdout_path}"))
            .ok()
            .unwrap_or_default();
        let stderr_tail = tail_lines(Path::new(&stderr_path), PER_FILE_LINE_LIMIT)
            .with_context(|| format!("read stderr tail for run_id={run_id} path={stderr_path}"))
            .ok()
            .unwrap_or_default();

        let header = if show_timestamps {
            let ts = started_at.as_deref().unwrap_or("unknown");
            format!("[{}][{}][{}]", ts, run_id, phase)
        } else {
            format!("[{}][{}]", run_id, phase)
        };

        let log_body = if stdout_tail.is_empty() && stderr_tail.is_empty() {
            LOG_UNAVAILABLE_MARKER.to_string()
        } else {
            redact_text(&stdout_tail, &redaction_patterns)
        };
        let content = format!(
            "{}\n{}{}",
            header,
            log_body,
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
    let mut stdout_pos: u64 = 0;
    let mut stderr_pos: u64 = 0;
    let mut current_phase = String::new();
    let mut waiting_notice_printed = false;
    let mut last_warning_at: Option<Instant> = None;

    loop {
        let latest = crate::events::query_latest_step_log_paths(&state.db_path, task_id);
        let (phase, stdout_path, stderr_path) = match latest {
            Ok(Some(info)) => info,
            Ok(None) => {
                if !waiting_notice_printed {
                    eprintln!("[waiting for first log stream]");
                    waiting_notice_printed = true;
                }
                let repo = SqliteTaskRepository::new(state.db_path.clone());
                if let Ok(Some(status)) = repo.load_task_status(task_id) {
                    if status == "completed" || status == "failed" {
                        eprintln!("\n--- task {} ---", status);
                        return Ok(());
                    }
                }
                tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
                continue;
            }
            Err(err) => {
                emit_anomaly_warning(
                    &AnomalyRule::TransientReadError,
                    &format!("{err}"),
                    &mut last_warning_at,
                );
                tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
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
        waiting_notice_printed = false;

        if let Err(err) = follow_one_stream(&stdout_path, &mut stdout_pos, false).await {
            emit_anomaly_warning(
                &AnomalyRule::TransientReadError,
                &format!("{err}"),
                &mut last_warning_at,
            );
        }

        if let Err(err) = follow_one_stream(&stderr_path, &mut stderr_pos, true).await {
            emit_anomaly_warning(
                &AnomalyRule::TransientReadError,
                &format!("{err}"),
                &mut last_warning_at,
            );
        }

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        if let Ok(Some(status)) = repo.load_task_status(task_id) {
            if status == "completed" || status == "failed" {
                tokio::time::sleep(Duration::from_millis(200)).await;
                eprintln!("\n--- task {} ---", status);
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(FOLLOW_POLL_MS)).await;
    }
}

pub async fn watch_task(state: &InnerState, task_id: &str, interval_secs: u64) -> Result<()> {
    let interval = Duration::from_secs(interval_secs);
    let mut last_warning: Option<String> = None;

    loop {
        let task = match load_task_summary(state, task_id) {
            Ok(task) => task,
            Err(err) if is_transient_query_error(&err) => {
                let rule = AnomalyRule::TransientReadError;
                let warning = format!(
                    "[{}: {}] {err}",
                    rule.escalation().label(),
                    rule.canonical_name(),
                );
                if last_warning.as_deref() != Some(&warning) {
                    eprintln!("{warning}");
                    last_warning = Some(warning);
                }
                tokio::time::sleep(interval).await;
                continue;
            }
            Err(err) => return Err(err),
        };
        let events = match retry_query("query step events", || {
            crate::events::query_step_events(&state.db_path, task_id)
        }) {
            Ok(events) => events,
            Err(err) if is_transient_query_error(&err) => {
                let rule = AnomalyRule::TransientReadError;
                let warning = format!(
                    "[{}: {}] {err}",
                    rule.escalation().label(),
                    rule.canonical_name(),
                );
                if last_warning.as_deref() != Some(&warning) {
                    eprintln!("{warning}");
                    last_warning = Some(warning);
                }
                tokio::time::sleep(interval).await;
                continue;
            }
            Err(err) => return Err(err),
        };

        let frame = render_watch_frame(&task, &events, task_id);
        print!("\x1b[2J\x1b[H{frame}");
        last_warning = None;

        if task.status == "completed" || task.status == "failed" {
            return Ok(());
        }

        tokio::time::sleep(interval).await;
    }
}

struct StepWatchInfo {
    step: String,
    scope: Option<crate::events::ObservedStepScope>,
    binding_item_id: Option<String>,
    agent_id: String,
    status: String,
    duration_ms: Option<u64>,
    details: String,
    started_at: Option<String>,
}

fn load_task_detail_snapshot(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    retry_query("load task details", || {
        let task = load_task_summary(state, task_id)?;
        let repo = SqliteTaskRepository::new(state.db_path.clone());
        let (items, runs, events) = repo.load_task_detail_rows(&task.id)?;

        Ok(TaskDetail {
            task,
            items,
            runs,
            events,
        })
    })
}

async fn follow_one_stream(path: &str, pos: &mut u64, stderr: bool) -> Result<()> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncSeekExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("open stream {path}"))?;
    let meta = file
        .metadata()
        .await
        .with_context(|| format!("stat stream {path}"))?;
    if meta.len() <= *pos {
        return Ok(());
    }

    file.seek(tokio::io::SeekFrom::Start(*pos))
        .await
        .with_context(|| format!("seek stream {path}"))?;
    let mut buf = vec![0u8; (meta.len() - *pos) as usize];
    let read = file
        .read(&mut buf)
        .await
        .with_context(|| format!("read stream {path}"))?;
    if read == 0 {
        return Ok(());
    }

    if stderr {
        eprint!("{}", String::from_utf8_lossy(&buf[..read]));
    } else {
        print!("{}", String::from_utf8_lossy(&buf[..read]));
    }
    *pos += read as u64;
    Ok(())
}

fn emit_anomaly_warning(rule: &AnomalyRule, message: &str, last_warning_at: &mut Option<Instant>) {
    let should_print = last_warning_at
        .map(|at| at.elapsed() >= Duration::from_secs(FOLLOW_WARNING_THROTTLE_SECS))
        .unwrap_or(true);
    if should_print {
        eprintln!(
            "[{}: {}] {}",
            rule.escalation().label(),
            rule.canonical_name(),
            message,
        );
        *last_warning_at = Some(Instant::now());
    }
}

#[derive(Default)]
struct WatchAnomalyCounts {
    intervene: u32,
    attention: u32,
    notice: u32,
}

impl WatchAnomalyCounts {
    fn total(&self) -> u32 {
        self.intervene + self.attention + self.notice
    }
}

fn render_watch_frame(
    task: &TaskSummary,
    events: &[crate::events::StepEvent],
    task_id: &str,
) -> String {
    let mut frame = String::new();
    let _ = writeln!(
        frame,
        "Task: {}  Status: {}  Workflow: {}",
        &task_id[..8.min(task_id.len())],
        colorize_status(&task.status),
        &task.workflow_id,
    );

    let cycle_count = events
        .iter()
        .filter(|e| e.event_type == "cycle_started")
        .count();
    let _ = writeln!(frame, "Cycle: {}  Tickets: {}", cycle_count, 0);
    let _ = writeln!(frame, "{}", "━".repeat(72));
    let _ = writeln!(
        frame,
        " {:<15} {:<7} {:<12} {:<10} {:<9} Details",
        "Step", "Scope", "Agent", "Status", "Duration"
    );
    let _ = writeln!(
        frame,
        " {:<15} {:<7} {:<12} {:<10} {:<9} ─────────────",
        "───────────────", "───────", "────────────", "──────────", "─────────"
    );

    let mut step_states: Vec<StepWatchInfo> = Vec::new();
    let mut watch_anomaly_counts = WatchAnomalyCounts::default();
    for ev in events {
        match ev.event_type.as_str() {
            "step_started" => {
                let step = ev.step.clone().unwrap_or_default();
                let agent = ev.agent_id.clone().unwrap_or_default();
                if let Some(existing) = step_states.iter_mut().find(|s| s.step == step) {
                    existing.scope = ev.step_scope;
                    existing.binding_item_id = ev.task_item_id.clone();
                    existing.status = "running".to_string();
                    existing.agent_id = agent;
                    existing.started_at = Some(ev.created_at.clone());
                } else {
                    step_states.push(StepWatchInfo {
                        step,
                        scope: ev.step_scope,
                        binding_item_id: ev.task_item_id.clone(),
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
                    if ev.step_scope.is_some() {
                        existing.scope = ev.step_scope;
                    }
                    if ev.task_item_id.is_some() {
                        existing.binding_item_id = ev.task_item_id.clone();
                    }
                    let success = ev.success.unwrap_or(false);
                    existing.status = if success {
                        "done".to_string()
                    } else {
                        "failed".to_string()
                    };
                    existing.duration_ms = ev.duration_ms;
                    existing.agent_id = ev.agent_id.clone().unwrap_or(existing.agent_id.clone());
                    if let Some(conf) = ev.confidence {
                        existing.details = format!("conf={:.2}", conf);
                    }
                }
            }
            "step_skipped" => {
                step_states.push(StepWatchInfo {
                    step: ev.step.clone().unwrap_or_default(),
                    scope: ev.step_scope,
                    binding_item_id: ev.task_item_id.clone(),
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
                    if ev.step_scope.is_some() {
                        existing.scope = ev.step_scope;
                    }
                    if ev.task_item_id.is_some() {
                        existing.binding_item_id = ev.task_item_id.clone();
                    }
                    let stdout_b = ev.stdout_bytes.unwrap_or(0);
                    let stderr_b = ev.stderr_bytes.unwrap_or(0);
                    let stdout_delta_b = ev.stdout_delta_bytes.unwrap_or(0);
                    let stderr_delta_b = ev.stderr_delta_bytes.unwrap_or(0);
                    let total_delta = stdout_delta_b + stderr_delta_b;
                    let pid = ev.pid.unwrap_or(0);
                    let alive = ev.pid_alive.unwrap_or(false);
                    let elapsed = ev.elapsed_secs.unwrap_or(0);

                    let lo_rule = AnomalyRule::LowOutput;
                    let lr_rule = AnomalyRule::LongRunning;

                    existing.details = match ev.output_state.as_deref() {
                        Some("low_output") => {
                            watch_anomaly_counts.intervene += 1;
                            format!(
                                "{} [{}] pid={} {} out={} err={} Δ={} quiet={}",
                                lo_rule.display_tag(),
                                lo_rule.escalation().label(),
                                pid,
                                if alive { "alive" } else { "DEAD" },
                                format_bytes(stdout_b),
                                format_bytes(stderr_b),
                                format_bytes(total_delta),
                                ev.stagnant_heartbeats.unwrap_or(0)
                            )
                        }
                        Some(state) => format!(
                            "pid={} {} out={} err={} Δ={} state={}",
                            pid,
                            if alive { "alive" } else { "DEAD" },
                            format_bytes(stdout_b),
                            format_bytes(stderr_b),
                            format_bytes(total_delta),
                            state
                        ),
                        None => format!(
                            "pid={} {} stdout={}",
                            pid,
                            if alive { "alive" } else { "DEAD" },
                            format_bytes(stdout_b)
                        ),
                    };

                    if elapsed > 600 && ev.output_state.as_deref() != Some("low_output") {
                        watch_anomaly_counts.notice += 1;
                        existing.details.push_str(&format!(
                            " {} [{}]",
                            lr_rule.display_tag(),
                            lr_rule.escalation().label(),
                        ));
                    }

                    if existing.scope == Some(crate::events::ObservedStepScope::Task) {
                        if let Some(anchor_item_id) = &existing.binding_item_id {
                            existing
                                .details
                                .push_str(&format!(" anchor={anchor_item_id}"));
                        }
                    }
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
        let _ = writeln!(
            frame,
            " {:<15} {:<7} {:<12} {:<18} {:<9} {}",
            s.step,
            match s.scope {
                Some(scope) => crate::events::observed_step_scope_label(Some(scope)),
                None => "?",
            },
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

    let _ = writeln!(frame);
    if watch_anomaly_counts.total() > 0 {
        let _ = writeln!(
            frame,
            "Anomalies: {} intervene, {} attention, {} notice",
            watch_anomaly_counts.intervene,
            watch_anomaly_counts.attention,
            watch_anomaly_counts.notice,
        );
    }
    if task.status == "completed" || task.status == "failed" {
        let _ = writeln!(frame, "Task finished: {}", colorize_status(&task.status));
    }
    frame
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

pub(crate) fn tail_lines(path: &Path, limit: usize) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("query-test-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn format_duration_milliseconds() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(999), "999ms");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(1000), "1.0s");
        assert_eq!(format_duration(1500), "1.5s");
        assert_eq!(format_duration(59_999), "60.0s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60_000), "1m 0s");
        assert_eq!(format_duration(90_000), "1m 30s");
        assert_eq!(format_duration(3_661_000), "61m 1s");
    }

    #[test]
    fn format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1023), "1023B");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0KB");
        assert_eq!(format_bytes(1536), "1.5KB");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(1024 * 1024), "1.0MB");
        assert_eq!(format_bytes(1024 * 1024 * 5), "5.0MB");
    }

    #[test]
    fn colorize_status_completed() {
        let result = colorize_status("completed");
        assert!(result.contains("completed"));
        assert!(result.contains("\x1b[32m")); // green
    }

    #[test]
    fn colorize_status_failed() {
        let result = colorize_status("failed");
        assert!(result.contains("failed"));
        assert!(result.contains("\x1b[31m")); // red
    }

    #[test]
    fn colorize_status_running() {
        let result = colorize_status("running");
        assert!(result.contains("\x1b[33m")); // yellow
    }

    #[test]
    fn colorize_status_paused() {
        let result = colorize_status("paused");
        assert!(result.contains("\x1b[90m")); // gray
    }

    #[test]
    fn colorize_status_unknown_passes_through() {
        assert_eq!(colorize_status("pending"), "pending");
        assert_eq!(colorize_status("other"), "other");
    }

    #[test]
    fn tail_lines_zero_limit_returns_empty() {
        let dir = test_dir("zero");
        let path = dir.join("log.txt");
        std::fs::write(&path, "line1\nline2\n").unwrap();
        let result = tail_lines(&path, 0).unwrap();
        assert_eq!(result, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_empty_file_returns_empty() {
        let dir = test_dir("empty");
        let path = dir.join("log.txt");
        std::fs::write(&path, "").unwrap();
        let result = tail_lines(&path, 10).unwrap();
        assert_eq!(result, "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_returns_last_n_lines() {
        let dir = test_dir("lastn");
        let path = dir.join("log.txt");
        // Use trailing newline so each "line" is terminated
        let content = (1..=20)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, &content).unwrap();

        let result = tail_lines(&path, 3).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line 18");
        assert_eq!(lines[2], "line 20");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_returns_all_when_limit_exceeds_file() {
        let dir = test_dir("exceed");
        let path = dir.join("log.txt");
        std::fs::write(&path, "line1\nline2\nline3").unwrap();

        let result = tail_lines(&path, 100).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tail_lines_missing_file_returns_error() {
        let result = tail_lines(Path::new("/nonexistent/path"), 10);
        assert!(result.is_err());
    }

    #[test]
    fn tail_lines_large_file() {
        let dir = test_dir("large");
        let path = dir.join("big.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        for i in 0..500 {
            writeln!(f, "line {:04}", i).unwrap();
        }
        drop(f);

        let result = tail_lines(&path, 5).unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], "line 0499");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Tests using TestState ──────────────────────────────────────────

    use crate::config_load::now_ts;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::task_repository::{NewCommandRun, SqliteTaskRepository, TaskRepository};
    use crate::test_utils::TestState;

    /// Helper: create a TestState, seed a QA file, create a task, return (state, task_id).
    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/query_test.md");
        std::fs::write(&qa_file, "# query test\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("query-test".to_string()),
                goal: Some("query-test-goal".to_string()),
                ..Default::default()
            },
        )
        .expect("task should be created");
        (state, created.id)
    }

    /// Helper: get the first task_item id for a given task.
    fn first_item_id(state: &crate::state::InnerState, task_id: &str) -> String {
        let conn = crate::db::open_conn(&state.db_path).expect("open db");
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("task item should exist")
    }

    #[test]
    fn resolve_task_id_exact_match() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let resolved = resolve_task_id(&state, &task_id).expect("resolve exact id");
        assert_eq!(resolved, task_id);
    }

    #[test]
    fn resolve_task_id_prefix_match() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let prefix = &task_id[..8];
        let resolved = resolve_task_id(&state, prefix).expect("resolve prefix id");
        assert_eq!(resolved, task_id);
    }

    #[test]
    fn resolve_task_id_not_found() {
        let mut fixture = TestState::new();
        let (state, _task_id) = seed_task(&mut fixture);
        let result = resolve_task_id(&state, "nonexistent-id-00000000");
        assert!(result.is_err());
    }

    #[test]
    fn load_task_summary_returns_counts() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let summary = load_task_summary(&state, &task_id).expect("load task summary");
        assert_eq!(summary.id, task_id);
        assert_eq!(summary.name, "query-test");
        assert_eq!(summary.goal, "query-test-goal");
        // The task should have at least 1 item (the seeded qa file)
        assert!(summary.total_items >= 1, "expected at least 1 total_items");
        // Initially nothing is finished or failed
        assert_eq!(summary.finished_items, 0);
        assert_eq!(summary.failed_items, 0);
    }

    #[test]
    fn load_task_summary_with_prefix() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let prefix = &task_id[..8];
        let summary = load_task_summary(&state, prefix).expect("load summary by prefix");
        assert_eq!(summary.id, task_id);
    }

    #[test]
    fn list_tasks_impl_returns_seeded_task() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].name, "query-test");
    }

    #[test]
    fn list_tasks_impl_empty_when_no_tasks() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_tasks_impl_multiple_tasks_ordered_desc() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/multi_test.md");
        std::fs::write(&qa_file, "# multi test\n").expect("seed qa file");

        let t1 = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-1".to_string()),
                ..Default::default()
            },
        )
        .expect("create task 1");

        let t2 = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-2".to_string()),
                ..Default::default()
            },
        )
        .expect("create task 2");

        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert_eq!(tasks.len(), 2);
        // Most recent first
        assert_eq!(tasks[0].id, t2.id);
        assert_eq!(tasks[1].id, t1.id);
    }

    #[test]
    fn get_task_details_impl_returns_items_and_empty_runs() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let detail = get_task_details_impl(&state, &task_id).expect("get task details");
        assert_eq!(detail.task.id, task_id);
        assert!(!detail.items.is_empty(), "should have at least 1 item");
        // No command runs yet
        assert!(detail.runs.is_empty());
    }

    #[test]
    fn get_task_details_impl_with_command_run() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("details-run");
        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "output").unwrap();
        std::fs::write(&stderr_path, "").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-detail-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo test".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let detail = get_task_details_impl(&state, &task_id).expect("get task details");
        assert_eq!(detail.runs.len(), 1);
        assert_eq!(detail.runs[0].id, "run-detail-1");
        assert_eq!(detail.runs[0].phase, "qa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_task_impl_removes_task_and_log_files() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Create log files on disk
        let dir = test_dir("delete-logs");
        let stdout_path = dir.join("delete_stdout.log");
        let stderr_path = dir.join("delete_stderr.log");
        std::fs::write(&stdout_path, "stdout data").unwrap();
        std::fs::write(&stderr_path, "stderr data").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-delete-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo delete".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        assert!(stdout_path.exists());
        assert!(stderr_path.exists());

        delete_task_impl(&state, &task_id).expect("delete task");

        // Log files should be cleaned up
        assert!(!stdout_path.exists(), "stdout log should be deleted");
        assert!(!stderr_path.exists(), "stderr log should be deleted");

        // Task should no longer be listable
        let tasks = list_tasks_impl(&state).expect("list after delete");
        assert!(tasks.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_task_impl_nonexistent_returns_error() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let result = delete_task_impl(&state, "nonexistent-task-id");
        assert!(result.is_err());
    }

    #[test]
    fn stream_task_logs_impl_returns_log_chunks() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Create actual log files on disk
        let dir = test_dir("stream-logs");
        let stdout_path = dir.join("stream_stdout.log");
        let stderr_path = dir.join("stream_stderr.log");
        std::fs::write(&stdout_path, "line 1\nline 2\nline 3\n").unwrap();
        std::fs::write(&stderr_path, "").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-stream-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo stream".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].run_id, "run-stream-1");
        assert_eq!(chunks[0].phase, "qa");
        assert!(chunks[0].content.contains("line 1"));
        assert!(chunks[0].content.contains("line 3"));
        // No stderr section since stderr is empty
        assert!(!chunks[0].content.contains("[stderr]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_task_logs_impl_works_when_active_config_is_not_runnable() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-invalid-active");
        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "token=redacted\nvisible line\n").unwrap();
        std::fs::write(&stderr_path, "").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-invalid-active-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo stream".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        *state.active_config_error.write().unwrap() =
            Some("active config is not runnable".to_string());

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("[REDACTED]"));
        assert!(chunks[0].content.contains("visible line"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_task_logs_impl_with_stderr() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-stderr");
        let stdout_path = dir.join("out.log");
        let stderr_path = dir.join("err.log");
        std::fs::write(&stdout_path, "stdout content\n").unwrap();
        std::fs::write(&stderr_path, "warning: something\n").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-stream-err".to_string(),
            task_item_id: item_id,
            phase: "implement".to_string(),
            command: "echo err".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 1,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("stdout content"));
        assert!(chunks[0].content.contains("[stderr]"));
        assert!(chunks[0].content.contains("warning: something"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_task_logs_impl_with_timestamps() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-ts");
        let stdout_path = dir.join("ts_out.log");
        let stderr_path = dir.join("ts_err.log");
        std::fs::write(&stdout_path, "data\n").unwrap();
        std::fs::write(&stderr_path, "").unwrap();

        let ts = now_ts();
        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-ts-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo ts".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: ts.clone(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let chunks =
            stream_task_logs_impl(&state, &task_id, 10, true).expect("stream with timestamps");
        assert_eq!(chunks.len(), 1);
        // When show_timestamps is true, header includes the timestamp
        assert!(
            chunks[0].content.contains(&ts),
            "content should include timestamp"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_task_logs_impl_tail_count_limits_output() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("stream-tail");
        let repo = SqliteTaskRepository::new(state.db_path.clone());

        // Insert 3 command runs with distinct log files
        for i in 0..3 {
            let stdout_path = dir.join(format!("tail_out_{}.log", i));
            let stderr_path = dir.join(format!("tail_err_{}.log", i));
            std::fs::write(&stdout_path, format!("run {} output\n", i)).unwrap();
            std::fs::write(&stderr_path, "").unwrap();

            repo.insert_command_run(&NewCommandRun {
                id: format!("run-tail-{}", i),
                task_item_id: item_id.clone(),
                phase: "qa".to_string(),
                command: format!("echo {}", i),
                cwd: "/tmp".to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.to_string_lossy().to_string(),
                stderr_path: stderr_path.to_string_lossy().to_string(),
                started_at: format!("2026-01-01T00:00:0{}Z", i),
                ended_at: now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: None,
                quality_score: None,
                validation_status: "unknown".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
            })
            .expect("insert command run");
        }

        // Request only 2 tail entries
        let chunks =
            stream_task_logs_impl(&state, &task_id, 2, false).expect("stream with tail limit");
        assert_eq!(chunks.len(), 2, "should be limited to 2 chunks");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stream_task_logs_impl_no_runs_returns_empty() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream empty logs");
        assert!(chunks.is_empty());
    }

    #[test]
    fn stream_task_logs_impl_returns_placeholder_when_logs_missing() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-missing-logs".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo missing".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: "/nonexistent/stdout.log".to_string(),
            stderr_path: "/nonexistent/stderr.log".to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains(LOG_UNAVAILABLE_MARKER));
    }

    #[test]
    fn stream_task_logs_impl_returns_partial_results_when_one_run_is_unavailable() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);
        let dir = test_dir("partial-logs");

        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "available output\n").unwrap();
        std::fs::write(&stderr_path, "").unwrap();

        let repo = SqliteTaskRepository::new(state.db_path.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-partial-good".to_string(),
            task_item_id: item_id.clone(),
            phase: "qa".to_string(),
            command: "echo ok".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert good run");
        repo.insert_command_run(&NewCommandRun {
            id: "run-partial-missing".to_string(),
            task_item_id: item_id,
            phase: "implement".to_string(),
            command: "echo missing".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: "/nonexistent/stdout.log".to_string(),
            stderr_path: "/nonexistent/stderr.log".to_string(),
            started_at: "2026-01-01T00:00:01Z".to_string(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert missing run");

        let chunks = stream_task_logs_impl(&state, &task_id, 10, false).expect("stream task logs");
        assert_eq!(chunks.len(), 2);
        assert!(chunks
            .iter()
            .any(|chunk| chunk.content.contains("available output")));
        assert!(chunks
            .iter()
            .any(|chunk| chunk.content.contains(LOG_UNAVAILABLE_MARKER)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_query_retries_transient_error_then_succeeds() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let value = retry_query("transient test", move || {
            let attempt = attempts_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt < 2 {
                Err(anyhow::anyhow!("database is locked"))
            } else {
                Ok(42)
            }
        })
        .expect("retry query should succeed");

        assert_eq!(value, 42);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn retry_query_does_not_retry_permanent_error() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_closure = attempts.clone();

        let result: Result<i32> = retry_query("permanent test", move || {
            attempts_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(anyhow::anyhow!("task not found: deadbeef"))
        });

        assert!(result.is_err());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn render_watch_frame_includes_running_step_and_cycle() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:01Z".to_string(),
        };
        let events = vec![
            crate::events::StepEvent {
                event_type: "cycle_started".to_string(),
                step: None,
                step_scope: None,
                task_item_id: None,
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:00Z".to_string(),
            },
            crate::events::StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(crate::events::ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains("Task: 12345678"));
        assert!(frame.contains("Cycle: 1"));
        assert!(frame.contains("Scope"));
        assert!(frame.contains("plan"));
        assert!(frame.contains(" task "));
        assert!(frame.contains("echo"));
    }

    #[test]
    fn render_watch_frame_shows_low_output_details_for_heartbeat() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:01:31Z".to_string(),
        };
        let events = vec![
            crate::events::StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(crate::events::ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
            crate::events::StepEvent {
                event_type: "step_heartbeat".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(crate::events::ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: Some(90),
                stdout_bytes: Some(137),
                stderr_bytes: Some(0),
                stdout_delta_bytes: Some(0),
                stderr_delta_bytes: Some(0),
                stagnant_heartbeats: Some(3),
                pid: Some(4321),
                pid_alive: Some(true),
                output_state: Some("low_output".to_string()),
                created_at: "2026-03-01T00:01:31Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains("LOW_OUTPUT"), "should contain LOW_OUTPUT tag");
        assert!(frame.contains("[INTERVENE]"), "should contain escalation tag");
        assert!(frame.contains("Δ=0B"));
        assert!(frame.contains("quiet=3"));
        assert!(frame.contains("anchor=item-1"));
        assert!(
            frame.contains("Anomalies: 1 intervene"),
            "should show anomaly summary"
        );
    }

    #[test]
    fn render_watch_frame_keeps_active_state_for_active_heartbeat() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:31Z".to_string(),
        };
        let events = vec![
            crate::events::StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(crate::events::ObservedStepScope::Item),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
            crate::events::StepEvent {
                event_type: "step_heartbeat".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(crate::events::ObservedStepScope::Item),
                task_item_id: Some("item-1".to_string()),
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: Some(30),
                stdout_bytes: Some(256),
                stderr_bytes: Some(0),
                stdout_delta_bytes: Some(64),
                stderr_delta_bytes: Some(0),
                stagnant_heartbeats: Some(0),
                pid: Some(4321),
                pid_alive: Some(true),
                output_state: Some("active".to_string()),
                created_at: "2026-03-01T00:00:31Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains(" item "));
        assert!(frame.contains("state=active"));
        assert!(!frame.contains("LOW OUTPUT"));
    }

    #[test]
    fn render_watch_frame_shows_unknown_scope_for_legacy_event() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:01Z".to_string(),
        };
        let events = vec![crate::events::StepEvent {
            event_type: "step_started".to_string(),
            step: Some("plan".to_string()),
            step_scope: None,
            task_item_id: Some("item-1".to_string()),
            agent_id: Some("echo".to_string()),
            success: None,
            duration_ms: None,
            confidence: None,
            reason: None,
            elapsed_secs: None,
            stdout_bytes: None,
            stderr_bytes: None,
            stdout_delta_bytes: None,
            stderr_delta_bytes: None,
            stagnant_heartbeats: None,
            pid: None,
            pid_alive: None,
            output_state: None,
            created_at: "2026-03-01T00:00:01Z".to_string(),
        }];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains(" ? "));
    }
}
