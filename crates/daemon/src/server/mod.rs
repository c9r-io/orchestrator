mod action_audit;
mod agent;
mod attention;
mod handoff;
mod mapping;
pub(crate) mod process_metrics;
mod resource;
mod secret;
mod session;
mod source;
mod store;
mod system;
mod task;
mod trigger;

use std::sync::Arc;

use agent_orchestrator::error::{ErrorCategory, OrchestratorError};
use agent_orchestrator::state::InnerState;
use orchestrator_proto::*;
use tokio::sync::Notify;
use tonic::{Request, Response, Status};

use crate::control_plane::{AuthzError, ControlPlaneSecurity, Role, required_role_for_rpc};
use crate::uds_security::{UdsAuthPolicy, UdsPeerInfo};

/// gRPC service implementation — thin translation layer from gRPC requests
/// to core service calls.
pub struct OrchestratorServer {
    pub(crate) state: Arc<InnerState>,
    pub(crate) shutdown_notify: Arc<Notify>,
    pub(crate) control_plane: Option<Arc<ControlPlaneSecurity>>,
    pub(crate) uds_auth_policy: Option<UdsAuthPolicy>,
    pub(crate) session_read_limits: session::SessionReadLimits,
}

impl OrchestratorServer {
    /// Construct a gRPC server facade around shared daemon state.
    pub fn new(
        state: Arc<InnerState>,
        shutdown_notify: Arc<Notify>,
        control_plane: Option<Arc<ControlPlaneSecurity>>,
        uds_auth_policy: Option<UdsAuthPolicy>,
    ) -> Self {
        Self {
            state,
            shutdown_notify,
            control_plane,
            uds_auth_policy,
            session_read_limits: session::SessionReadLimits::default(),
        }
    }

    pub(crate) fn reject_new_work_during_shutdown(&self, rpc: &'static str) -> Option<Status> {
        let snapshot = agent_orchestrator::service::daemon::runtime_snapshot(&self.state);
        if snapshot.shutdown_requested {
            return Some(Status::unavailable(format!(
                "{rpc} rejected: daemon is {}",
                snapshot.lifecycle_state.as_str()
            )));
        }
        if snapshot.maintenance_mode {
            return Some(Status::unavailable(format!(
                "{rpc} rejected: daemon is in maintenance mode"
            )));
        }
        None
    }
}

pub(crate) fn authorize<T>(
    server: &OrchestratorServer,
    request: &Request<T>,
    rpc: &'static str,
) -> std::result::Result<(), AuthzError> {
    let request_id = request
        .extensions()
        .get::<crate::control_plane::ActionRequestId>()
        .map(|value| value.0.as_str());
    match &server.control_plane {
        Some(control_plane) => control_plane.authorize(request, rpc),
        None => {
            let required = required_role_for_rpc(rpc);
            let peer = request.extensions().get::<UdsPeerInfo>();
            let effective_role = server
                .uds_auth_policy
                .as_ref()
                .map(|p| p.max_role)
                .unwrap_or(Role::Operator);
            let audit_all_reads = server
                .uds_auth_policy
                .as_ref()
                .is_some_and(|p| p.audit_all_reads);

            // Phase 4: optional UDS authorization policy
            if let Some(policy) = &server.uds_auth_policy {
                if !policy.max_role.allows(required) {
                    uds_audit(
                        &server.state.db_path,
                        rpc,
                        peer,
                        "denied",
                        Some("uds_policy_denied"),
                        Some(effective_role),
                        request_id,
                    );
                    return Err(AuthzError::PermissionDenied(
                        "UDS policy restricts this operation",
                    ));
                }
            }

            // Phase 3: audit mutating operations on UDS (and read-only if configured)
            if required != Role::ReadOnly || audit_all_reads {
                uds_audit(
                    &server.state.db_path,
                    rpc,
                    peer,
                    "allowed",
                    None,
                    Some(effective_role),
                    request_id,
                );
            }

            Ok(())
        }
    }
}

fn trusted_actor<T>(request: &Request<T>) -> String {
    crate::control_plane::subject_id_from_extensions(request.extensions())
        .or_else(|| {
            request
                .extensions()
                .get::<UdsPeerInfo>()
                .map(|peer| format!("uid:{}", peer.uid))
        })
        .unwrap_or_else(|| "local-operator".to_string())
}

fn uds_audit(
    db_path: &std::path::Path,
    rpc: &str,
    peer: Option<&UdsPeerInfo>,
    authz_result: &str,
    rejection_stage: Option<&str>,
    effective_role: Option<Role>,
    request_id: Option<&str>,
) {
    use agent_orchestrator::db::{ControlPlaneAuditRecord, insert_control_plane_audit};
    let peer_exe = peer
        .and_then(|p| p.pid)
        .and_then(crate::uds_security::resolve_peer_exe);
    let _ = insert_control_plane_audit(
        db_path,
        &ControlPlaneAuditRecord {
            request_id: request_id.map(str::to_owned),
            transport: "uds".into(),
            remote_addr: peer.and_then(|p| p.pid.map(|pid| format!("pid:{pid}"))),
            rpc: rpc.into(),
            subject_id: peer.map(|p| format!("uid:{}", p.uid)),
            authn_result: "peer_cred".into(),
            authz_result: authz_result.into(),
            role: effective_role.map(|r| r.as_str().to_string()),
            reason: rejection_stage.map(|s| s.to_string()),
            tls_fingerprint: None,
            rejection_stage: rejection_stage.map(|s| s.to_string()),
            traffic_class: None,
            limit_scope: None,
            decision: None,
            reason_code: None,
            peer_exe,
        },
    );
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

#[tonic::async_trait]
impl OrchestratorService for OrchestratorServer {
    type TaskLogsStream = task::TaskLogsStream;
    type TaskFollowStream = task::TaskFollowStream;
    type TaskWatchStream = task::TaskWatchStream;
    type TaskTimelineFollowStream = task::TaskTimelineFollowStream;
    type AttentionFollowStream = attention::AttentionFollowStream;
    type AgentSessionReadStream = session::AgentSessionReadStream;

    async fn task_create(
        &self,
        request: Request<TaskCreateRequest>,
    ) -> Result<Response<TaskCreateResponse>, Status> {
        task::task_create(self, request).await
    }

    async fn task_start(
        &self,
        request: Request<TaskStartRequest>,
    ) -> Result<Response<TaskStartResponse>, Status> {
        task::task_start(self, request).await
    }

    async fn task_pause(
        &self,
        request: Request<TaskPauseRequest>,
    ) -> Result<Response<TaskPauseResponse>, Status> {
        task::task_pause(self, request).await
    }

    async fn task_resume(
        &self,
        request: Request<TaskResumeRequest>,
    ) -> Result<Response<TaskResumeResponse>, Status> {
        task::task_resume(self, request).await
    }

    async fn task_delete(
        &self,
        request: Request<TaskDeleteRequest>,
    ) -> Result<Response<TaskDeleteResponse>, Status> {
        task::task_delete(self, request).await
    }

    async fn task_delete_bulk(
        &self,
        request: Request<TaskDeleteBulkRequest>,
    ) -> Result<Response<TaskDeleteBulkResponse>, Status> {
        task::task_delete_bulk(self, request).await
    }

    async fn task_retry(
        &self,
        request: Request<TaskRetryRequest>,
    ) -> Result<Response<TaskRetryResponse>, Status> {
        task::task_retry(self, request).await
    }

    async fn task_recover(
        &self,
        request: Request<TaskRecoverRequest>,
    ) -> Result<Response<TaskRecoverResponse>, Status> {
        task::task_recover(self, request).await
    }

    async fn task_list(
        &self,
        request: Request<TaskListRequest>,
    ) -> Result<Response<TaskListResponse>, Status> {
        task::task_list(self, request).await
    }

    async fn task_info(
        &self,
        request: Request<TaskInfoRequest>,
    ) -> Result<Response<TaskInfoResponse>, Status> {
        task::task_info(self, request).await
    }

    async fn task_timeline(
        &self,
        request: Request<TaskTimelineRequest>,
    ) -> Result<Response<TaskTimelineResponse>, Status> {
        task::task_timeline(self, request).await
    }

    async fn task_logs(
        &self,
        request: Request<TaskLogsRequest>,
    ) -> Result<Response<Self::TaskLogsStream>, Status> {
        task::task_logs(self, request).await
    }

    async fn task_follow(
        &self,
        request: Request<TaskFollowRequest>,
    ) -> Result<Response<Self::TaskFollowStream>, Status> {
        task::task_follow(self, request).await
    }

    async fn task_watch(
        &self,
        request: Request<TaskWatchRequest>,
    ) -> Result<Response<Self::TaskWatchStream>, Status> {
        task::task_watch(self, request).await
    }

    async fn task_timeline_follow(
        &self,
        request: Request<TaskTimelineFollowRequest>,
    ) -> Result<Response<Self::TaskTimelineFollowStream>, Status> {
        task::task_timeline_follow(self, request).await
    }

    async fn attention_list(
        &self,
        request: Request<AttentionListRequest>,
    ) -> Result<Response<AttentionListResponse>, Status> {
        attention::attention_list(self, request).await
    }

    async fn attention_get(
        &self,
        request: Request<AttentionGetRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        attention::attention_get(self, request).await
    }

    async fn attention_claim(
        &self,
        request: Request<AttentionClaimRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        attention::attention_claim(self, request).await
    }

    async fn attention_snooze(
        &self,
        request: Request<AttentionSnoozeRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        attention::attention_snooze(self, request).await
    }

    async fn attention_resolve(
        &self,
        request: Request<AttentionResolveRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        attention::attention_resolve(self, request).await
    }

    async fn attention_execute_action(
        &self,
        request: Request<AttentionExecuteActionRequest>,
    ) -> Result<Response<AttentionItem>, Status> {
        attention::attention_execute_action(self, request).await
    }

    async fn attention_follow(
        &self,
        request: Request<AttentionFollowRequest>,
    ) -> Result<Response<Self::AttentionFollowStream>, Status> {
        attention::attention_follow(self, request).await
    }

    async fn action_audit_list(
        &self,
        request: Request<ActionAuditListRequest>,
    ) -> Result<Response<ActionAuditListResponse>, Status> {
        action_audit::list(self, request).await
    }

    async fn action_audit_get(
        &self,
        request: Request<ActionAuditGetRequest>,
    ) -> Result<Response<ActionAuditRecord>, Status> {
        action_audit::get(self, request).await
    }

    async fn source_event_list(
        &self,
        request: Request<SourceEventListRequest>,
    ) -> Result<Response<SourceEventListResponse>, Status> {
        source::event_list(self, request).await
    }

    async fn source_event_get(
        &self,
        request: Request<SourceEventGetRequest>,
    ) -> Result<Response<SourceEvent>, Status> {
        source::event_get(self, request).await
    }

    async fn source_event_ingest(
        &self,
        request: Request<SourceEventIngestRequest>,
    ) -> Result<Response<SourceEventIngestResponse>, Status> {
        source::event_ingest(self, request).await
    }

    async fn source_binding_list(
        &self,
        request: Request<SourceBindingListRequest>,
    ) -> Result<Response<SourceBindingListResponse>, Status> {
        source::binding_list(self, request).await
    }

    async fn source_bind(
        &self,
        request: Request<SourceBindRequest>,
    ) -> Result<Response<SourceBinding>, Status> {
        source::bind(self, request).await
    }

    async fn source_replay(
        &self,
        request: Request<SourceReplayRequest>,
    ) -> Result<Response<SourceReplayResponse>, Status> {
        source::replay(self, request).await
    }

    async fn source_task_template_preview(
        &self,
        request: Request<SourceTaskTemplatePreviewRequest>,
    ) -> Result<Response<SourceTaskTemplatePreviewResponse>, Status> {
        source::task_template_preview(self, request).await
    }

    async fn agent_session_list(
        &self,
        request: Request<AgentSessionListRequest>,
    ) -> Result<Response<AgentSessionListResponse>, Status> {
        session::list(self, request).await
    }
    async fn agent_session_get(
        &self,
        request: Request<AgentSessionGetRequest>,
    ) -> Result<Response<AgentSessionGetResponse>, Status> {
        session::get(self, request).await
    }
    async fn agent_session_attach(
        &self,
        request: Request<AgentSessionAttachRequest>,
    ) -> Result<Response<AgentSessionAttachResponse>, Status> {
        session::attach(self, request).await
    }
    async fn agent_session_heartbeat(
        &self,
        request: Request<AgentSessionHeartbeatRequest>,
    ) -> Result<Response<AgentSessionHeartbeatResponse>, Status> {
        session::heartbeat(self, request).await
    }
    async fn agent_session_detach(
        &self,
        request: Request<AgentSessionDetachRequest>,
    ) -> Result<Response<AgentSessionDetachResponse>, Status> {
        session::detach(self, request).await
    }
    async fn agent_session_send_input(
        &self,
        request: Request<AgentSessionSendInputRequest>,
    ) -> Result<Response<AgentSessionSendInputResponse>, Status> {
        session::send_input(self, request).await
    }
    async fn agent_session_read(
        &self,
        request: Request<AgentSessionReadRequest>,
    ) -> Result<Response<Self::AgentSessionReadStream>, Status> {
        session::read(self, request).await
    }
    async fn agent_session_close(
        &self,
        request: Request<AgentSessionCloseRequest>,
    ) -> Result<Response<AgentSessionCloseResponse>, Status> {
        session::close(self, request).await
    }
    async fn agent_session_resolve_pid(
        &self,
        request: Request<AgentSessionResolvePidRequest>,
    ) -> Result<Response<AgentSessionResolvePidResponse>, Status> {
        session::resolve_pid(self, request).await
    }

    async fn handoff_generate(
        &self,
        request: Request<HandoffGenerateRequest>,
    ) -> Result<Response<HandoffSnapshotResponse>, Status> {
        handoff::handoff_generate(self, request).await
    }

    async fn handoff_get(
        &self,
        request: Request<HandoffGetRequest>,
    ) -> Result<Response<HandoffSnapshotResponse>, Status> {
        handoff::handoff_get(self, request).await
    }

    async fn resume_boundary_list(
        &self,
        request: Request<ResumeBoundaryListRequest>,
    ) -> Result<Response<ResumeBoundaryListResponse>, Status> {
        handoff::resume_boundary_list(self, request).await
    }

    async fn resume_plan(
        &self,
        request: Request<ResumePlanRequest>,
    ) -> Result<Response<ResumePlanResponse>, Status> {
        handoff::resume_plan(self, request).await
    }

    async fn resume_execute(
        &self,
        request: Request<ResumeExecuteRequest>,
    ) -> Result<Response<ResumeExecuteResponse>, Status> {
        handoff::resume_execute(self, request).await
    }

    async fn apply(
        &self,
        request: Request<ApplyRequest>,
    ) -> Result<Response<ApplyResponse>, Status> {
        resource::apply(self, request).await
    }

    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        resource::get(self, request).await
    }

    async fn describe(
        &self,
        request: Request<DescribeRequest>,
    ) -> Result<Response<DescribeResponse>, Status> {
        resource::describe(self, request).await
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        resource::delete(self, request).await
    }

    async fn store_get(
        &self,
        request: Request<StoreGetRequest>,
    ) -> Result<Response<StoreGetResponse>, Status> {
        store::store_get(self, request).await
    }

    async fn store_put(
        &self,
        request: Request<StorePutRequest>,
    ) -> Result<Response<StorePutResponse>, Status> {
        store::store_put(self, request).await
    }

    async fn store_delete(
        &self,
        request: Request<StoreDeleteRequest>,
    ) -> Result<Response<StoreDeleteResponse>, Status> {
        store::store_delete(self, request).await
    }

    async fn store_list(
        &self,
        request: Request<StoreListRequest>,
    ) -> Result<Response<StoreListResponse>, Status> {
        store::store_list(self, request).await
    }

    async fn store_prune(
        &self,
        request: Request<StorePruneRequest>,
    ) -> Result<Response<StorePruneResponse>, Status> {
        store::store_prune(self, request).await
    }

    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        system::ping(self, request).await
    }

    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        system::shutdown(self, request).await
    }

    async fn maintenance_mode(
        &self,
        request: Request<MaintenanceModeRequest>,
    ) -> Result<Response<MaintenanceModeResponse>, Status> {
        system::maintenance_mode(self, request).await
    }

    async fn config_debug(
        &self,
        request: Request<ConfigDebugRequest>,
    ) -> Result<Response<ConfigDebugResponse>, Status> {
        system::config_debug(self, request).await
    }

    async fn worker_status(
        &self,
        request: Request<WorkerStatusRequest>,
    ) -> Result<Response<WorkerStatusResponse>, Status> {
        system::worker_status(self, request).await
    }

    async fn check(
        &self,
        request: Request<CheckRequest>,
    ) -> Result<Response<CheckResponse>, Status> {
        system::check(self, request).await
    }

    async fn init(&self, request: Request<InitRequest>) -> Result<Response<InitResponse>, Status> {
        system::init(self, request).await
    }

    async fn db_status(
        &self,
        request: Request<DbStatusRequest>,
    ) -> Result<Response<DbStatusResponse>, Status> {
        system::db_status(self, request).await
    }

    async fn db_migrations_list(
        &self,
        request: Request<DbMigrationsListRequest>,
    ) -> Result<Response<DbMigrationsListResponse>, Status> {
        system::db_migrations_list(self, request).await
    }

    async fn db_vacuum(
        &self,
        request: Request<DbVacuumRequest>,
    ) -> Result<Response<DbVacuumResponse>, Status> {
        system::db_vacuum(self, request).await
    }

    async fn db_log_cleanup(
        &self,
        request: Request<DbLogCleanupRequest>,
    ) -> Result<Response<DbLogCleanupResponse>, Status> {
        system::db_log_cleanup(self, request).await
    }

    async fn manifest_validate(
        &self,
        request: Request<ManifestValidateRequest>,
    ) -> Result<Response<ManifestValidateResponse>, Status> {
        system::manifest_validate(self, request).await
    }

    async fn manifest_export(
        &self,
        request: Request<ManifestExportRequest>,
    ) -> Result<Response<ManifestExportResponse>, Status> {
        resource::manifest_export(self, request).await
    }

    async fn task_trace(
        &self,
        request: Request<TaskTraceRequest>,
    ) -> Result<Response<TaskTraceResponse>, Status> {
        task::task_trace(self, request).await
    }

    async fn secret_key_status(
        &self,
        request: Request<SecretKeyStatusRequest>,
    ) -> Result<Response<SecretKeyStatusResponse>, Status> {
        secret::secret_key_status(self, request).await
    }

    async fn secret_key_list(
        &self,
        request: Request<SecretKeyListRequest>,
    ) -> Result<Response<SecretKeyListResponse>, Status> {
        secret::secret_key_list(self, request).await
    }

    async fn secret_key_rotate(
        &self,
        request: Request<SecretKeyRotateRequest>,
    ) -> Result<Response<SecretKeyRotateResponse>, Status> {
        secret::secret_key_rotate(self, request).await
    }

    async fn secret_key_revoke(
        &self,
        request: Request<SecretKeyRevokeRequest>,
    ) -> Result<Response<SecretKeyRevokeResponse>, Status> {
        secret::secret_key_revoke(self, request).await
    }

    async fn secret_key_bootstrap(
        &self,
        request: Request<SecretKeyBootstrapRequest>,
    ) -> Result<Response<SecretKeyBootstrapResponse>, Status> {
        secret::secret_key_bootstrap(self, request).await
    }

    async fn secret_key_history(
        &self,
        request: Request<SecretKeyHistoryRequest>,
    ) -> Result<Response<SecretKeyHistoryResponse>, Status> {
        secret::secret_key_history(self, request).await
    }

    async fn agent_list(
        &self,
        request: Request<AgentListRequest>,
    ) -> Result<Response<AgentListResponse>, Status> {
        agent::agent_list(self, request).await
    }

    async fn agent_cordon(
        &self,
        request: Request<AgentCordonRequest>,
    ) -> Result<Response<AgentCordonResponse>, Status> {
        agent::agent_cordon(self, request).await
    }

    async fn agent_uncordon(
        &self,
        request: Request<AgentUncordonRequest>,
    ) -> Result<Response<AgentUncordonResponse>, Status> {
        agent::agent_uncordon(self, request).await
    }

    async fn agent_drain(
        &self,
        request: Request<AgentDrainRequest>,
    ) -> Result<Response<AgentDrainResponse>, Status> {
        agent::agent_drain(self, request).await
    }

    async fn event_cleanup(
        &self,
        request: Request<EventCleanupRequest>,
    ) -> Result<Response<EventCleanupResponse>, Status> {
        system::event_cleanup(self, request).await
    }

    async fn event_stats(
        &self,
        request: Request<EventStatsRequest>,
    ) -> Result<Response<EventStatsResponse>, Status> {
        system::event_stats(self, request).await
    }

    async fn task_events(
        &self,
        request: Request<TaskEventsRequest>,
    ) -> Result<Response<TaskEventsResponse>, Status> {
        system::task_events(self, request).await
    }

    async fn trigger_suspend(
        &self,
        request: Request<TriggerSuspendRequest>,
    ) -> Result<Response<TriggerSuspendResponse>, Status> {
        trigger::trigger_suspend(self, request).await
    }

    async fn trigger_resume(
        &self,
        request: Request<TriggerResumeRequest>,
    ) -> Result<Response<TriggerResumeResponse>, Status> {
        trigger::trigger_resume(self, request).await
    }

    async fn trigger_fire(
        &self,
        request: Request<TriggerFireRequest>,
    ) -> Result<Response<TriggerFireResponse>, Status> {
        trigger::trigger_fire(self, request).await
    }

    async fn qa_doctor(
        &self,
        request: Request<QaDoctorRequest>,
    ) -> Result<Response<QaDoctorResponse>, Status> {
        system::qa_doctor(self, request).await
    }

    async fn process_metrics_get(
        &self,
        request: Request<ProcessMetricsGetRequest>,
    ) -> Result<Response<ProcessMetricsGetResponse>, Status> {
        process_metrics::get(self, request).await
    }

    async fn process_metric_record(
        &self,
        request: Request<ProcessMetricRecordRequest>,
    ) -> Result<Response<ProcessMetricRecordResponse>, Status> {
        process_metrics::record(self, request).await
    }

    async fn process_metrics_rebuild(
        &self,
        request: Request<ProcessMetricsRebuildRequest>,
    ) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
        process_metrics::rebuild(self, request).await
    }

    async fn process_metrics_prune(
        &self,
        request: Request<ProcessMetricsPruneRequest>,
    ) -> Result<Response<ProcessMetricsMaintenanceResponse>, Status> {
        process_metrics::prune(self, request).await
    }

    async fn run_step(
        &self,
        request: Request<RunStepRequest>,
    ) -> Result<Response<RunStepResponse>, Status> {
        task::run_step(self, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_core_error_uses_not_found_status() {
        let status = map_core_error(OrchestratorError::not_found(
            "task.info",
            anyhow::anyhow!("task not found: deadbeef"),
        ));
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    #[test]
    fn map_core_error_uses_failed_precondition_for_invalid_state() {
        let status = map_core_error(OrchestratorError::invalid_state(
            "task.retry",
            anyhow::anyhow!("use --force to confirm task retry"),
        ));
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    }

    #[test]
    fn map_core_error_uses_invalid_argument_for_user_input() {
        let status = map_core_error(OrchestratorError::user_input(
            "task.start",
            anyhow::anyhow!("task_id or --latest required"),
        ));
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
}
