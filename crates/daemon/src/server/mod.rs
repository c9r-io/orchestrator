mod agent;
mod mapping;
mod resource;
mod secret;
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
    match &server.control_plane {
        Some(control_plane) => control_plane.authorize(request, rpc),
        None => {
            let required = required_role_for_rpc(rpc);
            let peer = request.extensions().get::<UdsPeerInfo>();

            // Phase 4: optional UDS authorization policy
            if let Some(policy) = &server.uds_auth_policy {
                if !policy.max_role.allows(required) {
                    uds_audit(
                        &server.state.db_path,
                        rpc,
                        peer,
                        "denied",
                        Some("uds_policy_denied"),
                    );
                    return Err(AuthzError::PermissionDenied(
                        "UDS policy restricts this operation",
                    ));
                }
            }

            // Phase 3: audit mutating operations on UDS
            if required != Role::ReadOnly {
                uds_audit(&server.state.db_path, rpc, peer, "allowed", None);
            }

            Ok(())
        }
    }
}

fn uds_audit(
    db_path: &std::path::Path,
    rpc: &str,
    peer: Option<&UdsPeerInfo>,
    authz_result: &str,
    rejection_stage: Option<&str>,
) {
    use agent_orchestrator::db::{ControlPlaneAuditRecord, insert_control_plane_audit};
    let _ = insert_control_plane_audit(
        db_path,
        &ControlPlaneAuditRecord {
            transport: "uds".into(),
            remote_addr: peer.and_then(|p| p.pid.map(|pid| format!("pid:{pid}"))),
            rpc: rpc.into(),
            subject_id: peer.map(|p| format!("uid:{}", p.uid)),
            authn_result: "peer_cred".into(),
            authz_result: authz_result.into(),
            role: None,
            reason: rejection_stage.map(|s| s.to_string()),
            tls_fingerprint: None,
            rejection_stage: rejection_stage.map(|s| s.to_string()),
            traffic_class: None,
            limit_scope: None,
            decision: None,
            reason_code: None,
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
