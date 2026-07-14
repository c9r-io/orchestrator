//! Integration coverage for the canonical action-audit gRPC query surface.

use agent_orchestrator::action_audit::{ActionAuditReservation, AsyncActionAuditRepository};
use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::{ActionAuditGetRequest, ActionAuditListRequest};

#[tokio::test]
async fn action_audit_list_and_get_are_project_scoped() {
    let harness = TestHarness::start().await;
    let repository = AsyncActionAuditRepository::new(harness.state().async_database.clone());
    repository
        .reserve(ActionAuditReservation {
            request_id: "req-integration-audit".into(),
            project_id: "project-a".into(),
            actor: Some("integration-actor".into()),
            resolved_role: Some("operator".into()),
            transport: "test".into(),
            target_type: "task".into(),
            target_id: "task-a".into(),
            action: "task.resume".into(),
            reason_code: "integration_test".into(),
            operator_reason: None,
            idempotency_key: Some("integration-retry".into()),
            expected_version: Some("1".into()),
            fencing_token: None,
            canonical_request: serde_json::json!({"expected_version": 1}),
        })
        .await
        .expect("reserve action audit");
    repository
        .complete(
            "req-integration-audit",
            "succeeded",
            None,
            Some("task"),
            Some("task-a"),
        )
        .await
        .expect("complete action audit");

    let mut client = harness.client();
    let listed = client
        .action_audit_list(ActionAuditListRequest {
            project_id: "project-a".into(),
            actor: Some("integration-actor".into()),
            status: Some("succeeded".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("list audit")
        .into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.records[0].request_id, "req-integration-audit");

    let record = client
        .action_audit_get(ActionAuditGetRequest {
            project_id: "project-a".into(),
            request_id: "req-integration-audit".into(),
        })
        .await
        .expect("get audit")
        .into_inner();
    assert_eq!(record.result_id.as_deref(), Some("task-a"));

    let error = client
        .action_audit_get(ActionAuditGetRequest {
            project_id: "project-b".into(),
            request_id: "req-integration-audit".into(),
        })
        .await
        .expect_err("cross-project get must fail");
    assert_eq!(error.code(), tonic::Code::NotFound);
}
