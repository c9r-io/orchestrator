use agent_orchestrator::action_audit::{
    ActionAuditFilter, ActionAuditRecord as CoreActionAuditRecord, ActionAuditReservation,
    AsyncActionAuditRepository,
};
use agent_orchestrator::config_ext::OrchestratorConfigExt as _;
use orchestrator_proto::{
    ActionAuditContext, ActionAuditGetRequest, ActionAuditListRequest, ActionAuditListResponse,
    ActionAuditRecord,
};
use serde_json::Value;
use tonic::metadata::MetadataValue;
use tonic::{Code, Request, Response, Status};
use uuid::Uuid;

use super::{OrchestratorServer, authorize, trusted_actor};
use crate::control_plane::{ActionRequestId, Role};

const REQUEST_ID_HEADER: &str = "x-request-id";

pub(crate) struct ActionDescriptor<'a> {
    pub project_id: &'a str,
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub action: &'a str,
    pub expected_version: Option<String>,
    pub fencing_token: Option<i64>,
    pub canonical_request: Value,
    pub fallback_reason_code: &'a str,
    pub fallback_operator_reason: Option<&'a str>,
    pub fallback_idempotency_key: Option<&'a str>,
    pub renewable_exemption: bool,
}

pub(crate) struct ActionAttempt {
    pub request_id: String,
    pub should_execute: bool,
    pub idempotency_key: Option<String>,
}

impl ActionAttempt {
    pub async fn succeeded(
        &self,
        server: &OrchestratorServer,
        result_type: Option<&str>,
        result_id: Option<&str>,
    ) -> Result<(), Status> {
        AsyncActionAuditRepository::new(server.state.async_database.clone())
            .complete(&self.request_id, "succeeded", None, result_type, result_id)
            .await
            .map(|_| ())
            .map_err(|error| self.status(Status::internal(error.to_string())))
    }

    pub async fn failed(&self, server: &OrchestratorServer, status: Status) -> Status {
        let code = stable_error_code(&status);
        let _ = AsyncActionAuditRepository::new(server.state.async_database.clone())
            .complete(&self.request_id, "failed", Some(code), None, None)
            .await;
        self.status(status)
    }

    pub fn response<T>(&self, value: T) -> Response<T> {
        let mut response = Response::new(value);
        if let Ok(value) = MetadataValue::try_from(self.request_id.as_str()) {
            response.metadata_mut().insert(REQUEST_ID_HEADER, value);
        }
        response
    }

    pub fn status(&self, mut status: Status) -> Status {
        if let Ok(value) = MetadataValue::try_from(self.request_id.as_str()) {
            status.metadata_mut().insert(REQUEST_ID_HEADER, value);
        }
        status
    }
}

pub(crate) async fn begin<T>(
    server: &OrchestratorServer,
    request: &mut Request<T>,
    rpc: &'static str,
    context: Option<&ActionAuditContext>,
    descriptor: ActionDescriptor<'_>,
) -> Result<ActionAttempt, Status> {
    let request_id = install_request_id(request)?;
    let actor = trusted_actor(request);
    let role = trusted_role(server, request);
    let transport = if server.control_plane.is_some() {
        "tcp"
    } else {
        "uds"
    };
    let mode = agent_orchestrator::config_load::read_active_config(&server.state)
        .map(|active| {
            active
                .config
                .runtime_policy_for_project(descriptor.project_id)
                .action_audit_mode
        })
        .unwrap_or_else(|_| "compatibility".to_string());
    if let Err(error) = authorize(server, request, rpc) {
        let resolved = resolve_context(&request_id, context, &descriptor, "compatibility")
            .unwrap_or_else(|_| ResolvedContext {
                reason_code: "authorization_attempt".to_string(),
                operator_reason: None,
                idempotency_key: None,
            });
        let repository = AsyncActionAuditRepository::new(server.state.async_database.clone());
        let _ = repository
            .deny(
                ActionAuditReservation {
                    request_id: request_id.clone(),
                    project_id: descriptor.project_id.to_string(),
                    actor: Some(actor),
                    resolved_role: role,
                    transport: transport.to_string(),
                    target_type: descriptor.target_type.to_string(),
                    target_id: descriptor.target_id.to_string(),
                    action: descriptor.action.to_string(),
                    reason_code: resolved.reason_code,
                    operator_reason: resolved.operator_reason,
                    idempotency_key: resolved.idempotency_key,
                    expected_version: descriptor.expected_version,
                    fencing_token: descriptor.fencing_token,
                    canonical_request: descriptor.canonical_request,
                },
                "authorization_denied",
            )
            .await;
        return Err(status_with_request_id(Status::from(error), &request_id));
    }

    let resolved = resolve_context(&request_id, context, &descriptor, &mode)
        .map_err(|status| status_with_request_id(status, &request_id))?;

    let repository = AsyncActionAuditRepository::new(server.state.async_database.clone());
    let input = ActionAuditReservation {
        request_id: request_id.clone(),
        project_id: descriptor.project_id.to_string(),
        actor: Some(actor),
        resolved_role: role,
        transport: transport.to_string(),
        target_type: descriptor.target_type.to_string(),
        target_id: descriptor.target_id.to_string(),
        action: descriptor.action.to_string(),
        reason_code: resolved.reason_code,
        operator_reason: resolved.operator_reason,
        idempotency_key: resolved.idempotency_key,
        expected_version: descriptor.expected_version,
        fencing_token: descriptor.fencing_token,
        canonical_request: descriptor.canonical_request,
    };
    let reservation = match repository.reserve(input.clone()).await {
        Ok(reservation) => reservation,
        Err(error) => {
            let _ = repository.fail_attempt(input, "idempotency_conflict").await;
            return Err(status_with_request_id(
                Status::already_exists(error.to_string()),
                &request_id,
            ));
        }
    };
    Ok(ActionAttempt {
        request_id: reservation.record.request_id,
        should_execute: reservation.should_execute,
        idempotency_key: reservation.record.idempotency_key,
    })
}

#[derive(Debug)]
struct ResolvedContext {
    reason_code: String,
    operator_reason: Option<String>,
    idempotency_key: Option<String>,
}

fn resolve_context(
    request_id: &str,
    context: Option<&ActionAuditContext>,
    descriptor: &ActionDescriptor<'_>,
    mode: &str,
) -> Result<ResolvedContext, Status> {
    if mode == "enforced" && context.is_none() {
        return Err(Status::invalid_argument("action audit context is required"));
    }
    let reason_code = context
        .map(|value| value.reason_code.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(descriptor.fallback_reason_code);
    if mode == "enforced" && reason_code == "legacy_client" {
        return Err(Status::invalid_argument("reason_code is required"));
    }
    if reason_code.is_empty() || reason_code.len() > 64 {
        return Err(Status::invalid_argument(
            "reason_code must contain 1-64 characters",
        ));
    }
    let operator_reason = context
        .and_then(|value| value.operator_reason.clone())
        .or_else(|| descriptor.fallback_operator_reason.map(str::to_owned));
    if operator_reason
        .as_ref()
        .is_some_and(|value| value.len() > 500)
    {
        return Err(Status::invalid_argument(
            "operator_reason exceeds 500 bytes",
        ));
    }
    let contextual_key = context.and_then(|value| value.idempotency_key.clone());
    if let (Some(contextual), Some(domain)) = (
        contextual_key.as_deref(),
        descriptor.fallback_idempotency_key,
    ) && contextual != domain
    {
        return Err(Status::invalid_argument(
            "audit and domain idempotency keys differ",
        ));
    }
    let idempotency_key = contextual_key
        .or_else(|| descriptor.fallback_idempotency_key.map(str::to_owned))
        .or_else(|| {
            (mode == "compatibility" && !descriptor.renewable_exemption)
                .then(|| format!("legacy:{request_id}"))
        });
    if mode == "enforced" && !descriptor.renewable_exemption && idempotency_key.is_none() {
        return Err(Status::invalid_argument("idempotency_key is required"));
    }
    Ok(ResolvedContext {
        reason_code: reason_code.to_string(),
        operator_reason,
        idempotency_key,
    })
}

fn install_request_id<T>(request: &mut Request<T>) -> Result<String, Status> {
    let propagated = request
        .metadata()
        .get(REQUEST_ID_HEADER)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| Status::invalid_argument("x-request-id must be ASCII"))
        })
        .transpose()?;
    let request_id = propagated.unwrap_or_else(|| format!("req-{}", Uuid::new_v4()));
    if request_id.len() > 128
        || request_id.is_empty()
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(Status::invalid_argument("x-request-id has invalid format"));
    }
    request
        .extensions_mut()
        .insert(ActionRequestId(request_id.clone()));
    Ok(request_id)
}

fn trusted_role<T>(server: &OrchestratorServer, request: &Request<T>) -> Option<String> {
    server
        .control_plane
        .as_ref()
        .and_then(|control_plane| control_plane.resolved_role(request))
        .or_else(|| {
            server
                .uds_auth_policy
                .as_ref()
                .map(|policy| policy.max_role)
        })
        .or(Some(Role::Operator))
        .map(|role| role.as_str().to_string())
}

fn stable_error_code(status: &Status) -> &'static str {
    match status.code() {
        Code::Unauthenticated | Code::PermissionDenied => "policy_denied",
        Code::Aborted => "stale_or_idempotency_conflict",
        Code::FailedPrecondition => {
            if status.message().contains("fencing") || status.message().contains("lease") {
                "fencing_rejected"
            } else {
                "stale_state"
            }
        }
        Code::InvalidArgument => "invalid_argument",
        Code::Unavailable => "side_effect_unavailable",
        _ => "internal_error",
    }
}

fn status_with_request_id(mut status: Status, request_id: &str) -> Status {
    if let Ok(value) = MetadataValue::try_from(request_id) {
        status.metadata_mut().insert(REQUEST_ID_HEADER, value);
    }
    status
}

fn to_proto(record: CoreActionAuditRecord) -> ActionAuditRecord {
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

pub(crate) async fn list(
    server: &OrchestratorServer,
    request: Request<ActionAuditListRequest>,
) -> Result<Response<ActionAuditListResponse>, Status> {
    authorize(server, &request, "ActionAuditList").map_err(Status::from)?;
    let request = request.into_inner();
    if request.project_id.trim().is_empty() {
        return Err(Status::invalid_argument("project_id is required"));
    }
    let records = AsyncActionAuditRepository::new(server.state.async_database.clone())
        .list(ActionAuditFilter {
            project_id: request.project_id,
            actor: request.actor,
            target_type: request.target_type,
            target_id: request.target_id,
            action: request.action,
            status: request.status,
            from_time: request.from_time,
            to_time: request.to_time,
            limit: if request.limit == 0 {
                100
            } else {
                request.limit as usize
            },
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(ActionAuditListResponse {
        records: records.into_iter().map(to_proto).collect(),
    }))
}

pub(crate) async fn get(
    server: &OrchestratorServer,
    request: Request<ActionAuditGetRequest>,
) -> Result<Response<ActionAuditRecord>, Status> {
    authorize(server, &request, "ActionAuditGet").map_err(Status::from)?;
    let request = request.into_inner();
    if request.project_id.trim().is_empty() || request.request_id.trim().is_empty() {
        return Err(Status::invalid_argument(
            "project_id and request_id are required",
        ));
    }
    let record = AsyncActionAuditRepository::new(server.state.async_database.clone())
        .get(&request.project_id, &request.request_id)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
        .ok_or_else(|| Status::not_found("action audit record not found"))?;
    Ok(Response::new(to_proto(record)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(renewable_exemption: bool) -> ActionDescriptor<'static> {
        ActionDescriptor {
            project_id: "default",
            target_type: "session",
            target_id: "session-1",
            action: "session.heartbeat",
            expected_version: Some("3".into()),
            fencing_token: Some(7),
            canonical_request: serde_json::json!({}),
            fallback_reason_code: "lease_heartbeat",
            fallback_operator_reason: None,
            fallback_idempotency_key: None,
            renewable_exemption,
        }
    }

    #[test]
    fn enforced_context_requires_retry_identity() {
        let context = ActionAuditContext {
            reason_code: "operator_replay".into(),
            operator_reason: None,
            idempotency_key: None,
        };
        let error = resolve_context("req-1", Some(&context), &descriptor(false), "enforced")
            .expect_err("missing key");
        assert_eq!(error.code(), Code::InvalidArgument);
    }

    #[test]
    fn enforced_mode_rejects_missing_action_context() {
        let error = resolve_context("req-1", None, &descriptor(false), "enforced")
            .expect_err("missing action context");
        assert_eq!(error.code(), Code::InvalidArgument);
    }

    #[test]
    fn heartbeat_has_explicit_renewable_exemption() {
        let context = ActionAuditContext {
            reason_code: "lease_heartbeat".into(),
            operator_reason: None,
            idempotency_key: None,
        };
        let resolved = resolve_context("req-1", Some(&context), &descriptor(true), "enforced")
            .expect("renewable exemption");
        assert!(resolved.idempotency_key.is_none());
    }

    #[test]
    fn propagated_request_id_is_validated_and_installed() {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            REQUEST_ID_HEADER,
            "req-client-42".parse().expect("metadata"),
        );
        assert_eq!(
            install_request_id(&mut request).expect("request id"),
            "req-client-42"
        );
        assert_eq!(
            request
                .extensions()
                .get::<ActionRequestId>()
                .map(|value| value.0.as_str()),
            Some("req-client-42")
        );
    }

    #[test]
    fn invalid_request_id_fails_before_mutation() {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            REQUEST_ID_HEADER,
            "contains spaces".parse().expect("metadata"),
        );
        let error = install_request_id(&mut request).expect_err("invalid request id");
        assert_eq!(error.code(), Code::InvalidArgument);
    }
}
