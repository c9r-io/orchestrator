use crate::scheduler::{
    RunningTask, delete_task_impl, find_latest_resumable_task_id, follow_task_logs,
    get_task_details_impl, list_tasks_impl, load_task_summary, prepare_task_for_start,
    resolve_task_id, run_task_loop, stop_task_runtime, stop_task_runtime_for_delete,
    stream_task_logs_impl,
};
use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use agent_orchestrator::config_load::read_loaded_config;
use agent_orchestrator::dto::{CreateTaskPayload, LogChunk, TaskDetail, TaskSummary};
use agent_orchestrator::env_resolve::collect_all_sensitive_store_values;
use agent_orchestrator::error::{OrchestratorError, Result, classify_task_error};
use agent_orchestrator::events::insert_event;
use agent_orchestrator::persistence::repository::{SchedulerRepository, SqliteSchedulerRepository};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::task_ops::{create_task_impl, reset_task_item_for_retry};
use anyhow::Context;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

/// Create a new task (synchronous — no async DB ops needed).
pub fn create_task(state: &InnerState, payload: CreateTaskPayload) -> Result<TaskSummary> {
    create_task_impl(state, payload).map_err(|err| classify_task_error("task.create", err))
}

/// Resolve a task ID (prefix match or exact).
pub async fn resolve_id(state: &InnerState, task_id: &str) -> Result<String> {
    resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.resolve_id", err))
}

/// Resolve task ID for start command (handles --latest flag).
pub async fn resolve_start_id(
    state: &InnerState,
    task_id: Option<&str>,
    latest: bool,
) -> Result<String> {
    if let Some(id) = task_id {
        resolve_task_id(state, id)
            .await
            .map_err(|err| classify_task_error("task.start", err))
    } else if latest {
        find_latest_resumable_task_id(state, true)
            .await?
            .context("no resumable task found")
            .map_err(|err| classify_task_error("task.start", err))
    } else {
        Err(OrchestratorError::user_input(
            "task.start",
            anyhow::anyhow!("task_id or --latest required"),
        ))
    }
}

/// Marks a task as pending and wakes the background worker.
///
/// Resets unresolved items when resuming from paused/failed status.
/// This is the canonical enqueue implementation — called by the service layer
/// and by [`SchedulerTaskEnqueuer`] (the [`TaskEnqueuer`] port impl).
async fn enqueue_task_inner(state: &InnerState, task_id: &str) -> anyhow::Result<()> {
    state.task_repo.reset_unresolved_items(task_id).await?;
    state
        .db_writer
        .set_task_status(task_id, "pending", false)
        .await?;
    state.worker_notify.notify_waiters();
    insert_event(
        state,
        task_id,
        None,
        "scheduler_enqueued",
        json!({"task_id": task_id}),
    )
    .await?;
    Ok(())
}

/// Enqueue a task for background worker processing.
pub async fn enqueue_task(state: &InnerState, task_id: &str) -> Result<()> {
    enqueue_task_inner(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.enqueue", err))
}

/// Returns the next pending task identifier without claiming it.
pub async fn next_pending_task_id(state: &InnerState) -> anyhow::Result<Option<String>> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .next_pending_task_id()
        .await
}

/// Claims the next pending task and transitions it to running.
pub async fn claim_next_pending_task(state: &InnerState) -> anyhow::Result<Option<String>> {
    SqliteSchedulerRepository::new(state.async_database.clone())
        .claim_next_pending_task()
        .await
}

/// Concrete [`TaskEnqueuer`](agent_orchestrator::scheduler_port::TaskEnqueuer)
/// implementation backed by the scheduler's enqueue logic.
///
/// Injected into [`InnerState`] by the daemon at startup so that core modules
/// (e.g. `trigger_engine`) can enqueue tasks without a direct crate dependency
/// on `orchestrator-scheduler`.
pub struct SchedulerTaskEnqueuer;

#[async_trait::async_trait]
impl agent_orchestrator::scheduler_port::TaskEnqueuer for SchedulerTaskEnqueuer {
    async fn enqueue_task(
        &self,
        state: &InnerState,
        task_id: &str,
    ) -> agent_orchestrator::error::Result<()> {
        enqueue_task_inner(state, task_id)
            .await
            .map_err(|err| classify_task_error("task.enqueue", err))
    }
}

/// Start a task and block until completion.
pub async fn start_task_blocking(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    prepare_task_for_start(&state, task_id)
        .await
        .map_err(|err| classify_task_error("task.start_blocking", err))?;
    let runtime = RunningTask::new();
    run_task_loop(state, task_id, runtime)
        .await
        .map_err(|err| classify_task_error("task.start_blocking", err))
}

/// Load a task summary.
pub async fn load_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    load_task_summary(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.summary", err))
}

/// List all tasks.
pub async fn list_tasks(state: &InnerState) -> Result<Vec<TaskSummary>> {
    list_tasks_impl(state)
        .await
        .map_err(|err| classify_task_error("task.list", err))
}

/// Get full task detail (task + items + runs + events).
pub async fn get_task_detail(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    get_task_details_impl(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.info", err))
}

/// Builds one cursor-paginated semantic process timeline.
pub async fn get_task_timeline(
    state: &InnerState,
    task_id: &str,
    query: crate::scheduler::timeline::TimelineQuery,
) -> Result<crate::scheduler::timeline::TimelinePage> {
    let started = Instant::now();
    let resolved_id = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.timeline", err))?;
    let watermark = crate::scheduler::timeline::cursor_watermark(query.cursor.as_deref())
        .map_err(|err| OrchestratorError::user_input("task.timeline", err))?;
    let source = state
        .task_repo
        .load_task_timeline_source(&resolved_id, watermark)
        .await
        .map_err(|err| classify_task_error("task.timeline", err))?;
    let patterns = timeline_redaction_patterns(state, &source.task.project_id)
        .map_err(|err| classify_task_error("task.timeline", err))?;
    let page = crate::scheduler::timeline::build_timeline_page(&source, &query, &patterns)
        .map_err(|err| OrchestratorError::user_input("task.timeline", err))?;
    let duration_seconds = started.elapsed().as_secs_f64();
    let response_bytes = serde_json::to_vec(&page).map_or(0, |value| value.len()) as f64;
    record_timeline_metrics(
        state,
        &source.task.project_id,
        duration_seconds,
        response_bytes,
    );
    tracing::info!(
        task_id = %resolved_id,
        source_events = source.events.len(),
        source_runs = source.runs.len(),
        entries = page.entries.len(),
        has_more = page.has_more,
        projection_version = page.projection_version,
        duration_ms = (duration_seconds * 1_000.0) as u64,
        "projected task timeline"
    );
    Ok(page)
}

fn record_timeline_metrics(
    state: &InnerState,
    project_id: &str,
    duration_seconds: f64,
    response_bytes: f64,
) {
    let Ok(active) = read_loaded_config(state) else {
        return;
    };
    if !active
        .config
        .runtime_policy_for_project(project_id)
        .observability
        .process_metrics
        .enabled
    {
        return;
    }
    let repository = agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
        state.async_database.clone(),
    );
    let project_id = project_id.to_string();
    let source_key = uuid::Uuid::new_v4().to_string();
    tokio::spawn(async move {
        for (metric_name, value) in [
            ("timeline_projection_seconds", duration_seconds),
            ("timeline_response_bytes", response_bytes),
        ] {
            if let Err(error) = repository
                .record(agent_orchestrator::process_metrics::MetricObservation {
                    project_id: project_id.clone(),
                    metric_name: metric_name.to_string(),
                    dimensions: std::collections::BTreeMap::new(),
                    value,
                    occurred_at: agent_orchestrator::config_load::now_ts(),
                    source_kind: "timeline_projection".to_string(),
                    source_key: source_key.clone(),
                })
                .await
            {
                tracing::warn!(error = %error, "failed to record timeline metric");
            }
        }
    });
}

/// Builds timeline upserts produced by events newer than the supplied watermark.
pub async fn get_task_timeline_updates(
    state: &InnerState,
    task_id: &str,
    after_event_id: i64,
    categories: &[String],
) -> Result<(i64, Vec<crate::scheduler::timeline::TimelineEntry>)> {
    let resolved_id = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.timeline_follow", err))?;
    let source = state
        .task_repo
        .load_task_timeline_source(&resolved_id, None)
        .await
        .map_err(|err| classify_task_error("task.timeline_follow", err))?;
    let patterns = timeline_redaction_patterns(state, &source.task.project_id)
        .map_err(|err| classify_task_error("task.timeline_follow", err))?;
    let updates = crate::scheduler::timeline::build_timeline_updates(
        &source,
        after_event_id,
        categories,
        &patterns,
    )
    .map_err(|err| OrchestratorError::user_input("task.timeline_follow", err))?;
    Ok((source.snapshot_max_event_id, updates))
}

fn timeline_redaction_patterns(
    state: &InnerState,
    project_id: &str,
) -> anyhow::Result<Vec<String>> {
    let active = read_loaded_config(state)?;
    let mut patterns = active
        .config
        .runtime_policy_for_project(project_id)
        .runner
        .redaction_patterns;
    let effective = active.config.effective_project_id(Some(project_id));
    if let Some(project) = active.config.projects.get(effective) {
        patterns.extend(collect_all_sensitive_store_values(&project.secret_stores));
    }
    Ok(patterns)
}

/// Pause a running task.
pub async fn pause_task(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    stop_task_runtime(state, task_id, "paused")
        .await
        .map_err(|err| classify_task_error("task.pause", err))
}

/// Delete a task (stops it first if running).
///
/// The reference check runs *before* the runtime is stopped. Stopping first and
/// discovering the refusal afterwards would leave the operator with a task that
/// is neither running nor deleted, having asked for one thing and got a third.
/// The delete re-derives the same check under its own transaction, so this is a
/// courtesy to the caller and never the authority.
pub async fn delete_task(state: Arc<InnerState>, task_id: &str) -> Result<()> {
    let resolved = resolve_task_id(&state, task_id)
        .await
        .map_err(|err| classify_task_error("task.delete", err))?;
    let blocked_by = state
        .task_repo
        .references_blocking_delete(&resolved)
        .await
        .map_err(|err| classify_task_error("task.delete", err))?;
    if !blocked_by.is_empty() {
        return Err(classify_task_error(
            "task.delete",
            anyhow::anyhow!(
                "task {resolved} is still referenced by {}, and no disposition is recorded \
                 for {}; delete refused and the task was left running",
                blocked_by.join(", "),
                if blocked_by.len() == 1 { "it" } else { "them" },
            ),
        ));
    }

    stop_task_runtime_for_delete(state.clone(), task_id)
        .await
        .map_err(|err| classify_task_error("task.delete", err))?;
    delete_task_impl(&state, task_id)
        .await
        .map_err(|err| classify_task_error("task.delete", err))
}

/// Recover orphaned running items for a task. Returns recovered item IDs.
pub async fn recover_task(state: &InnerState, task_id: &str) -> Result<Vec<String>> {
    let resolved = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.recover", err))?;
    let recovered = state
        .task_repo
        .recover_orphaned_running_items_for_task(&resolved)
        .await
        .map_err(|err| classify_task_error("task.recover", err))?;
    if !recovered.is_empty() {
        state.worker_notify.notify_waiters();
    }
    Ok(recovered)
}

/// Retry a failed task item. Returns the parent task ID.
pub fn retry_task_item(state: &InnerState, task_item_id: &str) -> Result<String> {
    reset_task_item_for_retry(state, task_item_id)
        .map_err(|err| classify_task_error("task.retry", err))
}

/// Get task logs (non-streaming).
pub async fn get_task_logs(
    state: &InnerState,
    task_id: &str,
    tail: usize,
    timestamps: bool,
) -> Result<Vec<LogChunk>> {
    let resolved_id = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.logs", err))?;
    stream_task_logs_impl(state, &resolved_id, tail, timestamps)
        .await
        .map_err(|err| classify_task_error("task.logs", err))
}

/// Follow task logs via a synchronous callback for each chunk.
///
/// `send_fn(text, is_stderr)` is called for every log chunk.
pub async fn follow_task_logs_stream<F>(
    state: &InnerState,
    task_id: &str,
    mut send_fn: F,
) -> Result<()>
where
    F: FnMut(String, bool) -> anyhow::Result<()>,
{
    let resolved_id = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.follow", err))?;
    follow_task_logs(state, &resolved_id, &mut send_fn)
        .await
        .map_err(|err| classify_task_error("task.follow", err))
}

/// Result of a task trace query.
pub struct TaskTraceResult {
    /// Timeline entries selected for rendering.
    pub entries: Vec<TraceEntry>,
    /// Anomalies detected while building the structured trace.
    pub anomalies: Vec<agent_orchestrator::anomaly::Anomaly>,
    /// Full structured trace (built via build_trace with complete anomaly detection).
    pub full_trace: Option<crate::scheduler::trace::TaskTrace>,
}

/// A single entry in a task trace timeline.
pub struct TraceEntry {
    /// Timestamp when the event was emitted.
    pub timestamp: String,
    /// Event type identifier.
    pub event_type: String,
    /// Step identifier associated with the event, if any.
    pub step: String,
    /// Task item identifier associated with the event, if any.
    pub item_id: Option<String>,
    /// Serialized event payload.
    pub payload_json: String,
}

/// Get task trace: event timeline, detected anomalies, and full structured trace.
pub async fn get_task_trace(
    state: &InnerState,
    task_id: &str,
    verbose: bool,
) -> Result<TaskTraceResult> {
    let resolved_id = resolve_task_id(state, task_id)
        .await
        .map_err(|err| classify_task_error("task.trace", err))?;

    // Fetch full task detail (events + command_runs) for build_trace
    let detail = get_task_details_impl(state, &resolved_id)
        .await
        .map_err(|err| classify_task_error("task.trace", err))?;

    // Build the full structured trace with comprehensive anomaly detection
    let event_dtos: Vec<agent_orchestrator::dto::EventDto> = detail.events;
    let command_run_dtos: Vec<agent_orchestrator::dto::CommandRunDto> = detail.runs;

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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_orchestrator::events::insert_event;
    use agent_orchestrator::task_ops::create_task_impl;
    use agent_orchestrator::task_repository::NewCommandRun;
    use agent_orchestrator::test_utils::TestState;

    fn seed_log_files(state: &InnerState, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = state.data_dir.join("logs").join(name);
        std::fs::create_dir_all(&dir).expect("create log dir");
        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        (stdout_path, stderr_path)
    }

    fn seed_task(fixture: &mut TestState) -> (Arc<InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/service-task-fixture.md");
        std::fs::write(&qa_file, "# service task fixture\n").expect("seed qa file");
        // FR-094: pass an explicit target file so task creation goes
        // through the `Explicit` strategy and does NOT trigger
        // QaDirectoryScan.  Otherwise the diagnostic events emitted by
        // `create_task_impl` would pollute the events table that
        // downstream service::task tests assert against.
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("service-task-fixture".to_string()),
                goal: Some("service-task-goal".to_string()),
                target_files: Some(vec!["docs/qa/service-task-fixture.md".to_string()]),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create fixture task");
        (state, created.id)
    }

    fn get_item_id(state: &InnerState, task_id: &str) -> String {
        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("load task item id")
    }

    #[tokio::test]
    async fn create_task_and_query_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/service-task.md");
        std::fs::write(&qa_file, "# service task\n").expect("seed qa file");

        let created = create_task(
            &state,
            CreateTaskPayload {
                name: Some("service-task".to_string()),
                goal: Some("service goal".to_string()),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create task through service");

        let resolved = resolve_id(&state, &created.id[..8])
            .await
            .expect("resolve task prefix");
        assert_eq!(resolved, created.id);

        let summary = load_summary(&state, &created.id)
            .await
            .expect("load summary");
        assert_eq!(summary.name, "service-task");

        let tasks = list_tasks(&state).await.expect("list tasks");
        assert_eq!(tasks.len(), 1);

        let detail = get_task_detail(&state, &created.id)
            .await
            .expect("get detail");
        assert_eq!(detail.task.goal, "service goal");
        assert!(!detail.items.is_empty());
    }

    #[tokio::test]
    async fn resolve_start_id_supports_explicit_latest_and_error() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);

        let explicit = resolve_start_id(&state, Some(&task_id), false)
            .await
            .expect("resolve explicit task");
        assert_eq!(explicit, task_id);

        let latest = resolve_start_id(&state, None, true)
            .await
            .expect("resolve latest task");
        assert_eq!(latest, task_id);

        let err = resolve_start_id(&state, None, false)
            .await
            .expect_err("missing args should fail");
        assert!(err.to_string().contains("task_id or --latest required"));
    }

    #[tokio::test]
    async fn get_task_logs_reads_existing_command_output() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = get_item_id(&state, &task_id);
        let (stdout_path, stderr_path) = seed_log_files(&state, "task-service-logs");
        std::fs::write(&stdout_path, "first line\nsecond line\n").expect("write stdout");
        std::fs::write(&stderr_path, "warn line\n").expect("write stderr");

        state
            .task_repo
            .insert_command_run(NewCommandRun {
                id: "run-task-service".to_string(),
                task_item_id: item_id,
                phase: "qa".to_string(),
                command: "echo hi".to_string(),
                command_template: None,
                cwd: state.data_dir.display().to_string(),
                workspace_id: "default".to_string(),
                agent_id: "echo".to_string(),
                exit_code: 0,
                stdout_path: stdout_path.display().to_string(),
                stderr_path: stderr_path.display().to_string(),
                started_at: agent_orchestrator::config_load::now_ts(),
                ended_at: agent_orchestrator::config_load::now_ts(),
                interrupted: 0,
                output_json: "{}".to_string(),
                artifacts_json: "[]".to_string(),
                confidence: Some(1.0),
                quality_score: Some(1.0),
                validation_status: "passed".to_string(),
                session_id: None,
                machine_output_source: "stdout".to_string(),
                output_json_path: None,
                command_rule_index: None,
            })
            .await
            .expect("insert command run");

        let chunks = get_task_logs(&state, &task_id[..8], 50, true)
            .await
            .expect("stream task logs");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("first line"));
        assert!(chunks[0].content.contains("warn line"));
    }

    #[tokio::test]
    #[ignore = "release-mode deterministic performance fixture"]
    async fn large_timeline_meets_projection_budget() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        let tx = conn.unchecked_transaction().expect("timeline transaction");
        for index in 0..50_000_u64 {
            tx.execute(
                "INSERT INTO events(task_id,event_type,payload_json,created_at)
                 VALUES(?1,'step_finished',?2,'2026-07-14T00:00:00Z')",
                rusqlite::params![task_id, format!(r#"{{"step":"test","sequence":{index}}}"#)],
            )
            .expect("seed timeline event");
        }
        tx.commit().expect("commit timeline fixture");
        let started = Instant::now();
        let page = get_task_timeline(
            &state,
            &task_id,
            crate::scheduler::timeline::TimelineQuery {
                cursor: None,
                limit: 200,
                categories: Vec::new(),
            },
        )
        .await
        .expect("project large timeline");
        let elapsed = started.elapsed();
        let bytes = serde_json::to_vec(&page).expect("serialize timeline").len();
        assert_eq!(page.entries.len(), 200);
        assert!(page.has_more);
        assert!(
            elapsed <= std::time::Duration::from_millis(750),
            "timeline projection exceeded 750ms: {elapsed:?}"
        );
        assert!(
            bytes <= 512 * 1024,
            "timeline response exceeded 512KiB: {bytes}"
        );
    }

    #[tokio::test]
    async fn retry_task_item_resets_item_and_delete_task_removes_parent() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = get_item_id(&state, &task_id);

        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE task_items SET status = 'failed', fix_required = 1, fixed = 1, last_error = 'boom' WHERE id = ?1",
            rusqlite::params![item_id],
        )
        .expect("seed failed item");

        let parent_id = retry_task_item(&state, &item_id).expect("retry task item");
        assert_eq!(parent_id, task_id);

        let status: String = conn
            .query_row(
                "SELECT status FROM task_items WHERE id = ?1",
                rusqlite::params![item_id],
                |row| row.get(0),
            )
            .expect("reload status");
        assert_eq!(status, "pending");

        delete_task(state.clone(), &task_id)
            .await
            .expect("delete task");
        let remaining = list_tasks(&state).await.expect("list after delete");
        assert!(remaining.is_empty());
    }

    /// FR-168: a task carrying a handoff deletes, through the operator's path.
    ///
    /// This is the acceptance criterion as written — a task with a
    /// `handoff_snapshots` row went through `orchestrator task delete` and
    /// failed with a bare `FOREIGN KEY constraint failed`. The repository-level
    /// suite asserts the disposition; this asserts that the service path an
    /// operator actually reaches gets the same answer, which is a different
    /// claim: `delete_task` has its own reference check and its own ordering.
    #[tokio::test]
    async fn a_task_with_a_handoff_deletes_through_the_service_path() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "INSERT INTO handoff_snapshots (id, project_id, task_id, source_event_cursor,
                 projection_version, briefing_json, content_hash, state_version, generated_by,
                 created_at)
             VALUES ('h-1', 'default', ?1, 0, 0, '{}', 'h', 'v1', 'op', '2026-01-01T00:00:00+00:00')",
            rusqlite::params![task_id],
        )
        .expect("seed a handoff snapshot");

        delete_task(state.clone(), &task_id)
            .await
            .expect("a task with a handoff must delete");

        assert!(
            list_tasks(&state).await.expect("list").is_empty(),
            "the task survived a delete that reported success"
        );
        let snapshots: i64 = conn
            .query_row("SELECT COUNT(*) FROM handoff_snapshots", [], |row| {
                row.get(0)
            })
            .expect("count snapshots");
        assert_eq!(
            snapshots, 0,
            "the handoff snapshot outlived the task that owned it"
        );
    }

    /// FR-168: a reference nobody has ruled on refuses, names itself, and does
    /// not stop the task's runtime on the way out.
    ///
    /// The ordering is the half the repository suite cannot see. `delete_task`
    /// used to stop the runtime first and discover the refusal afterwards,
    /// leaving the operator with a task that was neither running nor deleted —
    /// a third outcome nobody asked for.
    ///
    /// The ordering is asserted by **observing the runtime registration**, not
    /// by reading the wording of the error. `stop_task_runtime_for_delete`
    /// removes the task from `state.running` and sets its stop flag, so a
    /// refusal that ran after it leaves an empty map and a raised flag. An
    /// earlier version of this test asserted that the message contained
    /// "left running", which is a string literal in the error and would have
    /// gone on passing with the two steps in either order — the proxy failure
    /// this FR spent its budget on, reappearing in the test written to close it.
    #[tokio::test]
    async fn an_unruled_reference_refuses_before_the_runtime_is_stopped() {
        use std::sync::atomic::Ordering;

        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE later_addition (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL,
                 FOREIGN KEY(task_id) REFERENCES tasks(id)
             );",
        )
        .expect("add a table nobody has ruled on");
        conn.execute(
            "INSERT INTO later_addition (id, task_id) VALUES ('row-1', ?1)",
            rusqlite::params![task_id],
        )
        .expect("pin the task");

        // Register the task as running, so that stopping it is observable.
        let runtime = agent_orchestrator::state::RunningTask::new();
        let stop_flag = runtime.stop_flag.clone();
        state.running.lock().await.insert(task_id.clone(), runtime);

        let error = delete_task(state.clone(), &task_id)
            .await
            .expect_err("an unruled reference must refuse the delete");

        // The diagnostic names the holder. An exit status could not tell this
        // apart from a missing task or a disk error.
        let rendered = error.to_string();
        assert!(
            rendered.contains("later_addition.task_id"),
            "the refusal did not name the reference holding the task: {rendered}"
        );
        assert!(
            !rendered.contains("FOREIGN KEY constraint failed"),
            "the operator still gets the raw driver error: {rendered}"
        );

        // The runtime was left alone: still registered, flag never raised.
        assert!(
            state.running.lock().await.contains_key(&task_id),
            "the refused delete deregistered the task's runtime, leaving it \
             neither running nor deleted"
        );
        assert!(
            !stop_flag.load(Ordering::SeqCst),
            "the refused delete raised the task's stop flag on its way out"
        );

        assert_eq!(
            list_tasks(&state).await.expect("list").len(),
            1,
            "a refused delete removed the task anyway"
        );
    }

    #[tokio::test]
    async fn get_task_trace_filters_non_verbose_events() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = get_item_id(&state, &task_id);

        insert_event(
            &state,
            &task_id,
            Some(&item_id),
            "step_started",
            serde_json::json!({"step":"qa"}),
        )
        .await
        .expect("insert visible event");
        insert_event(
            &state,
            &task_id,
            Some(&item_id),
            "heartbeat",
            serde_json::json!({"step":"qa"}),
        )
        .await
        .expect("insert hidden event");

        let terse = get_task_trace(&state, &task_id, false)
            .await
            .expect("get terse trace");
        assert_eq!(terse.entries.len(), 1);
        assert_eq!(terse.entries[0].event_type, "step_started");
        assert!(terse.full_trace.is_some());

        let verbose = get_task_trace(&state, &task_id, true)
            .await
            .expect("get verbose trace");
        assert_eq!(verbose.entries.len(), 2);
    }
}
