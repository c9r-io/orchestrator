//! Integration tests for gRPC protocol compatibility: round-trip validation.

mod common;

use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::*;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn ping_roundtrip() {
    timeout(TEST_TIMEOUT, async {
        let harness = TestHarness::start().await;
        let mut client = harness.client();

        let resp = client
            .ping(PingRequest {})
            .await
            .expect("ping failed")
            .into_inner();

        assert!(!resp.version.is_empty(), "version should be set");
        assert!(
            !resp.lifecycle_state.is_empty(),
            "lifecycle_state should be set"
        );
        assert!(!resp.shutdown_requested);
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn task_crud_roundtrip() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let mut client = harness.client();

        // Create
        let create_resp = client
            .task_create(TaskCreateRequest {
                no_start: true,
                workflow_id: Some("qa_only".into()),
                ..Default::default()
            })
            .await
            .expect("task_create failed")
            .into_inner();

        let task_id = create_resp.task_id;
        assert!(!task_id.is_empty());

        // List — should contain the task
        let list_resp = client
            .task_list(TaskListRequest {
                status_filter: None,
                project_filter: None,
            })
            .await
            .expect("task_list failed")
            .into_inner();

        assert!(
            list_resp.tasks.iter().any(|t| t.id == task_id),
            "created task should appear in list"
        );

        // Info
        let info_resp = client
            .task_info(TaskInfoRequest {
                task_id: task_id.clone(),
            })
            .await
            .expect("task_info failed")
            .into_inner();

        assert_eq!(
            info_resp.task.as_ref().map(|t| t.id.as_str()),
            Some(task_id.as_str())
        );

        // Delete
        client
            .task_delete(TaskDeleteRequest {
                task_id: task_id.clone(),
                force: true,
            })
            .await
            .expect("task_delete failed");

        // List again — task should be gone
        let list_resp = client
            .task_list(TaskListRequest {
                status_filter: None,
                project_filter: None,
            })
            .await
            .expect("task_list after delete failed")
            .into_inner();

        assert!(
            !list_resp.tasks.iter().any(|t| t.id == task_id),
            "deleted task should not appear in list"
        );
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn task_delete_bulk_roundtrip() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let mut client = harness.client();

        // Create 3 tasks
        let mut task_ids = Vec::new();
        for _ in 0..3 {
            let resp = client
                .task_create(TaskCreateRequest {
                    no_start: true,
                    workflow_id: Some("qa_only".into()),
                    ..Default::default()
                })
                .await
                .expect("task_create failed")
                .into_inner();
            task_ids.push(resp.task_id);
        }

        // Verify all 3 exist
        let list_resp = client
            .task_list(TaskListRequest {
                status_filter: None,
                project_filter: None,
            })
            .await
            .expect("task_list failed")
            .into_inner();
        assert!(list_resp.tasks.len() >= 3);

        // Bulk delete all 3
        let bulk_resp = client
            .task_delete_bulk(TaskDeleteBulkRequest {
                task_ids: task_ids.clone(),
                force: true,
                status_filter: String::new(),
                project_filter: String::new(),
            })
            .await
            .expect("task_delete_bulk failed")
            .into_inner();

        assert_eq!(bulk_resp.deleted, 3);
        assert_eq!(bulk_resp.failed, 0);
        assert!(bulk_resp.errors.is_empty());

        // Verify all gone
        let list_resp = client
            .task_list(TaskListRequest {
                status_filter: None,
                project_filter: None,
            })
            .await
            .expect("task_list after bulk delete failed")
            .into_inner();

        for id in &task_ids {
            assert!(
                !list_resp.tasks.iter().any(|t| &t.id == id),
                "deleted task {id} should not appear in list"
            );
        }
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn apply_get_describe_roundtrip() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        let mut client = harness.client();

        // Get named agent (kind/name format — kinds are lowercase)
        let get_resp = client
            .get(GetRequest {
                resource: "agent/echo".into(),
                selector: None,
                output_format: "yaml".into(),
                project: None,
            })
            .await
            .expect("get failed")
            .into_inner();

        assert!(
            get_resp.content.contains("echo"),
            "get response should contain agent name"
        );

        // Describe agent
        let describe_resp = client
            .describe(DescribeRequest {
                resource: "agent/echo".into(),
                output_format: "yaml".into(),
                project: None,
            })
            .await
            .expect("describe failed")
            .into_inner();

        assert!(
            describe_resp.content.contains("echo"),
            "describe response should contain agent details"
        );
    })
    .await
    .expect("test timed out");
}
