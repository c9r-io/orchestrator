use std::sync::Arc;
use std::time::Instant;

use agent_orchestrator::state::InnerState;
use orchestrator_proto::*;
use tokio::sync::Notify;
use tonic::{Request, Response, Status};

/// gRPC service implementation — thin translation layer from gRPC requests
/// to core service calls.
pub struct OrchestratorServer {
    state: Arc<InnerState>,
    startup_instant: Instant,
    shutdown_notify: Arc<Notify>,
}

impl OrchestratorServer {
    pub fn new(
        state: Arc<InnerState>,
        startup_instant: Instant,
        shutdown_notify: Arc<Notify>,
    ) -> Self {
        Self {
            state,
            startup_instant,
            shutdown_notify,
        }
    }
}

fn map_resource_error(error: anyhow::Error) -> Status {
    let message = error.to_string();
    if message.starts_with("[FAILED_PRECONDITION]") {
        return Status::failed_precondition(
            message.trim_start_matches("[FAILED_PRECONDITION] ").to_string(),
        );
    }
    if message.starts_with("use --force") {
        return Status::failed_precondition(message);
    }
    Status::internal(message)
}

#[tonic::async_trait]
impl OrchestratorService for OrchestratorServer {
    // ─── Task lifecycle ───────────────────────────────────────

    async fn task_create(
        &self,
        request: Request<TaskCreateRequest>,
    ) -> Result<Response<TaskCreateResponse>, Status> {
        let req = request.into_inner();
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

        let created = agent_orchestrator::service::task::create_task(&self.state, payload)
            .map_err(|e| Status::internal(format!("task create failed: {e}")))?;

        let mut status = "created".to_string();
        let mut message = format!("Task created: {}", created.id);

        if !req.no_start {
            if req.detach {
                agent_orchestrator::service::task::enqueue_task(&self.state, &created.id)
                    .await
                    .map_err(|e| Status::internal(format!("enqueue failed: {e}")))?;
                status = "enqueued".to_string();
                message = format!("Task enqueued: {}", created.id);
            } else {
                agent_orchestrator::service::task::start_task_blocking(
                    self.state.clone(),
                    &created.id,
                )
                .await
                .map_err(|e| Status::internal(format!("start failed: {e}")))?;
                let summary =
                    agent_orchestrator::service::task::load_summary(&self.state, &created.id)
                        .await
                        .map_err(|e| Status::internal(format!("load summary: {e}")))?;
                status = summary.status.clone();
                message = format!("Task finished: {} status={}", summary.id, summary.status);
            }
        }

        Ok(Response::new(TaskCreateResponse {
            task_id: created.id,
            status,
            message,
        }))
    }

    async fn task_start(
        &self,
        request: Request<TaskStartRequest>,
    ) -> Result<Response<TaskStartResponse>, Status> {
        let req = request.into_inner();
        let id = agent_orchestrator::service::task::resolve_start_id(
            &self.state,
            req.task_id.as_deref(),
            req.latest,
        )
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

        if req.detach {
            agent_orchestrator::service::task::enqueue_task(&self.state, &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskStartResponse {
                task_id: id.clone(),
                status: "enqueued".into(),
                message: format!("Task enqueued: {id}"),
            }))
        } else {
            agent_orchestrator::service::task::start_task_blocking(self.state.clone(), &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            let summary = agent_orchestrator::service::task::load_summary(&self.state, &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskStartResponse {
                task_id: id.clone(),
                status: summary.status.clone(),
                message: format!("Task finished: {} status={}", summary.id, summary.status),
            }))
        }
    }

    async fn task_pause(
        &self,
        request: Request<TaskPauseRequest>,
    ) -> Result<Response<TaskPauseResponse>, Status> {
        let req = request.into_inner();
        let id = agent_orchestrator::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;
        agent_orchestrator::service::task::pause_task(self.state.clone(), &id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;
        Ok(Response::new(TaskPauseResponse {
            task_id: id.clone(),
            message: format!("Task paused: {id}"),
        }))
    }

    async fn task_resume(
        &self,
        request: Request<TaskResumeRequest>,
    ) -> Result<Response<TaskResumeResponse>, Status> {
        let req = request.into_inner();
        let id = agent_orchestrator::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;

        if req.detach {
            agent_orchestrator::service::task::enqueue_task(&self.state, &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskResumeResponse {
                task_id: id.clone(),
                status: "enqueued".into(),
                message: format!("Task enqueued: {id}"),
            }))
        } else {
            agent_orchestrator::service::task::start_task_blocking(self.state.clone(), &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            let summary = agent_orchestrator::service::task::load_summary(&self.state, &id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskResumeResponse {
                task_id: id.clone(),
                status: summary.status.clone(),
                message: format!("Task finished: {} status={}", summary.id, summary.status),
            }))
        }
    }

    async fn task_delete(
        &self,
        request: Request<TaskDeleteRequest>,
    ) -> Result<Response<TaskDeleteResponse>, Status> {
        let req = request.into_inner();
        if !req.force {
            return Err(Status::failed_precondition(
                "use --force to confirm task deletion",
            ));
        }
        let id = agent_orchestrator::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;
        agent_orchestrator::service::task::delete_task(self.state.clone(), &id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;
        Ok(Response::new(TaskDeleteResponse {
            message: format!("Task deleted: {id}"),
        }))
    }

    async fn task_retry(
        &self,
        request: Request<TaskRetryRequest>,
    ) -> Result<Response<TaskRetryResponse>, Status> {
        let req = request.into_inner();
        if !req.force {
            return Err(Status::failed_precondition(
                "use --force to confirm task retry",
            ));
        }
        let task_id =
            agent_orchestrator::service::task::retry_task_item(&self.state, &req.task_item_id)
                .map_err(|e| Status::internal(format!("{e}")))?;

        if req.detach {
            agent_orchestrator::service::task::enqueue_task(&self.state, &task_id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskRetryResponse {
                task_id: task_id.clone(),
                status: "enqueued".into(),
                message: format!("Task enqueued: {task_id}"),
            }))
        } else {
            agent_orchestrator::service::task::start_task_blocking(self.state.clone(), &task_id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            let summary = agent_orchestrator::service::task::load_summary(&self.state, &task_id)
                .await
                .map_err(|e| Status::internal(format!("{e}")))?;
            Ok(Response::new(TaskRetryResponse {
                task_id: task_id.clone(),
                status: summary.status.clone(),
                message: format!("Retry finished: {} status={}", summary.id, summary.status),
            }))
        }
    }

    // ─── Task queries ─────────────────────────────────────────

    async fn task_list(
        &self,
        request: Request<TaskListRequest>,
    ) -> Result<Response<TaskListResponse>, Status> {
        let req = request.into_inner();
        let tasks = agent_orchestrator::service::task::list_tasks(&self.state)
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

    async fn task_info(
        &self,
        request: Request<TaskInfoRequest>,
    ) -> Result<Response<TaskInfoResponse>, Status> {
        let req = request.into_inner();
        let detail = agent_orchestrator::service::task::get_task_detail(&self.state, &req.task_id)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(TaskInfoResponse {
            task: Some(summary_to_proto(&detail.task)),
            items: detail.items.into_iter().map(item_to_proto).collect(),
            runs: detail.runs.into_iter().map(run_to_proto).collect(),
            events: detail.events.into_iter().map(event_to_proto).collect(),
        }))
    }

    // ─── Task streaming ───────────────────────────────────────

    type TaskLogsStream = tokio_stream::wrappers::ReceiverStream<Result<TaskLogChunk, Status>>;

    async fn task_logs(
        &self,
        request: Request<TaskLogsRequest>,
    ) -> Result<Response<Self::TaskLogsStream>, Status> {
        let req = request.into_inner();
        let logs = agent_orchestrator::service::task::get_task_logs(
            &self.state,
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

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    type TaskFollowStream = tokio_stream::wrappers::ReceiverStream<Result<TaskLogLine, Status>>;

    async fn task_follow(
        &self,
        request: Request<TaskFollowRequest>,
    ) -> Result<Response<Self::TaskFollowStream>, Status> {
        let req = request.into_inner();
        let state = self.state.clone();
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

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    type TaskWatchStream =
        tokio_stream::wrappers::ReceiverStream<Result<TaskWatchSnapshot, Status>>;

    async fn task_watch(
        &self,
        request: Request<TaskWatchRequest>,
    ) -> Result<Response<Self::TaskWatchStream>, Status> {
        let req = request.into_inner();
        let state = self.state.clone();
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
                    match agent_orchestrator::service::task::load_summary(&state, &req.task_id)
                        .await
                    {
                        Ok(s) => s,
                        Err(_) => break,
                    };

                let detail =
                    match agent_orchestrator::service::task::get_task_detail(&state, &req.task_id)
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
                    break; // client disconnected
                }

                // Stop streaming on terminal status
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

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    // ─── Resource management ──────────────────────────────────

    async fn apply(
        &self,
        request: Request<ApplyRequest>,
    ) -> Result<Response<ApplyResponse>, Status> {
        let req = request.into_inner();
        let result = agent_orchestrator::service::resource::apply_manifests(
            &self.state,
            &req.content,
            req.dry_run,
            req.project.as_deref(),
            req.prune,
        )
        .map_err(map_resource_error)?;

        Ok(Response::new(result))
    }

    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let req = request.into_inner();
        let content = agent_orchestrator::service::resource::get_resource(
            &self.state,
            &req.resource,
            req.selector.as_deref(),
            &req.output_format,
            req.project.as_deref(),
        )
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(GetResponse {
            content,
            format: req.output_format,
        }))
    }

    async fn describe(
        &self,
        request: Request<DescribeRequest>,
    ) -> Result<Response<DescribeResponse>, Status> {
        let req = request.into_inner();
        let content = agent_orchestrator::service::resource::describe_resource(
            &self.state,
            &req.resource,
            &req.output_format,
            req.project.as_deref(),
        )
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(DescribeResponse {
            content,
            format: req.output_format,
        }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::resource::delete_resource(
            &self.state,
            &req.resource,
            req.force,
            req.project.as_deref(),
            req.dry_run,
        )
        .map_err(map_resource_error)?;
        let scope = req
            .project
            .map(|p| format!(" (project: {})", p))
            .unwrap_or_default();
        let verb = if req.dry_run { "would be deleted (dry run)" } else { "deleted" };
        Ok(Response::new(DeleteResponse {
            message: format!("{} {}{}", req.resource, verb, scope),
        }))
    }

    // ─── Store ────────────────────────────────────────────────

    async fn store_get(
        &self,
        request: Request<StoreGetRequest>,
    ) -> Result<Response<StoreGetResponse>, Status> {
        let req = request.into_inner();
        let result = agent_orchestrator::service::store::store_get(
            &self.state,
            &req.store,
            &req.key,
            &req.project,
        )
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(StoreGetResponse {
            value_json: result.clone(),
            found: result.is_some(),
        }))
    }

    async fn store_put(
        &self,
        request: Request<StorePutRequest>,
    ) -> Result<Response<StorePutResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::store::store_put(
            &self.state,
            &req.store,
            &req.key,
            &req.value_json,
            &req.project,
            &req.task_id,
        )
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(StorePutResponse {
            message: format!("stored key '{}' in '{}'", req.key, req.store),
        }))
    }

    async fn store_delete(
        &self,
        request: Request<StoreDeleteRequest>,
    ) -> Result<Response<StoreDeleteResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::store::store_delete(
            &self.state,
            &req.store,
            &req.key,
            &req.project,
        )
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(StoreDeleteResponse {
            message: format!("deleted key '{}' from '{}'", req.key, req.store),
        }))
    }

    async fn store_list(
        &self,
        request: Request<StoreListRequest>,
    ) -> Result<Response<StoreListResponse>, Status> {
        let req = request.into_inner();
        let entries = agent_orchestrator::service::store::store_list(
            &self.state,
            &req.store,
            &req.project,
            req.limit,
            req.offset,
        )
        .await
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(StoreListResponse { entries }))
    }

    async fn store_prune(
        &self,
        request: Request<StorePruneRequest>,
    ) -> Result<Response<StorePruneResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::store::store_prune(&self.state, &req.store, &req.project)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(StorePruneResponse {
            message: format!("pruned store '{}'", req.store),
        }))
    }

    // ─── System ───────────────────────────────────────────────

    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            git_hash: env!("BUILD_GIT_HASH").to_string(),
            uptime_secs: self.startup_instant.elapsed().as_secs().to_string(),
        }))
    }

    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(graceful = req.graceful, "shutdown requested via RPC");
        self.shutdown_notify.notify_one();
        Ok(Response::new(ShutdownResponse {
            message: "shutdown initiated".to_string(),
        }))
    }

    async fn config_debug(
        &self,
        request: Request<ConfigDebugRequest>,
    ) -> Result<Response<ConfigDebugResponse>, Status> {
        let req = request.into_inner();
        let content =
            agent_orchestrator::service::system::debug_info(&self.state, req.component.as_deref())
                .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(ConfigDebugResponse {
            content,
            format: "text".to_string(),
        }))
    }

    async fn worker_status(
        &self,
        _request: Request<WorkerStatusRequest>,
    ) -> Result<Response<WorkerStatusResponse>, Status> {
        let status = agent_orchestrator::service::system::worker_status(&self.state)
            .await
            .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(status))
    }

    async fn check(
        &self,
        request: Request<CheckRequest>,
    ) -> Result<Response<CheckResponse>, Status> {
        let req = request.into_inner();
        let (content, exit_code) = agent_orchestrator::service::system::run_check(
            &self.state,
            req.workflow.as_deref(),
            &req.output_format,
            req.project_id.as_deref(),
        )
        .map_err(|e| Status::internal(format!("{e}")))?;

        Ok(Response::new(CheckResponse {
            content,
            format: req.output_format,
            exit_code,
        }))
    }

    async fn init(&self, request: Request<InitRequest>) -> Result<Response<InitResponse>, Status> {
        let req = request.into_inner();
        let message =
            agent_orchestrator::service::system::run_init(&self.state, req.root.as_deref())
                .map_err(|e| Status::internal(format!("{e}")))?;
        Ok(Response::new(InitResponse { message }))
    }

    async fn manifest_validate(
        &self,
        request: Request<ManifestValidateRequest>,
    ) -> Result<Response<ManifestValidateResponse>, Status> {
        let req = request.into_inner();
        let (valid, errors, message) =
            agent_orchestrator::service::system::validate_manifests(
                &self.state,
                &req.content,
                req.project_id.as_deref(),
            )
            .map_err(|e| Status::internal(format!("{e}")))?;
        Ok(Response::new(ManifestValidateResponse {
            valid,
            errors,
            message,
        }))
    }

    async fn manifest_export(
        &self,
        request: Request<ManifestExportRequest>,
    ) -> Result<Response<ManifestExportResponse>, Status> {
        let req = request.into_inner();
        let content =
            agent_orchestrator::service::resource::export_manifests(&self.state, &req.output_format)
                .map_err(|e| Status::internal(format!("{e}")))?;
        Ok(Response::new(ManifestExportResponse {
            content,
            format: req.output_format,
        }))
    }

    async fn task_trace(
        &self,
        request: Request<TaskTraceRequest>,
    ) -> Result<Response<TaskTraceResponse>, Status> {
        let req = request.into_inner();
        let result = agent_orchestrator::service::task::get_task_trace(
            &self.state,
            &req.task_id,
            req.verbose,
        )
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

        // Serialize full trace to JSON for --json output
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

}

// ─── DTO → Proto conversions ─────────────────────────────────

fn summary_to_proto(t: &agent_orchestrator::dto::TaskSummary) -> TaskSummary {
    TaskSummary {
        id: t.id.clone(),
        name: t.name.clone(),
        status: t.status.clone(),
        started_at: t.started_at.clone(),
        completed_at: t.completed_at.clone(),
        goal: t.goal.clone(),
        project_id: t.project_id.clone(),
        workspace_id: t.workspace_id.clone(),
        workflow_id: t.workflow_id.clone(),
        target_files: t.target_files.clone(),
        total_items: t.total_items,
        finished_items: t.finished_items,
        failed_items: t.failed_items,
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        parent_task_id: t.parent_task_id.clone(),
        spawn_reason: t.spawn_reason.clone(),
        spawn_depth: t.spawn_depth,
    }
}

fn item_to_proto(i: agent_orchestrator::dto::TaskItemDto) -> TaskItem {
    TaskItem {
        id: i.id,
        task_id: i.task_id,
        order_no: i.order_no,
        qa_file_path: i.qa_file_path,
        status: i.status,
        ticket_files: i.ticket_files,
        ticket_content_json: serde_json::to_string(&i.ticket_content).unwrap_or_default(),
        fix_required: i.fix_required,
        fixed: i.fixed,
        last_error: i.last_error,
        started_at: i.started_at,
        completed_at: i.completed_at,
        updated_at: i.updated_at,
    }
}

fn run_to_proto(r: agent_orchestrator::dto::CommandRunDto) -> CommandRun {
    CommandRun {
        id: r.id,
        task_item_id: r.task_item_id,
        phase: r.phase,
        command: r.command,
        cwd: r.cwd,
        workspace_id: r.workspace_id,
        agent_id: r.agent_id,
        exit_code: r.exit_code,
        stdout_path: r.stdout_path,
        stderr_path: r.stderr_path,
        started_at: r.started_at,
        ended_at: r.ended_at,
        interrupted: r.interrupted,
    }
}

fn event_to_proto(e: agent_orchestrator::dto::EventDto) -> Event {
    Event {
        id: e.id,
        task_id: e.task_id,
        task_item_id: e.task_item_id,
        event_type: e.event_type,
        payload_json: serde_json::to_string(&e.payload).unwrap_or_default(),
        created_at: e.created_at,
    }
}
