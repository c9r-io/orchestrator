use crate::async_database::flatten_err;
use crate::dto::{CreateTaskPayload, LogChunk, TaskDetail, TaskSummary};
use crate::scheduler::{
    delete_task_impl, find_latest_resumable_task_id, follow_task_logs, get_task_details_impl,
    list_tasks_impl, load_task_summary, prepare_task_for_start, resolve_task_id, run_task_loop,
    stop_task_runtime, stop_task_runtime_for_delete, stream_task_logs_impl, RunningTask,
};
use crate::scheduler_service::enqueue_task as enqueue_task_impl;
use crate::state::InnerState;
use crate::task_ops::{create_task_impl, reset_task_item_for_retry};
use anyhow::{Context, Result};
use std::future::Future;
use std::sync::Arc;

/// Create a new task (synchronous — no async DB ops needed).
pub fn create_task(state: &InnerState, payload: CreateTaskPayload) -> Result<TaskSummary> {
    create_task_impl(state, payload)
}

/// Resolve a task ID (prefix match or exact).
pub async fn resolve_id(state: &InnerState, task_id: &str) -> Result<String> {
    resolve_task_id(state, task_id).await
}

/// Resolve task ID for start command (handles --latest flag).
pub async fn resolve_start_id(
    state: &InnerState,
    task_id: Option<&str>,
    latest: bool,
) -> Result<String> {
    if let Some(id) = task_id {
        resolve_task_id(state, id).await
    } else if latest {
        find_latest_resumable_task_id(state, true)
            .await?
            .context("no resumable task found")
    } else {
        anyhow::bail!("task_id or --latest required")
    }
}

/// Enqueue a task for background worker processing.
pub async fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    enqueue_task_impl(state, task_id).await
}

/// Start a task and block until completion.
pub async fn start_task_blocking(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    prepare_task_for_start(&state, task_id).await?;
    let runtime = RunningTask::new();
    run_task_loop(state, task_id, runtime).await
}

/// Load a task summary.
pub async fn load_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    load_task_summary(state, task_id).await
}

/// List all tasks.
pub async fn list_tasks(state: &InnerState) -> Result<Vec<TaskSummary>> {
    list_tasks_impl(state).await
}

/// Get full task detail (task + items + runs + events).
pub async fn get_task_detail(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    get_task_details_impl(state, task_id).await
}

/// Pause a running task.
pub async fn pause_task(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    stop_task_runtime(state, task_id, "paused").await
}

/// Delete a task (stops it first if running).
pub async fn delete_task(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    stop_task_runtime_for_delete(state.clone(), task_id).await?;
    delete_task_impl(&state, task_id).await
}

/// Retry a failed task item. Returns the parent task ID.
pub fn retry_task_item(state: &InnerState, task_item_id: &str) -> Result<String> {
    reset_task_item_for_retry(state, task_item_id)
}

/// Get task logs (non-streaming).
pub async fn get_task_logs(
    state: &InnerState,
    task_id: &str,
    tail: usize,
    timestamps: bool,
) -> Result<Vec<LogChunk>> {
    let resolved_id = resolve_task_id(state, task_id).await?;
    stream_task_logs_impl(state, &resolved_id, tail, timestamps).await
}

/// Follow task logs via a callback for each line.
/// The callback receives each log line as a String.
pub async fn follow_task_logs_stream<F, Fut>(
    state: &InnerState,
    task_id: &str,
    _send_fn: F,
) -> Result<()>
where
    F: Fn(String) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let resolved_id = resolve_task_id(state, task_id).await?;
    // For now, delegate to the existing stdout-based implementation.
    // TODO: Phase 3 — refactor follow_task_logs to use a channel/callback instead of stdout.
    follow_task_logs(state, &resolved_id).await
}

/// Result of a task trace query.
pub struct TaskTraceResult {
    pub entries: Vec<TraceEntry>,
    pub anomalies: Vec<crate::anomaly::Anomaly>,
}

/// A single entry in a task trace timeline.
pub struct TraceEntry {
    pub timestamp: String,
    pub event_type: String,
    pub step: String,
    pub item_id: Option<String>,
    pub payload_json: String,
}

/// Get task trace: event timeline and detected anomalies.
pub async fn get_task_trace(
    state: &InnerState,
    task_id: &str,
    verbose: bool,
) -> Result<TaskTraceResult> {
    let resolved_id = resolve_task_id(state, task_id).await?;
    let task_id_owned = resolved_id.clone();

    let events: Vec<(i64, String, Option<String>, String, String, String)> = state
        .async_database
        .reader()
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, task_id, task_item_id, event_type, payload_json, created_at \
                 FROM events WHERE task_id = ?1 ORDER BY created_at ASC, id ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![task_id_owned], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(flatten_err)?;

    let mut entries = Vec::new();
    let mut anomalies = Vec::new();

    for (_id, _task_id, item_id, event_type, payload_json, created_at) in &events {
        let step = serde_json::from_str::<serde_json::Value>(payload_json)
            .ok()
            .and_then(|v| v.get("step").and_then(|s| s.as_str()).map(String::from))
            .unwrap_or_default();

        if verbose
            || matches!(
                event_type.as_str(),
                "step_started"
                    | "step_finished"
                    | "cycle_started"
                    | "cycle_finished"
                    | "task_started"
                    | "task_finished"
                    | "self_restart_phase"
                    | "self_test_phase"
                    | "self_restart_ready"
                    | "binary_verification"
                    | "anomaly"
            )
        {
            entries.push(TraceEntry {
                timestamp: created_at.clone(),
                event_type: event_type.clone(),
                step: step.clone(),
                item_id: item_id.clone(),
                payload_json: payload_json.clone(),
            });
        }

        // Detect anomalies from event data
        if event_type == "anomaly" {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload_json) {
                let rule = v.get("rule").and_then(|s| s.as_str()).unwrap_or("unknown");
                let severity = v
                    .get("severity")
                    .and_then(|s| s.as_str())
                    .unwrap_or("warning");
                let message = v
                    .get("message")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                anomalies.push(crate::anomaly::Anomaly {
                    rule: rule.to_string(),
                    severity: match severity {
                        "error" => crate::anomaly::Severity::Error,
                        "info" => crate::anomaly::Severity::Info,
                        _ => crate::anomaly::Severity::Warning,
                    },
                    escalation: crate::anomaly::Escalation::Notice,
                    message,
                    at: Some(created_at.clone()),
                });
            }
        }

        // Detect nonzero exit codes as anomalies
        if event_type == "step_finished" {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload_json) {
                if let Some(exit_code) = v.get("exit_code").and_then(|c| c.as_i64()) {
                    if exit_code != 0 {
                        anomalies.push(crate::anomaly::Anomaly::new(
                            crate::anomaly::AnomalyRule::NonzeroExit,
                            format!("step '{}' exited with code {}", step, exit_code),
                            Some(created_at.clone()),
                        ));
                    }
                }
            }
        }
    }

    Ok(TaskTraceResult { entries, anomalies })
}
