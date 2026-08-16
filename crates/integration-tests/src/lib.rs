//! Integration test harness for the orchestrator.
//!
//! Provides [`TestHarness`] which spins up an in-process gRPC server backed by
//! a real [`InnerState`] and returns a connected [`OrchestratorServiceClient`].

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use agent_orchestrator::action_audit::{
    ActionAuditFilter, ActionAuditRecord as CoreActionAuditRecord, AsyncActionAuditRepository,
};
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

fn action_audit_to_proto(record: CoreActionAuditRecord) -> ActionAuditRecord {
    ActionAuditRecord {
        request_id: record.request_id,
        schema_version: record.schema_version,
        project_id: record.project_id,
        actor: record.actor,
        resolved_role: record.resolved_role,
        transport: record.transport,
        target_type: record.target_type,
        target_id: record.target_id,
        action: record.action,
        reason_code: record.reason_code,
        operator_reason: record.operator_reason,
        idempotency_key: record.idempotency_key,
        expected_version: record.expected_version,
        fencing_token: record.fencing_token,
        request_hash: record.request_hash,
        status: record.status,
        error_code: record.error_code,
        result_type: record.result_type,
        result_id: record.result_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
        completed_at: record.completed_at,
    }
}

// ---------------------------------------------------------------------------
// Test gRPC server — thin delegation to core service functions
// ---------------------------------------------------------------------------

/// In-process gRPC server for integration tests. Mirrors the daemon's server
/// but skips authorization and shutdown rejection.
pub struct TestOrchestratorServer {
    state: Arc<InnerState>,
}

type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[tonic::async_trait]
impl OrchestratorService for TestOrchestratorServer {
    type TaskLogsStream = BoxStream<TaskLogChunk>;
    type TaskFollowStream = BoxStream<TaskLogLine>;
    type TaskWatchStream = BoxStream<TaskWatchSnapshot>;
    type TaskTimelineFollowStream = BoxStream<TimelineDelta>;
    type AttentionFollowStream = BoxStream<AttentionDelta>;
    type SourceAutomationWatchStream = BoxStream<SourceAutomationDelta>;
    type SourceConnectionWatchStream = BoxStream<SourceConnectionDelta>;
    type AgentSessionReadStream = BoxStream<AgentSessionOutputChunk>;

    async fn action_audit_list(
        &self,
        request: Request<ActionAuditListRequest>,
    ) -> Result<Response<ActionAuditListResponse>, Status> {
        let request = request.into_inner();
        let records = AsyncActionAuditRepository::new(self.state.async_database.clone())
            .list(ActionAuditFilter {
                project_id: request.project_id,
                actor: request.actor,
                target_type: request.target_type,
                target_id: request.target_id,
                action: request.action,
                status: request.status,
                from_time: request.from_time,
                to_time: request.to_time,
                limit: request.limit as usize,
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        Ok(Response::new(ActionAuditListResponse {
            records: records.into_iter().map(action_audit_to_proto).collect(),
        }))
    }

    async fn action_audit_get(
        &self,
        request: Request<ActionAuditGetRequest>,
    ) -> Result<Response<ActionAuditRecord>, Status> {
        let request = request.into_inner();
        let record = AsyncActionAuditRepository::new(self.state.async_database.clone())
            .get(&request.project_id, &request.request_id)
            .await
            .map_err(|error| Status::internal(error.to_string()))?
            .ok_or_else(|| Status::not_found("action audit record not found"))?;
        Ok(Response::new(action_audit_to_proto(record)))
    }

    async fn source_event_list(
        &self,
        _: Request<SourceEventListRequest>,
    ) -> Result<Response<SourceEventListResponse>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_event_get(
        &self,
        _: Request<SourceEventGetRequest>,
    ) -> Result<Response<SourceEvent>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_route_get(
        &self,
        _: Request<SourceAutomationRouteGetRequest>,
    ) -> Result<Response<SourceAutomationRoute>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_connection_list(
        &self,
        _: Request<SourceConnectionListRequest>,
    ) -> Result<Response<SourceConnectionListResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_get(
        &self,
        _: Request<SourceConnectionGetRequest>,
    ) -> Result<Response<SourceConnection>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_watch(
        &self,
        _: Request<SourceConnectionWatchRequest>,
    ) -> Result<Response<Self::SourceConnectionWatchStream>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_catalog_get(
        &self,
        _: Request<SourceConnectionCatalogRequest>,
    ) -> Result<Response<SourceConnectionCatalogResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_connect(
        &self,
        _: Request<SourceConnectionConnectRequest>,
    ) -> Result<Response<SourceConnectionIntentResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_intent_get(
        &self,
        _: Request<SourceConnectionIntentGetRequest>,
    ) -> Result<Response<SourceConnectionIntentResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_cancel(
        &self,
        _: Request<SourceConnectionIntentMutationRequest>,
    ) -> Result<Response<SourceConnectionIntentResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_reauthorize(
        &self,
        _: Request<SourceConnectionMutationRequest>,
    ) -> Result<Response<SourceConnectionIntentResponse>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_disconnect(
        &self,
        _: Request<SourceConnectionMutationRequest>,
    ) -> Result<Response<SourceConnection>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_transfer(
        &self,
        _: Request<SourceConnectionTransferRequest>,
    ) -> Result<Response<SourceConnection>, Status> {
        Err(Status::unimplemented(
            "managed source connection uses production daemon",
        ))
    }

    async fn source_connection_dedicated_preview(
        &self,
        _: Request<SourceConnectionDedicatedPreviewRequest>,
    ) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack provisioning uses production daemon",
        ))
    }

    async fn source_connection_dedicated_approve(
        &self,
        _: Request<SourceConnectionDedicatedMutationRequest>,
    ) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack provisioning uses production daemon",
        ))
    }

    async fn source_connection_dedicated_get(
        &self,
        _: Request<SourceConnectionDedicatedGetRequest>,
    ) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack provisioning uses production daemon",
        ))
    }

    async fn source_connection_dedicated_abandon(
        &self,
        _: Request<SourceConnectionDedicatedMutationRequest>,
    ) -> Result<Response<SourceConnectionDedicatedProvisioningResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack provisioning uses production daemon",
        ))
    }

    async fn source_connection_migrate_to_shared(
        &self,
        _: Request<SourceConnectionMutationRequest>,
    ) -> Result<Response<SourceConnectionIntentResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack migration uses production daemon",
        ))
    }

    async fn source_connection_dedicated_upgrade_preview(
        &self,
        _: Request<SourceConnectionDedicatedUpgradePreviewRequest>,
    ) -> Result<Response<SourceConnectionDedicatedLifecycleResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack lifecycle uses production daemon",
        ))
    }

    async fn source_connection_dedicated_upgrade_apply(
        &self,
        _: Request<SourceConnectionDedicatedUpgradeApplyRequest>,
    ) -> Result<Response<SourceConnectionDedicatedLifecycleResponse>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack lifecycle uses production daemon",
        ))
    }

    async fn source_connection_dedicated_delete(
        &self,
        _: Request<SourceConnectionDedicatedDeleteRequest>,
    ) -> Result<Response<SourceConnection>, Status> {
        Err(Status::unimplemented(
            "dedicated Slack lifecycle uses production daemon",
        ))
    }

    async fn source_automation_list(
        &self,
        _: Request<SourceAutomationListRequest>,
    ) -> Result<Response<SourceAutomationListResponse>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_get(
        &self,
        _: Request<SourceAutomationGetRequest>,
    ) -> Result<Response<SourceAutomationDetail>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_watch(
        &self,
        _: Request<SourceAutomationWatchRequest>,
    ) -> Result<Response<Self::SourceAutomationWatchStream>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_simulate(
        &self,
        _: Request<SourceAutomationSimulateRequest>,
    ) -> Result<Response<SourceAutomationSimulateResponse>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_replay(
        &self,
        _: Request<SourceAutomationMutationRequest>,
    ) -> Result<Response<SourceAutomationRoute>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_ignore(
        &self,
        _: Request<SourceAutomationMutationRequest>,
    ) -> Result<Response<SourceAutomationRoute>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_status_get(
        &self,
        _: Request<SourceAutomationStatusRequest>,
    ) -> Result<Response<SourceAutomationStatusResponse>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_automation_catalog_get(
        &self,
        _: Request<SourceAutomationCatalogRequest>,
    ) -> Result<Response<SourceAutomationCatalogResponse>, Status> {
        Err(Status::unimplemented(
            "source automation integration fixture uses the production daemon",
        ))
    }

    async fn source_event_ingest(
        &self,
        _: Request<SourceEventIngestRequest>,
    ) -> Result<Response<SourceEventIngestResponse>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_binding_list(
        &self,
        _: Request<SourceBindingListRequest>,
    ) -> Result<Response<SourceBindingListResponse>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_bind(
        &self,
        _: Request<SourceBindRequest>,
    ) -> Result<Response<SourceBinding>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_replay(
        &self,
        _: Request<SourceReplayRequest>,
    ) -> Result<Response<SourceReplayResponse>, Status> {
        Err(Status::unimplemented(
            "source integration fixture uses the production daemon",
        ))
    }

    async fn source_task_template_preview(
        &self,
        _: Request<SourceTaskTemplatePreviewRequest>,
    ) -> Result<Response<SourceTaskTemplatePreviewResponse>, Status> {
        Err(Status::unimplemented(
            "source template integration fixture uses the production daemon",
        ))
    }

    async fn source_task_binding_simulate(
        &self,
        _: Request<SourceTaskBindingSimulateRequest>,
    ) -> Result<Response<SourceTaskBindingSimulateResponse>, Status> {
        Err(Status::unimplemented(
            "source binding integration fixture uses the production daemon",
        ))
    }

    async fn source_task_binding_suspend(
        &self,
        _: Request<SourceTaskBindingMutationRequest>,
    ) -> Result<Response<SourceTaskBindingMutationResponse>, Status> {
        Err(Status::unimplemented(
            "source binding integration fixture uses the production daemon",
        ))
    }

    async fn source_task_binding_resume(
        &self,
        _: Request<SourceTaskBindingMutationRequest>,
    ) -> Result<Response<SourceTaskBindingMutationResponse>, Status> {
        Err(Status::unimplemented(
            "source binding integration fixture uses the production daemon",
        ))
    }

    async fn agent_session_list(
        &self,
        _: Request<AgentSessionListRequest>,
    ) -> Result<Response<AgentSessionListResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_get(
        &self,
        _: Request<AgentSessionGetRequest>,
    ) -> Result<Response<AgentSessionGetResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_attach(
        &self,
        _: Request<AgentSessionAttachRequest>,
    ) -> Result<Response<AgentSessionAttachResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_heartbeat(
        &self,
        _: Request<AgentSessionHeartbeatRequest>,
    ) -> Result<Response<AgentSessionHeartbeatResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_detach(
        &self,
        _: Request<AgentSessionDetachRequest>,
    ) -> Result<Response<AgentSessionDetachResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_send_input(
        &self,
        _: Request<AgentSessionSendInputRequest>,
    ) -> Result<Response<AgentSessionSendInputResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_read(
        &self,
        _: Request<AgentSessionReadRequest>,
    ) -> Result<Response<Self::AgentSessionReadStream>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_close(
        &self,
        _: Request<AgentSessionCloseRequest>,
    ) -> Result<Response<AgentSessionCloseResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }
    async fn agent_session_resolve_pid(
        &self,
        _: Request<AgentSessionResolvePidRequest>,
    ) -> Result<Response<AgentSessionResolvePidResponse>, Status> {
        Err(Status::unimplemented(
            "session integration fixture uses the production daemon",
        ))
    }

    async fn attention_list(
        &self,
        _request: Request<AttentionListRequest>,
    ) -> Result<Response<AttentionListResponse>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_get(
        &self,
        _request: Request<AttentionGetRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_claim(
        &self,
        _request: Request<AttentionClaimRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_snooze(
        &self,
        _request: Request<AttentionSnoozeRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_resolve(
        &self,
        _request: Request<AttentionResolveRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_execute_action(
        &self,
        _request: Request<AttentionExecuteActionRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn attention_follow(
        &self,
        _request: Request<AttentionFollowRequest>,
    ) -> Result<Response<Self::AttentionFollowStream>, Status> {
        Err(Status::unimplemented(
            "attention integration fixture uses the production daemon",
        ))
    }

    async fn handoff_generate(
        &self,
        _request: Request<HandoffGenerateRequest>,
    ) -> Result<Response<HandoffSnapshotResponse>, Status> {
        Err(Status::unimplemented(
            "handoff integration fixture uses the production daemon",
        ))
    }

    async fn handoff_get(
        &self,
        _request: Request<HandoffGetRequest>,
    ) -> Result<Response<HandoffSnapshotResponse>, Status> {
        Err(Status::unimplemented(
            "handoff integration fixture uses the production daemon",
        ))
    }

    async fn resume_boundary_list(
        &self,
        _request: Request<ResumeBoundaryListRequest>,
    ) -> Result<Response<ResumeBoundaryListResponse>, Status> {
        Err(Status::unimplemented(
            "resume integration fixture uses the production daemon",
        ))
    }

    async fn resume_plan(
        &self,
        _request: Request<ResumePlanRequest>,
    ) -> Result<Response<ResumePlanResponse>, Status> {
        Err(Status::unimplemented(
            "resume integration fixture uses the production daemon",
        ))
    }

    async fn resume_execute(
        &self,
        _request: Request<ResumeExecuteRequest>,
    ) -> Result<Response<ResumeExecuteResponse>, Status> {
        Err(Status::unimplemented(
            "resume integration fixture uses the production daemon",
        ))
    }

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
            step_filter: if req.step_filter.is_empty() {
                None
            } else {
                Some(req.step_filter)
            },
            initial_vars: if req.initial_vars.is_empty() {
                None
            } else {
                Some(req.initial_vars)
            },
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
        _: Request<TaskRetryRequest>,
    ) -> Result<Response<TaskRetryResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_recover(
        &self,
        _: Request<TaskRecoverRequest>,
    ) -> Result<Response<TaskRecoverResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
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

    async fn task_timeline(
        &self,
        _: Request<TaskTimelineRequest>,
    ) -> Result<Response<TaskTimelineResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_timeline_follow(
        &self,
        _: Request<TaskTimelineFollowRequest>,
    ) -> Result<Response<Self::TaskTimelineFollowStream>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_logs(
        &self,
        _: Request<TaskLogsRequest>,
    ) -> Result<Response<Self::TaskLogsStream>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_follow(
        &self,
        _: Request<TaskFollowRequest>,
    ) -> Result<Response<Self::TaskFollowStream>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_watch(
        &self,
        _: Request<TaskWatchRequest>,
    ) -> Result<Response<Self::TaskWatchStream>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn apply(&self, _: Request<ApplyRequest>) -> Result<Response<ApplyResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
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

    async fn resource_catalog_list(
        &self,
        request: Request<ResourceCatalogListRequest>,
    ) -> Result<Response<ResourceCatalogListResponse>, Status> {
        let req = request.into_inner();
        let page = agent_orchestrator::service::resource::list_resource_summaries(
            &self.state,
            &req.resource_type,
            req.project.as_deref(),
            req.cursor.as_deref(),
            if req.limit == 0 {
                100
            } else {
                req.limit as usize
            },
        )
        .map_err(map_core_error)?;
        Ok(Response::new(ResourceCatalogListResponse {
            resources: page
                .resources
                .into_iter()
                .map(|resource| ResourceSummary {
                    kind: resource.kind,
                    name: resource.name,
                    project_id: resource.project_id,
                    revision: resource.revision,
                    source: Some(resource.source),
                })
                .collect(),
            next_cursor: page.next_cursor,
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
            resource: None,
        }))
    }

    async fn delete(&self, _: Request<DeleteRequest>) -> Result<Response<DeleteResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn store_get(
        &self,
        _: Request<StoreGetRequest>,
    ) -> Result<Response<StoreGetResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn store_put(
        &self,
        _: Request<StorePutRequest>,
    ) -> Result<Response<StorePutResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn store_delete(
        &self,
        _: Request<StoreDeleteRequest>,
    ) -> Result<Response<StoreDeleteResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn store_list(
        &self,
        _: Request<StoreListRequest>,
    ) -> Result<Response<StoreListResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn store_prune(
        &self,
        _: Request<StorePruneRequest>,
    ) -> Result<Response<StorePruneResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    /// Deliberately not modelled.
    ///
    /// A double that answered this would answer "ready" by construction, and a
    /// readiness contract asserted against a stand-in that cannot be unready is
    /// asserted against nothing. Readiness is exercised against the real
    /// `OrchestratorServer` (QA 216 / `test-daemon-readiness.sh`), the same
    /// reason FR-164's audit assertions could not live here.
    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        Err(Status::unimplemented(
            "readiness is asserted against the production daemon, not this fixture",
        ))
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
        _: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn maintenance_mode(
        &self,
        _: Request<MaintenanceModeRequest>,
    ) -> Result<Response<MaintenanceModeResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn config_debug(
        &self,
        _: Request<ConfigDebugRequest>,
    ) -> Result<Response<ConfigDebugResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn worker_status(
        &self,
        _: Request<WorkerStatusRequest>,
    ) -> Result<Response<WorkerStatusResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn check(&self, _: Request<CheckRequest>) -> Result<Response<CheckResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn init(&self, _: Request<InitRequest>) -> Result<Response<InitResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn db_status(
        &self,
        _: Request<DbStatusRequest>,
    ) -> Result<Response<DbStatusResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn db_migrations_list(
        &self,
        _: Request<DbMigrationsListRequest>,
    ) -> Result<Response<DbMigrationsListResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn db_vacuum(
        &self,
        _: Request<DbVacuumRequest>,
    ) -> Result<Response<DbVacuumResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn db_log_cleanup(
        &self,
        _: Request<DbLogCleanupRequest>,
    ) -> Result<Response<DbLogCleanupResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn manifest_validate(
        &self,
        _: Request<ManifestValidateRequest>,
    ) -> Result<Response<ManifestValidateResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn manifest_export(
        &self,
        _: Request<ManifestExportRequest>,
    ) -> Result<Response<ManifestExportResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_trace(
        &self,
        _: Request<TaskTraceRequest>,
    ) -> Result<Response<TaskTraceResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
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

    async fn secret_key_bootstrap(
        &self,
        _request: Request<SecretKeyBootstrapRequest>,
    ) -> Result<Response<SecretKeyBootstrapResponse>, Status> {
        Err(Status::unimplemented(
            "secret_key_bootstrap not available in test harness",
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
        _: Request<EventCleanupRequest>,
    ) -> Result<Response<EventCleanupResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn event_stats(
        &self,
        _: Request<EventStatsRequest>,
    ) -> Result<Response<EventStatsResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn task_events(
        &self,
        _: Request<TaskEventsRequest>,
    ) -> Result<Response<TaskEventsResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn trigger_suspend(
        &self,
        _: Request<TriggerSuspendRequest>,
    ) -> Result<Response<TriggerSuspendResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn trigger_resume(
        &self,
        _: Request<TriggerResumeRequest>,
    ) -> Result<Response<TriggerResumeResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn trigger_fire(
        &self,
        _: Request<TriggerFireRequest>,
    ) -> Result<Response<TriggerFireResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn qa_doctor(
        &self,
        _: Request<QaDoctorRequest>,
    ) -> Result<Response<QaDoctorResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn process_metrics_get(
        &self,
        request: Request<ProcessMetricsGetRequest>,
    ) -> Result<Response<ProcessMetricsGetResponse>, Status> {
        let req = request.into_inner();
        let (window_seconds, bucket_seconds) =
            agent_orchestrator::process_metrics::validate_window_bucket(
                &req.window,
                &req.bucket,
                30,
            )
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let metrics = agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
            self.state.async_database.clone(),
        )
        .query(agent_orchestrator::process_metrics::ProcessMetricsQuery {
            project_id: req.project_id,
            window_seconds,
            bucket_seconds,
            collection_enabled: true,
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        Ok(Response::new(ProcessMetricsGetResponse {
            schema_version: metrics.schema_version,
            metrics_json: serde_json::to_string(&metrics)
                .map_err(|error| Status::internal(error.to_string()))?,
        }))
    }

    async fn process_metric_record(
        &self,
        request: Request<ProcessMetricRecordRequest>,
    ) -> Result<Response<ProcessMetricRecordResponse>, Status> {
        let req = request.into_inner();
        let recorded_at = agent_orchestrator::config_load::now_ts();
        let inserted = agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
            self.state.async_database.clone(),
        )
        .record(agent_orchestrator::process_metrics::MetricObservation {
            project_id: req.project_id,
            metric_name: req.metric_name,
            dimensions: req.dimensions.into_iter().collect(),
            value: req.value,
            occurred_at: recorded_at.clone(),
            source_kind: "integration".into(),
            source_key: req.source_key,
        })
        .await
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(ProcessMetricRecordResponse {
            inserted,
            recorded_at,
        }))
    }

    async fn process_metrics_rebuild(
        &self,
        request: Request<ProcessMetricsRebuildRequest>,
    ) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
        let project_id = request.into_inner().project_id;
        let affected_rows =
            agent_orchestrator::process_metrics::AsyncProcessMetricsRepository::new(
                self.state.async_database.clone(),
            )
            .rebuild(&project_id)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        Ok(Response::new(ProcessMetricsMaintenanceResponse {
            affected_rows,
            message: "rebuilt".into(),
        }))
    }

    async fn process_metrics_prune(
        &self,
        _: Request<ProcessMetricsPruneRequest>,
    ) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
    }

    async fn run_step(
        &self,
        _: Request<RunStepRequest>,
    ) -> Result<Response<RunStepResponse>, Status> {
        Err(Status::unimplemented(
            "no integration test drives this RPC; the production daemon owns it",
        ))
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
    channel: Channel,
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
        let client = OrchestratorServiceClient::new(channel.clone());

        Self {
            _test_state: test_state,
            state,
            channel,
            client,
            _server_handle: server_handle,
        }
    }

    /// Get a clone of the gRPC client.
    pub fn client(&self) -> OrchestratorServiceClient<Channel> {
        self.client.clone()
    }

    /// Get the connected raw channel for testing another real adapter.
    pub fn channel(&self) -> Channel {
        self.channel.clone()
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
