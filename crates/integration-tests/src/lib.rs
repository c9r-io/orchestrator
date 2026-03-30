//! Integration test harness for the orchestrator.
//!
//! Provides [`TestHarness`] which spins up an in-process gRPC server backed by
//! a real [`InnerState`] and returns a connected [`OrchestratorServiceClient`].

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use agent_orchestrator::dto::{
    CommandRunDto, EventDto, TaskGraphDebugBundle, TaskItemDto, TaskSummary,
};
use agent_orchestrator::error::{ErrorCategory, OrchestratorError};
use agent_orchestrator::state::InnerState;
use agent_orchestrator::test_utils::TestState;
use futures::Stream;
use orchestrator_proto::*;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

// ---------------------------------------------------------------------------
// Proto mapping helpers (mirrors crates/daemon/src/server/mapping.rs)
// ---------------------------------------------------------------------------

fn summary_to_proto(t: TaskSummary) -> orchestrator_proto::TaskSummary {
    orchestrator_proto::TaskSummary {
        id: t.id,
        name: t.name,
        status: t.status,
        started_at: t.started_at,
        completed_at: t.completed_at,
        goal: t.goal,
        project_id: t.project_id,
        workspace_id: t.workspace_id,
        workflow_id: t.workflow_id,
        target_files: t.target_files,
        total_items: t.total_items,
        finished_items: t.finished_items,
        failed_items: t.failed_items,
        created_at: t.created_at,
        updated_at: t.updated_at,
        parent_task_id: t.parent_task_id,
        spawn_reason: t.spawn_reason,
        spawn_depth: t.spawn_depth,
    }
}

fn item_to_proto(i: TaskItemDto) -> TaskItem {
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

fn run_to_proto(r: CommandRunDto) -> CommandRun {
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

fn event_to_proto(e: EventDto) -> Event {
    Event {
        id: e.id,
        task_id: e.task_id,
        task_item_id: e.task_item_id,
        event_type: e.event_type,
        payload_json: serde_json::to_string(&e.payload).unwrap_or_default(),
        created_at: e.created_at,
    }
}

fn graph_debug_to_proto(bundle: TaskGraphDebugBundle) -> orchestrator_proto::TaskGraphDebugBundle {
    orchestrator_proto::TaskGraphDebugBundle {
        graph_run_id: bundle.graph_run_id,
        cycle: bundle.cycle,
        source: bundle.source,
        status: bundle.status,
        fallback_mode: bundle.fallback_mode,
        planner_failure_class: bundle.planner_failure_class,
        planner_failure_message: bundle.planner_failure_message,
        effective_graph_json: bundle.effective_graph_json,
        planner_raw_output_json: bundle.planner_raw_output_json,
        normalized_plan_json: bundle.normalized_plan_json,
        execution_replay_json: bundle.execution_replay_json,
        created_at: bundle.created_at,
        updated_at: bundle.updated_at,
    }
}

fn map_core_error(error: OrchestratorError) -> Status {
    let message = error.to_string();
    match error.category() {
        ErrorCategory::UserInput => Status::invalid_argument(message),
        ErrorCategory::ConfigValidation | ErrorCategory::InvalidState => {
            Status::failed_precondition(message)
        }
        ErrorCategory::NotFound => Status::not_found(message),
        ErrorCategory::SecurityDenied => Status::permission_denied(message),
        ErrorCategory::ExternalDependency => Status::unavailable(message),
        ErrorCategory::InternalInvariant => Status::internal(message),
    }
}

// ---------------------------------------------------------------------------
// Test gRPC server — thin delegation to core service functions
// ---------------------------------------------------------------------------

/// In-process gRPC server for integration tests. Mirrors the daemon's server
/// but skips authorization and shutdown rejection.
pub struct TestOrchestratorServer {
    state: Arc<InnerState>,
    shutdown_notify: Arc<Notify>,
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[tonic::async_trait]
impl OrchestratorService for TestOrchestratorServer {
    type TaskLogsStream = BoxStream<TaskLogChunk>;
    type TaskFollowStream = BoxStream<TaskLogLine>;
    type TaskWatchStream = BoxStream<TaskWatchSnapshot>;

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

        let created = orchestrator_scheduler::service::task::create_task(&self.state, payload)
            .map_err(map_core_error)?;

        let mut status = "created".to_string();
        let mut message = format!("Task created: {}", created.id);

        if !req.no_start {
            orchestrator_scheduler::service::task::enqueue_task(&self.state, &created.id)
                .await
                .map_err(map_core_error)?;
            status = "enqueued".to_string();
            message = format!("Task enqueued: {}", created.id);
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
        let id = orchestrator_scheduler::service::task::resolve_start_id(
            &self.state,
            req.task_id.as_deref(),
            req.latest,
        )
        .await
        .map_err(map_core_error)?;

        orchestrator_scheduler::service::task::enqueue_task(&self.state, &id)
            .await
            .map_err(map_core_error)?;
        Ok(Response::new(TaskStartResponse {
            task_id: id.clone(),
            status: "enqueued".into(),
            message: format!("Task enqueued: {id}"),
        }))
    }

    async fn task_pause(
        &self,
        request: Request<TaskPauseRequest>,
    ) -> Result<Response<TaskPauseResponse>, Status> {
        let req = request.into_inner();
        let id = orchestrator_scheduler::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(map_core_error)?;
        orchestrator_scheduler::service::task::pause_task(self.state.clone(), &id)
            .await
            .map_err(map_core_error)?;
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
        let id = orchestrator_scheduler::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(map_core_error)?;
        orchestrator_scheduler::service::task::enqueue_task(&self.state, &id)
            .await
            .map_err(map_core_error)?;
        Ok(Response::new(TaskResumeResponse {
            task_id: id.clone(),
            status: "enqueued".into(),
            message: format!("Task enqueued: {id}"),
        }))
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
        let id = orchestrator_scheduler::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(map_core_error)?;
        orchestrator_scheduler::service::task::delete_task(self.state.clone(), &id)
            .await
            .map_err(map_core_error)?;
        Ok(Response::new(TaskDeleteResponse {
            message: format!("Task deleted: {id}"),
        }))
    }

    async fn task_delete_bulk(
        &self,
        request: Request<TaskDeleteBulkRequest>,
    ) -> Result<Response<TaskDeleteBulkResponse>, Status> {
        let req = request.into_inner();
        if !req.force {
            return Err(Status::failed_precondition(
                "use --force to confirm bulk task deletion",
            ));
        }

        let ids: Vec<String> = if !req.task_ids.is_empty() {
            req.task_ids
        } else {
            let tasks = orchestrator_scheduler::service::task::list_tasks(&self.state)
                .await
                .map_err(map_core_error)?;
            tasks
                .into_iter()
                .filter(|t| {
                    if !req.status_filter.is_empty() && t.status != req.status_filter {
                        return false;
                    }
                    if !req.project_filter.is_empty() && t.project_id != req.project_filter {
                        return false;
                    }
                    true
                })
                .map(|t| t.id)
                .collect()
        };

        let mut deleted: i32 = 0;
        let mut failed: i32 = 0;
        let mut errors: Vec<String> = Vec::new();

        for id in &ids {
            match orchestrator_scheduler::service::task::delete_task(self.state.clone(), id).await {
                Ok(_) => deleted += 1,
                Err(e) => {
                    failed += 1;
                    errors.push(format!("{id}: {e}"));
                }
            }
        }

        Ok(Response::new(TaskDeleteBulkResponse {
            deleted,
            failed,
            errors,
            message: format!("Deleted {deleted} task(s) ({failed} error(s))"),
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
            orchestrator_scheduler::service::task::retry_task_item(&self.state, &req.task_item_id)
                .map_err(map_core_error)?;
        orchestrator_scheduler::service::task::enqueue_task(&self.state, &task_id)
            .await
            .map_err(map_core_error)?;
        Ok(Response::new(TaskRetryResponse {
            task_id: task_id.clone(),
            status: "enqueued".into(),
            message: format!("Task enqueued: {task_id}"),
        }))
    }

    async fn task_recover(
        &self,
        request: Request<TaskRecoverRequest>,
    ) -> Result<Response<TaskRecoverResponse>, Status> {
        let req = request.into_inner();
        let id = orchestrator_scheduler::service::task::resolve_id(&self.state, &req.task_id)
            .await
            .map_err(map_core_error)?;
        let recovered = orchestrator_scheduler::service::task::recover_task(&self.state, &id)
            .await
            .map_err(map_core_error)?;
        let count = recovered.len() as u64;
        let message = if count == 0 {
            format!("No orphaned running items found for task {id}")
        } else {
            format!("Recovered {count} orphaned running item(s) for task {id}")
        };
        Ok(Response::new(TaskRecoverResponse {
            task_id: id,
            recovered_items: count,
            message,
        }))
    }

    async fn task_list(
        &self,
        request: Request<TaskListRequest>,
    ) -> Result<Response<TaskListResponse>, Status> {
        let req = request.into_inner();
        let tasks = orchestrator_scheduler::service::task::list_tasks(&self.state)
            .await
            .map_err(map_core_error)?;
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
        let protos = filtered.into_iter().map(summary_to_proto).collect();
        Ok(Response::new(TaskListResponse { tasks: protos }))
    }

    async fn task_info(
        &self,
        request: Request<TaskInfoRequest>,
    ) -> Result<Response<TaskInfoResponse>, Status> {
        let req = request.into_inner();
        let detail =
            orchestrator_scheduler::service::task::get_task_detail(&self.state, &req.task_id)
                .await
                .map_err(map_core_error)?;

        let agent_states = {
            use agent_orchestrator::config_load::read_active_config;
            use agent_orchestrator::selection::resolve_effective_agents;
            let project_id = &detail.task.project_id;
            let pid = if project_id.is_empty() {
                ""
            } else {
                project_id.as_str()
            };
            let mut statuses = Vec::new();
            if let Ok(active) = read_active_config(&self.state) {
                let agents = resolve_effective_agents(pid, &active.config, None);
                let lifecycle_map = self.state.agent_lifecycle.read().await;
                let health_map = self.state.agent_health.read().await;
                for (id, cfg) in agents.iter() {
                    let runtime: agent_orchestrator::metrics::AgentRuntimeState =
                        lifecycle_map.get(id.as_str()).cloned().unwrap_or_default();
                    let (is_healthy, diseased_until, consecutive_errors) =
                        agent_orchestrator::health::agent_health_summary(&health_map, id);
                    statuses.push(AgentStatus {
                        name: id.clone(),
                        enabled: cfg.enabled,
                        lifecycle_state: runtime.lifecycle.as_str().to_string(),
                        in_flight_items: runtime.in_flight_items as i32,
                        capabilities: cfg.capabilities.clone(),
                        drain_requested_at: runtime.drain_requested_at.map(|dt| dt.to_rfc3339()),
                        is_healthy,
                        diseased_until,
                        consecutive_errors: consecutive_errors as i32,
                    });
                }
                statuses.sort_by(|a, b| a.name.cmp(&b.name));
            }
            statuses
        };

        Ok(Response::new(TaskInfoResponse {
            task: Some(summary_to_proto(detail.task)),
            items: detail.items.into_iter().map(item_to_proto).collect(),
            runs: detail.runs.into_iter().map(run_to_proto).collect(),
            events: detail.events.into_iter().map(event_to_proto).collect(),
            graph_debug: detail
                .graph_debug
                .into_iter()
                .map(graph_debug_to_proto)
                .collect(),
            agent_states,
        }))
    }

    async fn task_logs(
        &self,
        request: Request<TaskLogsRequest>,
    ) -> Result<Response<Self::TaskLogsStream>, Status> {
        let req = request.into_inner();
        let logs = orchestrator_scheduler::service::task::get_task_logs(
            &self.state,
            &req.task_id,
            req.tail as usize,
            req.timestamps,
        )
        .await
        .map_err(map_core_error)?;

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
        Ok(Response::new(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)) as BoxStream<TaskLogChunk>,
        ))
    }

    async fn task_follow(
        &self,
        request: Request<TaskFollowRequest>,
    ) -> Result<Response<Self::TaskFollowStream>, Status> {
        let req = request.into_inner();
        let state = self.state.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move {
            let _ = orchestrator_scheduler::service::task::follow_task_logs_stream(
                &state,
                &req.task_id,
                |line: String, _is_stderr: bool| {
                    let _ = tx.try_send(Ok(TaskLogLine {
                        line,
                        timestamp: String::new(),
                    }));
                    Ok(())
                },
            )
            .await;
        });
        Ok(Response::new(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)) as BoxStream<TaskLogLine>,
        ))
    }

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
                    match orchestrator_scheduler::service::task::load_summary(&state, &req.task_id)
                        .await
                    {
                        Ok(s) => s,
                        Err(_) => break,
                    };
                let detail = match orchestrator_scheduler::service::task::get_task_detail(
                    &state,
                    &req.task_id,
                )
                .await
                {
                    Ok(d) => d,
                    Err(_) => break,
                };
                let terminal = matches!(
                    summary.status.as_str(),
                    "completed" | "failed" | "cancelled" | "deleted"
                );
                let snapshot = TaskWatchSnapshot {
                    task: Some(summary_to_proto(summary)),
                    items: detail.items.into_iter().map(item_to_proto).collect(),
                };
                if tx.send(Ok(snapshot)).await.is_err() {
                    break;
                }
                if terminal {
                    break;
                }
                tokio::time::sleep(interval).await;
            }
        });
        Ok(Response::new(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
                as BoxStream<TaskWatchSnapshot>,
        ))
    }

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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
        let scope = req
            .project
            .map(|p| format!(" (project: {})", p))
            .unwrap_or_default();
        let verb = if req.dry_run {
            "would be deleted (dry run)"
        } else {
            "deleted"
        };
        Ok(Response::new(DeleteResponse {
            message: format!("{} {}{}", req.resource, verb, scope),
        }))
    }

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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
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
        .map_err(map_core_error)?;
        Ok(Response::new(StoreListResponse { entries }))
    }

    async fn store_prune(
        &self,
        request: Request<StorePruneRequest>,
    ) -> Result<Response<StorePruneResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::store::store_prune(&self.state, &req.store, &req.project)
            .await
            .map_err(map_core_error)?;
        Ok(Response::new(StorePruneResponse {
            message: format!("pruned store '{}'", req.store),
        }))
    }

    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        let runtime = agent_orchestrator::service::daemon::runtime_snapshot(&self.state);
        Ok(Response::new(PingResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            git_hash: String::new(),
            uptime_secs: runtime.uptime_secs.to_string(),
            shutdown_requested: runtime.shutdown_requested,
            lifecycle_state: runtime.lifecycle_state.as_str().to_string(),
            maintenance_mode: runtime.maintenance_mode,
            incarnation: runtime.incarnation,
        }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        self.state.daemon_runtime.request_shutdown();
        self.shutdown_notify.notify_one();
        Ok(Response::new(ShutdownResponse {
            message: "shutdown initiated".to_string(),
        }))
    }

    async fn maintenance_mode(
        &self,
        request: Request<MaintenanceModeRequest>,
    ) -> Result<Response<MaintenanceModeResponse>, Status> {
        let req = request.into_inner();
        self.state.daemon_runtime.set_maintenance_mode(req.enable);
        let state_str = if req.enable { "enabled" } else { "disabled" };
        Ok(Response::new(MaintenanceModeResponse {
            maintenance_mode: req.enable,
            message: format!("maintenance mode {state_str}"),
        }))
    }

    async fn config_debug(
        &self,
        request: Request<ConfigDebugRequest>,
    ) -> Result<Response<ConfigDebugResponse>, Status> {
        let req = request.into_inner();
        let content =
            agent_orchestrator::service::system::debug_info(&self.state, req.component.as_deref())
                .map_err(map_core_error)?;
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
            .map_err(map_core_error)?;
        Ok(Response::new(status))
    }

    async fn check(
        &self,
        request: Request<CheckRequest>,
    ) -> Result<Response<CheckResponse>, Status> {
        let req = request.into_inner();
        let report = orchestrator_scheduler::service::system::run_check(
            &self.state,
            req.workflow.as_deref(),
            &req.output_format,
            req.project_id.as_deref(),
        )
        .map_err(map_core_error)?;
        Ok(Response::new(CheckResponse {
            content: report.content,
            format: req.output_format,
            exit_code: report.exit_code,
            diagnostics: report
                .report
                .checks
                .iter()
                .map(orchestrator_scheduler::service::system::diagnostic_entry_from_check)
                .collect(),
        }))
    }

    async fn init(&self, request: Request<InitRequest>) -> Result<Response<InitResponse>, Status> {
        let req = request.into_inner();
        let message =
            agent_orchestrator::service::system::run_init(&self.state, req.root.as_deref())
                .map_err(map_core_error)?;
        Ok(Response::new(InitResponse { message }))
    }

    async fn db_status(
        &self,
        _request: Request<DbStatusRequest>,
    ) -> Result<Response<DbStatusResponse>, Status> {
        let status =
            agent_orchestrator::service::system::db_status(&self.state).map_err(map_core_error)?;
        Ok(Response::new(status))
    }

    async fn db_migrations_list(
        &self,
        _request: Request<DbMigrationsListRequest>,
    ) -> Result<Response<DbMigrationsListResponse>, Status> {
        let list = agent_orchestrator::service::system::db_migrations_list(&self.state)
            .map_err(map_core_error)?;
        Ok(Response::new(list))
    }

    async fn db_vacuum(
        &self,
        _request: Request<DbVacuumRequest>,
    ) -> Result<Response<DbVacuumResponse>, Status> {
        let result = agent_orchestrator::db_maintenance::vacuum_database(&self.state.db_path)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DbVacuumResponse {
            size_before: result.size_before,
            size_after: result.size_after,
            message: "VACUUM complete".into(),
        }))
    }

    async fn db_log_cleanup(
        &self,
        request: Request<DbLogCleanupRequest>,
    ) -> Result<Response<DbLogCleanupResponse>, Status> {
        let req = request.into_inner();
        let days = if req.older_than_days == 0 {
            30
        } else {
            req.older_than_days
        };
        let result = agent_orchestrator::log_cleanup::cleanup_old_logs(
            &self.state.async_database,
            &self.state.logs_dir,
            days,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DbLogCleanupResponse {
            files_deleted: result.files_deleted,
            bytes_freed: result.bytes_freed,
            message: format!("Deleted {} file(s)", result.files_deleted),
        }))
    }

    async fn manifest_validate(
        &self,
        request: Request<ManifestValidateRequest>,
    ) -> Result<Response<ManifestValidateResponse>, Status> {
        let req = request.into_inner();
        let report = agent_orchestrator::service::system::validate_manifests(
            &self.state,
            &req.content,
            req.project_id.as_deref(),
        )
        .map_err(map_core_error)?;
        Ok(Response::new(ManifestValidateResponse {
            valid: report.valid,
            errors: report.errors,
            message: report.message,
            diagnostics: report.diagnostics,
        }))
    }

    async fn manifest_export(
        &self,
        request: Request<ManifestExportRequest>,
    ) -> Result<Response<ManifestExportResponse>, Status> {
        let req = request.into_inner();
        let content = agent_orchestrator::service::resource::export_manifests(
            &self.state,
            &req.output_format,
        )
        .map_err(map_core_error)?;
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
        let result = orchestrator_scheduler::service::task::get_task_trace(
            &self.state,
            &req.task_id,
            req.verbose,
        )
        .await
        .map_err(map_core_error)?;

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

    async fn secret_key_status(
        &self,
        _request: Request<SecretKeyStatusRequest>,
    ) -> Result<Response<SecretKeyStatusResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_status not available in test harness",
        ))
    }

    async fn secret_key_list(
        &self,
        _request: Request<SecretKeyListRequest>,
    ) -> Result<Response<SecretKeyListResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_list not available in test harness",
        ))
    }

    async fn secret_key_rotate(
        &self,
        _request: Request<SecretKeyRotateRequest>,
    ) -> Result<Response<SecretKeyRotateResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_rotate not available in test harness",
        ))
    }

    async fn secret_key_revoke(
        &self,
        _request: Request<SecretKeyRevokeRequest>,
    ) -> Result<Response<SecretKeyRevokeResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_revoke not available in test harness",
        ))
    }

    async fn secret_key_history(
        &self,
        _request: Request<SecretKeyHistoryRequest>,
    ) -> Result<Response<SecretKeyHistoryResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_history not available in test harness",
        ))
    }

    async fn agent_list(
        &self,
        request: Request<AgentListRequest>,
    ) -> Result<Response<AgentListResponse>, Status> {
        let req = request.into_inner();
        let active = agent_orchestrator::config_load::read_active_config(&self.state)
            .map_err(|e| Status::internal(e.to_string()))?;
        let project_id = req.project_id.as_deref().unwrap_or("");
        let agents = agent_orchestrator::selection::resolve_effective_agents(
            project_id,
            &active.config,
            None,
        );
        let lifecycle_map = self.state.agent_lifecycle.read().await;
        let health_map = self.state.agent_health.read().await;

        let mut statuses: Vec<AgentStatus> = agents
            .iter()
            .map(|(id, cfg)| {
                let runtime = lifecycle_map.get(id).cloned().unwrap_or_default();
                let (is_healthy, diseased_until, consecutive_errors) =
                    agent_orchestrator::health::agent_health_summary(&health_map, id);
                AgentStatus {
                    name: id.clone(),
                    enabled: cfg.enabled,
                    lifecycle_state: runtime.lifecycle.as_str().to_string(),
                    in_flight_items: runtime.in_flight_items as i32,
                    capabilities: cfg.capabilities.clone(),
                    drain_requested_at: runtime.drain_requested_at.map(|dt| dt.to_rfc3339()),
                    is_healthy,
                    diseased_until,
                    consecutive_errors: consecutive_errors as i32,
                }
            })
            .collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Response::new(AgentListResponse { agents: statuses }))
    }

    async fn agent_cordon(
        &self,
        request: Request<AgentCordonRequest>,
    ) -> Result<Response<AgentCordonResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::agent_lifecycle::cordon_agent(&self.state, &req.agent_name)
            .await
            .map_err(Status::failed_precondition)?;
        Ok(Response::new(AgentCordonResponse {
            message: format!("agent '{}' cordoned", req.agent_name),
        }))
    }

    async fn agent_uncordon(
        &self,
        request: Request<AgentUncordonRequest>,
    ) -> Result<Response<AgentUncordonResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::agent_lifecycle::uncordon_agent(&self.state, &req.agent_name)
            .await
            .map_err(Status::failed_precondition)?;
        Ok(Response::new(AgentUncordonResponse {
            message: format!("agent '{}' uncordoned", req.agent_name),
        }))
    }

    async fn agent_drain(
        &self,
        request: Request<AgentDrainRequest>,
    ) -> Result<Response<AgentDrainResponse>, Status> {
        let req = request.into_inner();
        let result_state = agent_orchestrator::agent_lifecycle::drain_agent(
            &self.state,
            &req.agent_name,
            req.timeout_secs,
        )
        .await
        .map_err(Status::failed_precondition)?;
        Ok(Response::new(AgentDrainResponse {
            message: format!(
                "agent '{}' drain initiated — state: {}",
                req.agent_name,
                result_state.as_str()
            ),
            lifecycle_state: result_state.as_str().to_string(),
        }))
    }

    async fn event_cleanup(
        &self,
        request: Request<EventCleanupRequest>,
    ) -> Result<Response<EventCleanupResponse>, Status> {
        let req = request.into_inner();
        let older_than = if req.older_than_days == 0 {
            30
        } else {
            req.older_than_days
        };
        if req.dry_run {
            let count = agent_orchestrator::event_cleanup::count_pending_cleanup(
                &self.state.async_database,
                older_than,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Response::new(EventCleanupResponse {
                affected_count: count,
                message: format!("{count} events (dry-run)"),
            }));
        }
        let affected = if req.archive {
            let archive_dir = self.state.data_dir.join("archive/events");
            agent_orchestrator::event_cleanup::archive_events(
                &self.state.async_database,
                &archive_dir,
                older_than,
                1000,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        } else {
            agent_orchestrator::event_cleanup::cleanup_old_events(
                &self.state.async_database,
                older_than,
                1000,
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?
        };
        Ok(Response::new(EventCleanupResponse {
            affected_count: affected,
            message: format!("{affected} events deleted"),
        }))
    }

    async fn event_stats(
        &self,
        _request: Request<EventStatsRequest>,
    ) -> Result<Response<EventStatsResponse>, Status> {
        let stats = agent_orchestrator::event_cleanup::event_stats(&self.state.async_database)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(EventStatsResponse {
            total_rows: stats.total_rows,
            earliest: stats.earliest.unwrap_or_default(),
            latest: stats.latest.unwrap_or_default(),
            by_task_status: stats
                .by_task_status
                .into_iter()
                .map(|(status, count)| EventStatusCount { status, count })
                .collect(),
        }))
    }

    async fn task_events(
        &self,
        request: Request<TaskEventsRequest>,
    ) -> Result<Response<TaskEventsResponse>, Status> {
        let req = request.into_inner();
        let type_filter = if req.event_type_filter.is_empty() {
            None
        } else {
            Some(req.event_type_filter.as_str())
        };
        let events = agent_orchestrator::event_cleanup::list_task_events(
            &self.state.async_database,
            &req.task_id,
            type_filter,
            req.limit,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(TaskEventsResponse {
            events: events
                .into_iter()
                .map(|e| Event {
                    id: e.id,
                    task_id: e.task_id,
                    task_item_id: e.task_item_id,
                    event_type: e.event_type,
                    payload_json: serde_json::to_string(&e.payload).unwrap_or_default(),
                    created_at: e.created_at,
                })
                .collect(),
        }))
    }

    async fn trigger_suspend(
        &self,
        request: Request<TriggerSuspendRequest>,
    ) -> Result<Response<TriggerSuspendResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::resource::suspend_trigger(
            &self.state,
            &req.trigger_name,
            req.project.as_deref(),
        )
        .map_err(map_core_error)?;
        Ok(Response::new(TriggerSuspendResponse {
            message: format!("trigger '{}' suspended", req.trigger_name),
        }))
    }

    async fn trigger_resume(
        &self,
        request: Request<TriggerResumeRequest>,
    ) -> Result<Response<TriggerResumeResponse>, Status> {
        let req = request.into_inner();
        agent_orchestrator::service::resource::resume_trigger(
            &self.state,
            &req.trigger_name,
            req.project.as_deref(),
        )
        .map_err(map_core_error)?;
        Ok(Response::new(TriggerResumeResponse {
            message: format!("trigger '{}' resumed", req.trigger_name),
        }))
    }

    async fn trigger_fire(
        &self,
        request: Request<TriggerFireRequest>,
    ) -> Result<Response<TriggerFireResponse>, Status> {
        let req = request.into_inner();
        let task_id = agent_orchestrator::service::resource::fire_trigger(
            &self.state,
            &req.trigger_name,
            req.project.as_deref(),
        )
        .map_err(map_core_error)?;
        Ok(Response::new(TriggerFireResponse {
            task_id: task_id.clone(),
            message: format!("trigger '{}' fired — task {}", req.trigger_name, task_id),
        }))
    }

    async fn qa_doctor(
        &self,
        request: Request<QaDoctorRequest>,
    ) -> Result<Response<QaDoctorResponse>, Status> {
        let _ = request.into_inner();
        let stats =
            agent_orchestrator::qa_doctor::qa_doctor_stats(&self.state.async_database)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(QaDoctorResponse {
            task_execution_metrics_total: stats.task_execution_metrics_total,
            task_execution_metrics_last_24h: stats.task_execution_metrics_last_24h,
            task_completion_rate: stats.task_completion_rate,
        }))
    }
}

// ---------------------------------------------------------------------------
// TestHarness — spins up in-process gRPC server + client
// ---------------------------------------------------------------------------

/// Integration test harness. Creates an isolated state, starts an in-process
/// gRPC server on a random TCP port, and provides a connected client.
pub struct TestHarness {
    _test_state: TestState,
    state: Arc<InnerState>,
    client: OrchestratorServiceClient<Channel>,
    _server_handle: JoinHandle<()>,
}

impl TestHarness {
    /// Start the harness with a manifest YAML applied to the state.
    pub async fn start_with_manifest(manifest_yaml: &str) -> Self {
        let mut test_state = TestState::new();
        let state = test_state.build();

        // Rewrite relative workspace root_path values to point at the test
        // temp directory so workspace validation succeeds.
        let ws_root = state.data_dir.join("workspace/default");
        let resolved_yaml = manifest_yaml.replace(
            "root_path: \".\"",
            &format!("root_path: \"{}\"", ws_root.display()),
        );

        // Apply manifest
        agent_orchestrator::service::resource::apply_manifests(
            &state,
            &resolved_yaml,
            false,
            None,
            false,
        )
        .expect("failed to apply test manifest");

        Self::start_inner(test_state, state).await
    }

    /// Start the harness without any manifest (bare state).
    pub async fn start() -> Self {
        let mut test_state = TestState::new();
        let state = test_state.build();
        Self::start_inner(test_state, state).await
    }

    async fn start_inner(test_state: TestState, state: Arc<InnerState>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test TCP listener");
        let addr: SocketAddr = listener.local_addr().expect("no local addr");

        let shutdown_notify = Arc::new(Notify::new());
        let server = TestOrchestratorServer {
            state: state.clone(),
            shutdown_notify: shutdown_notify.clone(),
        };

        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let shutdown = shutdown_notify.clone();
        let server_handle = tokio::spawn(async move {
            Server::builder()
                .add_service(OrchestratorServiceServer::new(server))
                .serve_with_incoming_shutdown(incoming, shutdown.notified())
                .await
                .expect("gRPC server error");
        });

        // Give the server a moment to start accepting connections
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let channel = Channel::from_shared(format!("http://{addr}"))
            .expect("invalid channel URI")
            .connect()
            .await
            .expect("failed to connect to test gRPC server");
        let client = OrchestratorServiceClient::new(channel);

        Self {
            _test_state: test_state,
            state,
            client,
            _server_handle: server_handle,
        }
    }

    /// Get a clone of the gRPC client.
    pub fn client(&self) -> OrchestratorServiceClient<Channel> {
        self.client.clone()
    }

    /// Direct access to the shared state (for driving task execution).
    pub fn state(&self) -> &Arc<InnerState> {
        &self.state
    }

    /// Seed a minimal QA markdown file in the default workspace so task
    /// creation finds at least one target.
    pub fn seed_qa_file(&self) {
        let active = agent_orchestrator::config_load::read_active_config(&self.state)
            .expect("read active config");
        let ws = active
            .workspaces
            .get("default")
            .expect("default workspace should exist");
        for qa_target in &ws.qa_targets {
            let qa_path = ws.root_path.join(qa_target);
            std::fs::create_dir_all(&qa_path).expect("failed to create qa dir");
            std::fs::write(
                qa_path.join("integration-test.md"),
                "# Integration Test QA\n",
            )
            .expect("failed to write QA file");
        }
    }
}
