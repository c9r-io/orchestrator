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
    /// Full structured trace (built via build_trace with complete anomaly detection).
    pub full_trace: Option<crate::scheduler::trace::TaskTrace>,
}

/// A single entry in a task trace timeline.
pub struct TraceEntry {
    pub timestamp: String,
    pub event_type: String,
    pub step: String,
    pub item_id: Option<String>,
    pub payload_json: String,
}

/// Get task trace: event timeline, detected anomalies, and full structured trace.
pub async fn get_task_trace(
    state: &InnerState,
    task_id: &str,
    verbose: bool,
) -> Result<TaskTraceResult> {
    let resolved_id = resolve_task_id(state, task_id).await?;

    // Fetch full task detail (events + command_runs) for build_trace
    let detail = get_task_details_impl(state, &resolved_id).await?;

    // Build the full structured trace with comprehensive anomaly detection
    let event_dtos: Vec<crate::dto::EventDto> = detail.events;
    let command_run_dtos: Vec<crate::dto::CommandRunDto> = detail.runs;

    let full_trace = crate::scheduler::trace::build_trace_with_meta(
        crate::scheduler::trace::TraceTaskMeta {
            task_id: &detail.task.id,
            status: &detail.task.status,
            created_at: &detail.task.created_at,
            started_at: detail.task.started_at.as_deref(),
            completed_at: detail.task.completed_at.as_deref(),
            updated_at: &detail.task.updated_at,
        },
        &event_dtos,
        &command_run_dtos,
    );

    // Build timeline entries for terminal rendering
    let mut entries = Vec::new();
    for event in &event_dtos {
        let step = event
            .payload
            .get("step")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let payload_json =
            serde_json::to_string(&event.payload).unwrap_or_else(|_| "{}".to_string());

        if verbose
            || matches!(
                event.event_type.as_str(),
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
                timestamp: event.created_at.clone(),
                event_type: event.event_type.clone(),
                step,
                item_id: event.task_item_id.clone(),
                payload_json,
            });
        }
    }

    // Use anomalies from the full trace builder (includes low_output, long_running, etc.)
    let anomalies = full_trace.anomalies.clone();

    Ok(TaskTraceResult {
        entries,
        anomalies,
        full_trace: Some(full_trace),
    })
}
