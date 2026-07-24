use std::sync::Arc;
use std::time::Duration;

use agent_orchestrator::attention::{AttentionCandidate, AttentionSeverity};
use agent_orchestrator::test_utils::TestState;
use orchestrator_proto::*;
use serde_json::json;
use tokio::sync::{Mutex, Notify};
use tonic::{Code, Request};

use super::action_audit::{self, ActionDescriptor};
use super::source_connection;
use super::{OrchestratorServer, action_audit as audit_rpc, attention, handoff, session};
use crate::control_plane::Role;
use crate::uds_security::UdsAuthPolicy;

struct BoundaryFixture {
    server: OrchestratorServer,
    _state: TestState,
}

impl BoundaryFixture {
    fn new(max_role: Option<Role>) -> Self {
        let mut fixture = TestState::new();
        let state = fixture.build();
        std::fs::write(
            fixture
                .temp_root()
                .join("workspace/default/docs/qa/boundary-fixture.md"),
            "# Deterministic boundary fixture\n",
        )
        .expect("write boundary QA fixture");
        let slack = orchestrator_slack_gateway::slack::SlackClient::new(
            "http://127.0.0.1:9",
            Duration::from_millis(50),
        )
        .expect("test Slack client");
        let server = OrchestratorServer::new(
            state,
            Arc::new(Notify::new()),
            None,
            max_role.map(|max_role| UdsAuthPolicy {
                max_role,
                audit_all_reads: true,
            }),
            None,
            Arc::new(slack),
            Arc::new(Mutex::new(())),
        );
        Self {
            server,
            _state: fixture,
        }
    }

    async fn seed_attention(&self, id: &str) {
        self.server
            .state
            .attention_repo
            .upsert_external_candidate(AttentionCandidate {
                id: id.into(),
                project_id: "default".into(),
                task_id: "task-boundary".into(),
                task_item_id: None,
                step_id: Some("qa".into()),
                session_id: None,
                kind: "step_failed".into(),
                severity: AttentionSeverity::Intervention,
                title: "Boundary test attention".into(),
                summary: "Safe fixture".into(),
                requested_decision: None,
                actions: Vec::new(),
                dedupe_key: format!("boundary:{id}"),
                source_event_id: format!("event:{id}"),
                source_route_id: None,
                source_binding_name: None,
                occurred_at: "2026-07-25T00:00:00Z".into(),
                sla_deadline: None,
            })
            .await
            .expect("seed attention");
    }

    fn seed_task(&self) -> String {
        orchestrator_scheduler::service::task::create_task(
            &self.server.state,
            agent_orchestrator::dto::CreateTaskPayload {
                name: Some("Boundary task".into()),
                goal: Some("Exercise the real daemon adapter".into()),
                project_id: Some("default".into()),
                workspace_id: Some("default".into()),
                workflow_id: Some("basic".into()),
                target_files: None,
                parent_task_id: None,
                spawn_reason: None,
                step_filter: None,
                initial_vars: None,
            },
        )
        .expect("seed task")
        .id
    }
}

fn claim(id: &str, expected_version: i64, key: &str) -> AttentionClaimRequest {
    AttentionClaimRequest {
        id: id.into(),
        expected_version,
        idempotency_key: key.into(),
        audit: None,
    }
}

#[tokio::test]
async fn attention_rpc_matrix_covers_success_invalid_denied_and_stale() {
    let fixture = BoundaryFixture::new(None);
    fixture.seed_attention("attention-success").await;
    let claimed = attention::attention_claim(
        &fixture.server,
        Request::new(claim("attention-success", 1, "claim-success")),
    )
    .await
    .expect("claim succeeds")
    .into_inner();
    assert_eq!(claimed.state, "claimed");
    assert_eq!(claimed.version, 2);

    let stale = attention::attention_claim(
        &fixture.server,
        Request::new(claim("attention-success", 1, "claim-stale")),
    )
    .await
    .expect_err("stale claim");
    assert_eq!(stale.code(), Code::Aborted);

    fixture.seed_attention("attention-invalid").await;
    let invalid = attention::attention_snooze(
        &fixture.server,
        Request::new(AttentionSnoozeRequest {
            id: "attention-invalid".into(),
            expected_version: 1,
            until: "not-rfc3339".into(),
            idempotency_key: "snooze-invalid".into(),
            audit: None,
        }),
    )
    .await
    .expect_err("invalid snooze");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let denied_fixture = BoundaryFixture::new(Some(Role::ReadOnly));
    denied_fixture.seed_attention("attention-denied").await;
    let denied = attention::attention_claim(
        &denied_fixture.server,
        Request::new(claim("attention-denied", 1, "claim-denied")),
    )
    .await
    .expect_err("read-only role is denied");
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert!(denied.metadata().get("x-request-id").is_some());
}

#[tokio::test]
async fn handoff_rpc_matrix_covers_success_invalid_and_denied() {
    let fixture = BoundaryFixture::new(None);
    let task_id = fixture.seed_task();
    let generated = handoff::handoff_generate(
        &fixture.server,
        Request::new(HandoffGenerateRequest {
            task_id: task_id.clone(),
            source_event_cursor: None,
            audit: None,
        }),
    )
    .await
    .expect("handoff succeeds")
    .into_inner();
    assert_eq!(generated.task_id, task_id);
    assert!(!generated.content_hash.is_empty());

    let invalid = handoff::handoff_generate(
        &fixture.server,
        Request::new(HandoffGenerateRequest {
            task_id,
            source_event_cursor: Some(999),
            audit: None,
        }),
    )
    .await
    .expect_err("future cursor is invalid");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let denied_fixture = BoundaryFixture::new(Some(Role::ReadOnly));
    let denied_task = denied_fixture.seed_task();
    let denied = handoff::handoff_generate(
        &denied_fixture.server,
        Request::new(HandoffGenerateRequest {
            task_id: denied_task,
            source_event_cursor: None,
            audit: None,
        }),
    )
    .await
    .expect_err("read-only role is denied");
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn session_rpc_matrix_covers_success_invalid_and_policy_denial() {
    let fixture = BoundaryFixture::new(None);
    let listed = session::list(
        &fixture.server,
        Request::new(AgentSessionListRequest::default()),
    )
    .await
    .expect("session list succeeds")
    .into_inner();
    assert!(listed.sessions.is_empty());

    let invalid = session::attach(
        &fixture.server,
        Request::new(AgentSessionAttachRequest {
            session_id: "session-missing".into(),
            client_id: "client-a".into(),
            mode: "invalid-mode".into(),
            audit: None,
        }),
    )
    .await
    .expect_err("invalid attach mode");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let denied = session::send_input(
        &fixture.server,
        Request::new(AgentSessionSendInputRequest {
            session_id: "session-missing".into(),
            client_id: "client-a".into(),
            fencing_token: 1,
            input: b"bounded".to_vec(),
            idempotency_key: "session-denied".into(),
            audit: None,
        }),
    )
    .await
    .expect_err("control policy denies mutation");
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn source_connection_rpc_matrix_covers_success_invalid_denied_and_unavailable() {
    let fixture = BoundaryFixture::new(None);
    let listed = source_connection::list(
        &fixture.server,
        Request::new(SourceConnectionListRequest {
            project_id: "default".into(),
            limit: 10,
            ..Default::default()
        }),
    )
    .await
    .expect("connection list succeeds")
    .into_inner();
    assert!(listed.connections.is_empty());

    let invalid = source_connection::list(
        &fixture.server,
        Request::new(SourceConnectionListRequest {
            project_id: " ".into(),
            ..Default::default()
        }),
    )
    .await
    .expect_err("blank project is invalid");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let connect_request = || SourceConnectionConnectRequest {
        project_id: "default".into(),
        provider: "slack".into(),
        provisioning_mode: "managed_shared".into(),
        display_label: "Boundary workspace".into(),
        idempotency_key: "connection-boundary".into(),
        reason: "exercise boundary contract".into(),
    };
    let unavailable = source_connection::connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect_err("missing Gateway fails closed");
    assert_eq!(unavailable.code(), Code::FailedPrecondition);

    let denied_fixture = BoundaryFixture::new(Some(Role::ReadOnly));
    let denied =
        source_connection::connect(&denied_fixture.server, Request::new(connect_request()))
            .await
            .expect_err("read-only role is denied");
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn action_audit_rpc_matrix_covers_success_invalid_denied_and_idempotency_conflict() {
    let fixture = BoundaryFixture::new(None);
    let listed = audit_rpc::list(
        &fixture.server,
        Request::new(ActionAuditListRequest {
            project_id: "default".into(),
            limit: 10,
            ..Default::default()
        }),
    )
    .await
    .expect("audit list succeeds")
    .into_inner();
    assert!(listed.records.is_empty());

    let invalid = audit_rpc::get(
        &fixture.server,
        Request::new(ActionAuditGetRequest {
            project_id: "default".into(),
            request_id: String::new(),
        }),
    )
    .await
    .expect_err("empty request id is invalid");
    assert_eq!(invalid.code(), Code::InvalidArgument);

    let denied_fixture = BoundaryFixture::new(Some(Role::ReadOnly));
    let mut denied_request = Request::new(());
    let denied = match action_audit::begin(
        &denied_fixture.server,
        &mut denied_request,
        "AttentionClaim",
        None,
        ActionDescriptor {
            project_id: "default",
            target_type: "attention_item",
            target_id: "attention-denied",
            action: "attention.claim",
            expected_version: Some("1".into()),
            fencing_token: None,
            canonical_request: json!({"expected_version":1}),
            fallback_reason_code: "boundary_test",
            fallback_operator_reason: None,
            fallback_idempotency_key: Some("denied-key"),
            renewable_exemption: false,
        },
    )
    .await
    {
        Ok(_) => panic!("denied action unexpectedly reserved"),
        Err(status) => status,
    };
    assert_eq!(denied.code(), Code::PermissionDenied);

    let context = ActionAuditContext {
        reason_code: "boundary_test".into(),
        operator_reason: Some("verify idempotency conflict".into()),
        idempotency_key: Some("same-key".into()),
    };
    let descriptor = |value| ActionDescriptor {
        project_id: "default",
        target_type: "task",
        target_id: "task-boundary",
        action: "task.boundary_test",
        expected_version: None,
        fencing_token: None,
        canonical_request: json!({"value":value}),
        fallback_reason_code: "boundary_test",
        fallback_operator_reason: None,
        fallback_idempotency_key: None,
        renewable_exemption: false,
    };
    let mut first_request = Request::new(());
    let first = action_audit::begin(
        &fixture.server,
        &mut first_request,
        "TaskPause",
        Some(&context),
        descriptor(1),
    )
    .await
    .expect("first reservation");
    first
        .succeeded(&fixture.server, Some("task"), Some("task-boundary"))
        .await
        .expect("complete first reservation");

    let mut conflict_request = Request::new(());
    let conflict = match action_audit::begin(
        &fixture.server,
        &mut conflict_request,
        "TaskPause",
        Some(&context),
        descriptor(2),
    )
    .await
    {
        Ok(_) => panic!("conflicting action unexpectedly reserved"),
        Err(status) => status,
    };
    assert_eq!(conflict.code(), Code::AlreadyExists);
    assert!(conflict.metadata().get("x-request-id").is_some());
}
