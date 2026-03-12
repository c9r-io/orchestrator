//! Integration tests for task lifecycle: create → start → complete, pause → resume.

mod common;

use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::*;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
async fn task_create_start_complete() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let mut client = harness.client();

        // Create task without auto-start
        let create_resp = client
            .task_create(TaskCreateRequest {
                no_start: true,
                workflow_id: Some("qa_only".into()),
                ..Default::default()
            })
            .await
            .expect("task_create failed")
            .into_inner();

        assert!(!create_resp.task_id.is_empty());
        assert_eq!(create_resp.status, "created");

        let task_id = create_resp.task_id.clone();

        // Start task via gRPC
        let start_resp = client
            .task_start(TaskStartRequest {
                task_id: Some(task_id.clone()),
                latest: false,
            })
            .await
            .expect("task_start failed")
            .into_inner();

        assert_eq!(start_resp.status, "enqueued");

        // Execute the task in-process (blocking)
        let state = harness.state().clone();
        let tid = task_id.clone();
        tokio::spawn(async move {
            let _ = agent_orchestrator::service::task::start_task_blocking(state, &tid).await;
        });

        // Poll until completed or failed
        let mut final_status = String::new();
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let info = client
                .task_info(TaskInfoRequest {
                    task_id: task_id.clone(),
                })
                .await
                .expect("task_info failed")
                .into_inner();

            if let Some(task) = &info.task {
                if matches!(task.status.as_str(), "completed" | "failed") {
                    final_status = task.status.clone();
                    break;
                }
            }
        }

        assert_eq!(
            final_status, "completed",
            "task should complete successfully"
        );
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn task_pause_resume() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let mut client = harness.client();

        // Create task
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

        // Enqueue the task
        client
            .task_start(TaskStartRequest {
                task_id: Some(task_id.clone()),
                latest: false,
            })
            .await
            .expect("task_start failed");

        // Pause immediately
        let pause_resp = client
            .task_pause(TaskPauseRequest {
                task_id: task_id.clone(),
            })
            .await
            .expect("task_pause failed")
            .into_inner();

        assert!(
            pause_resp.message.contains("paused"),
            "pause message should confirm: {}",
            pause_resp.message
        );

        // Verify paused status
        let info = client
            .task_info(TaskInfoRequest {
                task_id: task_id.clone(),
            })
            .await
            .expect("task_info failed")
            .into_inner();

        assert_eq!(
            info.task.as_ref().map(|t| t.status.as_str()),
            Some("paused"),
            "task should be paused"
        );

        // Resume
        let resume_resp = client
            .task_resume(TaskResumeRequest {
                task_id: task_id.clone(),
            })
            .await
            .expect("task_resume failed")
            .into_inner();

        assert_eq!(resume_resp.status, "enqueued");
    })
    .await
    .expect("test timed out");
}
