use std::pin::Pin;

use futures::Stream;
use orchestrator_proto::*;
use tonic::{Request, Response, Status};

use super::mapping::{
    event_to_proto, graph_debug_to_proto, item_to_proto, run_to_proto, summary_to_proto,
};
use super::OrchestratorServer;
use crate::protection::ProtectionLease;

pub(crate) type TaskLogsStream = Pin<Box<dyn Stream<Item = Result<TaskLogChunk, Status>> + Send>>;
pub(crate) type TaskFollowStream = Pin<Box<dyn Stream<Item = Result<TaskLogLine, Status>> + Send>>;
pub(crate) type TaskWatchStream =
    Pin<Box<dyn Stream<Item = Result<TaskWatchSnapshot, Status>> + Send>>;

struct GuardedStream<S> {
    inner: S,
    _lease: ProtectionLease,
}

impl<S, T> Stream for GuardedStream<S>
where
    S: Stream<Item = Result<T, Status>> + Unpin,
{
    type Item = Result<T, Status>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

fn boxed_stream<S, T>(stream: S) -> Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>
where
    S: Stream<Item = Result<T, Status>> + Send + 'static,
{
    Box::pin(stream)
}

fn guarded_stream<S, T>(
    stream: S,
    lease: ProtectionLease,
) -> Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>
where
    S: Stream<Item = Result<T, Status>> + Send + Unpin + 'static,
{
    Box::pin(GuardedStream {
        inner: stream,
        _lease: lease,
    })
}

pub(crate) async fn task_create(
    server: &OrchestratorServer,
    request: Request<TaskCreateRequest>,
) -> Result<Response<TaskCreateResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskCreate")?;
    super::authorize(server, &request, "TaskCreate").map_err(Status::from)?;
    let req = request.into_inner();
    if !req.no_start {
        if let Some(status) = server.reject_new_work_during_shutdown("TaskCreate") {
            return Err(status);
        }
    }
    let payload = agent_orchestrator::dto::CreateTaskPayload {
        name: req.name,
        goal: req.goal,
        project_id: req.project_id,
        workspace_id: req.workspace_id,
        workflow_id: req.workflow_id,
        target_files: if req.target_files.is_empty() {
            None
        } else {
            Some(req.target_files)
        },
        parent_task_id: None,
        spawn_reason: None,
    };

    let created = agent_orchestrator::service::task::create_task(&server.state, payload)
        .map_err(|e| Status::internal(format!("task create failed: {e}")))?;

    let mut status = "created".to_string();
    let mut message = format!("Task created: {}", created.id);

    if !req.no_start {
        agent_orchestrator::service::task::enqueue_task(&server.state, &created.id)
            .await
            .map_err(|e| Status::internal(format!("enqueue failed: {e}")))?;
        status = "enqueued".to_string();
        message = format!("Task enqueued: {}", created.id);
    }

    Ok(Response::new(TaskCreateResponse {
        task_id: created.id,
        status,
        message,
    }))
}

pub(crate) async fn task_start(
    server: &OrchestratorServer,
    request: Request<TaskStartRequest>,
) -> Result<Response<TaskStartResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskStart")?;
    super::authorize(server, &request, "TaskStart").map_err(Status::from)?;
    if let Some(status) = server.reject_new_work_during_shutdown("TaskStart") {
        return Err(status);
    }
    let req = request.into_inner();
    let id = agent_orchestrator::service::task::resolve_start_id(
        &server.state,
        req.task_id.as_deref(),
        req.latest,
    )
    .await
    .map_err(|e| Status::internal(format!("{e}")))?;

    agent_orchestrator::service::task::enqueue_task(&server.state, &id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(TaskStartResponse {
        task_id: id.clone(),
        status: "enqueued".into(),
        message: format!("Task enqueued: {id}"),
    }))
}

pub(crate) async fn task_pause(
    server: &OrchestratorServer,
    request: Request<TaskPauseRequest>,
) -> Result<Response<TaskPauseResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskPause")?;
    super::authorize(server, &request, "TaskPause").map_err(Status::from)?;
    let req = request.into_inner();
    let id = agent_orchestrator::service::task::resolve_id(&server.state, &req.task_id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    agent_orchestrator::service::task::pause_task(server.state.clone(), &id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(TaskPauseResponse {
        task_id: id.clone(),
        message: format!("Task paused: {id}"),
    }))
}

pub(crate) async fn task_resume(
    server: &OrchestratorServer,
    request: Request<TaskResumeRequest>,
) -> Result<Response<TaskResumeResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskResume")?;
    super::authorize(server, &request, "TaskResume").map_err(Status::from)?;
    if let Some(status) = server.reject_new_work_during_shutdown("TaskResume") {
        return Err(status);
    }
    let req = request.into_inner();
    let id = agent_orchestrator::service::task::resolve_id(&server.state, &req.task_id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

    agent_orchestrator::service::task::enqueue_task(&server.state, &id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(TaskResumeResponse {
        task_id: id.clone(),
        status: "enqueued".into(),
        message: format!("Task enqueued: {id}"),
    }))
}

pub(crate) async fn task_delete(
    server: &OrchestratorServer,
    request: Request<TaskDeleteRequest>,
) -> Result<Response<TaskDeleteResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskDelete")?;
    super::authorize(server, &request, "TaskDelete").map_err(Status::from)?;
    let req = request.into_inner();
    if !req.force {
        return Err(Status::failed_precondition(
            "use --force to confirm task deletion",
        ));
    }
    let id = agent_orchestrator::service::task::resolve_id(&server.state, &req.task_id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    agent_orchestrator::service::task::delete_task(server.state.clone(), &id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(TaskDeleteResponse {
        message: format!("Task deleted: {id}"),
    }))
}

pub(crate) async fn task_retry(
    server: &OrchestratorServer,
    request: Request<TaskRetryRequest>,
) -> Result<Response<TaskRetryResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskRetry")?;
    super::authorize(server, &request, "TaskRetry").map_err(Status::from)?;
    if let Some(status) = server.reject_new_work_during_shutdown("TaskRetry") {
        return Err(status);
    }
    let req = request.into_inner();
    if !req.force {
        return Err(Status::failed_precondition(
            "use --force to confirm task retry",
        ));
    }
    let task_id =
        agent_orchestrator::service::task::retry_task_item(&server.state, &req.task_item_id)
            .map_err(|e| Status::internal(format!("{e}")))?;

    agent_orchestrator::service::task::enqueue_task(&server.state, &task_id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;
    Ok(Response::new(TaskRetryResponse {
        task_id: task_id.clone(),
        status: "enqueued".into(),
        message: format!("Task enqueued: {task_id}"),
    }))
}

pub(crate) async fn task_list(
    server: &OrchestratorServer,
    request: Request<TaskListRequest>,
) -> Result<Response<TaskListResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskList")?;
    super::authorize(server, &request, "TaskList").map_err(Status::from)?;
    let req = request.into_inner();
    let tasks = agent_orchestrator::service::task::list_tasks(&server.state)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

    let filtered: Vec<_> = tasks
        .into_iter()
        .filter(|t| match &req.status_filter {
            Some(s) if !s.is_empty() => t.status == *s,
            _ => true,
        })
        .filter(|t| match &req.project_filter {
            Some(p) if !p.is_empty() => t.project_id == *p,
            _ => true,
        })
        .collect();

    let protos = filtered.into_iter().map(|t| summary_to_proto(&t)).collect();
    Ok(Response::new(TaskListResponse { tasks: protos }))
}

pub(crate) async fn task_info(
    server: &OrchestratorServer,
    request: Request<TaskInfoRequest>,
) -> Result<Response<TaskInfoResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskInfo")?;
    super::authorize(server, &request, "TaskInfo").map_err(Status::from)?;
    let req = request.into_inner();
    let detail = agent_orchestrator::service::task::get_task_detail(&server.state, &req.task_id)
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

    Ok(Response::new(TaskInfoResponse {
        task: Some(summary_to_proto(&detail.task)),
        items: detail.items.into_iter().map(item_to_proto).collect(),
        runs: detail.runs.into_iter().map(run_to_proto).collect(),
        events: detail.events.into_iter().map(event_to_proto).collect(),
        graph_debug: detail
            .graph_debug
            .into_iter()
            .map(graph_debug_to_proto)
            .collect(),
    }))
}

pub(crate) async fn task_logs(
    server: &OrchestratorServer,
    request: Request<TaskLogsRequest>,
) -> Result<Response<TaskLogsStream>, Status> {
    let _guard = server.protect_unary(&request, "TaskLogs")?;
    super::authorize(server, &request, "TaskLogs").map_err(Status::from)?;
    let req = request.into_inner();
    let logs = agent_orchestrator::service::task::get_task_logs(
        &server.state,
        &req.task_id,
        req.tail as usize,
        req.timestamps,
    )
    .await
    .map_err(|e| Status::internal(format!("{e}")))?;

    let (tx, rx) = tokio::sync::mpsc::channel(32);
    for chunk in logs {
        let proto = TaskLogChunk {
            run_id: chunk.run_id,
            phase: chunk.phase,
            content: chunk.content,
            stdout_path: chunk.stdout_path,
            stderr_path: chunk.stderr_path,
            started_at: chunk.started_at,
        };
        let _ = tx.send(Ok(proto)).await;
    }

    Ok(Response::new(boxed_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    )))
}

pub(crate) async fn task_follow(
    server: &OrchestratorServer,
    request: Request<TaskFollowRequest>,
) -> Result<Response<TaskFollowStream>, Status> {
    let lease = server.protect_stream(&request, "TaskFollow")?;
    super::authorize(server, &request, "TaskFollow").map_err(Status::from)?;
    let req = request.into_inner();
    let state = server.state.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    tokio::spawn(async move {
        let line_tx = tx.clone();
        let send_fn = move |line: String| {
            let tx = line_tx.clone();
            async move {
                let _ = tx
                    .send(Ok(TaskLogLine {
                        line,
                        timestamp: String::new(),
                    }))
                    .await;
            }
        };
        let _ = agent_orchestrator::service::task::follow_task_logs_stream(
            &state,
            &req.task_id,
            send_fn,
        )
        .await;
    });

    Ok(Response::new(guarded_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
        lease,
    )))
}

pub(crate) async fn task_watch(
    server: &OrchestratorServer,
    request: Request<TaskWatchRequest>,
) -> Result<Response<TaskWatchStream>, Status> {
    let lease = server.protect_stream(&request, "TaskWatch")?;
    super::authorize(server, &request, "TaskWatch").map_err(Status::from)?;
    let req = request.into_inner();
    let state = server.state.clone();
    let interval_secs = if req.interval_secs == 0 {
        2
    } else {
        req.interval_secs
    };
    let (tx, rx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(interval_secs);
        loop {
            let summary =
                match agent_orchestrator::service::task::load_summary(&state, &req.task_id).await {
                    Ok(s) => s,
                    Err(_) => break,
                };

            let detail = match agent_orchestrator::service::task::get_task_detail(
                &state,
                &req.task_id,
            )
            .await
            {
                Ok(d) => d,
                Err(_) => break,
            };

            let snapshot = TaskWatchSnapshot {
                task: Some(summary_to_proto(&summary)),
                items: detail.items.into_iter().map(item_to_proto).collect(),
            };

            if tx.send(Ok(snapshot)).await.is_err() {
                break;
            }

            let terminal = matches!(
                summary.status.as_str(),
                "completed" | "failed" | "cancelled" | "deleted"
            );
            if terminal {
                break;
            }

            tokio::time::sleep(interval).await;
        }
    });

    Ok(Response::new(guarded_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
        lease,
    )))
}

pub(crate) async fn task_trace(
    server: &OrchestratorServer,
    request: Request<TaskTraceRequest>,
) -> Result<Response<TaskTraceResponse>, Status> {
    let _guard = server.protect_unary(&request, "TaskTrace")?;
    super::authorize(server, &request, "TaskTrace").map_err(Status::from)?;
    let req = request.into_inner();
    let result =
        agent_orchestrator::service::task::get_task_trace(&server.state, &req.task_id, req.verbose)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;

    let entries = result
        .entries
        .into_iter()
        .map(|e| TraceEntry {
            timestamp: e.timestamp,
            event_type: e.event_type,
            step: e.step,
            item_id: e.item_id,
            payload_json: e.payload_json,
        })
        .collect();

    let anomalies = result
        .anomalies
        .into_iter()
        .map(|a| Anomaly {
            rule: a.rule,
            severity: format!("{:?}", a.severity).to_lowercase(),
            message: a.message,
            at: a.at,
            escalation: format!("{:?}", a.escalation).to_lowercase(),
        })
        .collect();

    let trace_json = result
        .full_trace
        .as_ref()
        .and_then(|t| serde_json::to_string(t).ok())
        .unwrap_or_else(|| "{}".to_string());

    Ok(Response::new(TaskTraceResponse {
        entries,
        anomalies,
        trace_json,
    }))
}
