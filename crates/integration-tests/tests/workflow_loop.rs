//! Integration tests for workflow execution: failing steps, prehook skips, multi-cycle loops.

mod common;

use orchestrator_integration_tests::TestHarness;
use orchestrator_proto::*;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Helper: create a task, run it in-process, and return the final task info.
async fn run_task_to_completion(harness: &TestHarness, workflow_id: &str) -> TaskInfoResponse {
    harness.seed_qa_file();
    let mut client = harness.client();

    let create_resp = client
        .task_create(TaskCreateRequest {
            no_start: true,
            workflow_id: Some(workflow_id.into()),
            ..Default::default()
        })
        .await
        .expect("task_create failed")
        .into_inner();

    let task_id = create_resp.task_id;

    // Execute in-process
    let state = harness.state().clone();
    let tid = task_id.clone();
    let handle = tokio::spawn(async move {
        let _ = agent_orchestrator::service::task::start_task_blocking(state, &tid).await;
    });

    // Wait for completion
    let mut info = None;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let resp = client
            .task_info(TaskInfoRequest {
                task_id: task_id.clone(),
            })
            .await
            .expect("task_info failed")
            .into_inner();

        if let Some(task) = &resp.task {
            if matches!(task.status.as_str(), "completed" | "failed" | "cancelled") {
                info = Some(resp);
                break;
            }
        }
    }

    // Ensure the worker finishes
    let _ = handle.await;

    info.expect("task did not reach terminal state within timeout")
}

#[tokio::test]
async fn workflow_failing_step() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-failing.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;

        let info = run_task_to_completion(&harness, "qa_only").await;
        let task = info.task.expect("task missing from info");

        // A failing agent command should result in a failed task
        assert_eq!(
            task.status, "failed",
            "task with failing agent should end as failed"
        );
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn workflow_prehook_skip() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-prehook.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;

        let info = run_task_to_completion(&harness, "qa_fix_gated").await;
        let task = info.task.expect("task missing from info");

        // The workflow should complete (2 cycles, fix step gated by is_last_cycle)
        assert_eq!(
            task.status, "completed",
            "prehook-gated workflow should complete"
        );

        // Check events for prehook skip evidence in cycle 1
        let has_prehook_skip = info
            .events
            .iter()
            .any(|e| e.event_type.contains("prehook") && e.payload_json.contains("skip"));
        // The fix step should have been skipped in cycle 1 (not the last cycle)
        assert!(
            has_prehook_skip,
            "should have prehook skip event from cycle 1 for the gated step"
        );
    })
    .await
    .expect("test timed out");
}

#[tokio::test]
async fn multi_cycle_loop() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("echo-multicycle.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;

        let info = run_task_to_completion(&harness, "qa_only").await;
        let task = info.task.expect("task missing from info");

        assert_eq!(task.status, "completed", "multi-cycle task should complete");

        // Verify multiple cycles ran by checking for cycle-related events
        let cycle_events: Vec<_> = info
            .events
            .iter()
            .filter(|e| e.event_type.contains("cycle"))
            .collect();

        assert!(
            cycle_events.len() >= 2,
            "should have events from multiple cycles, got {}",
            cycle_events.len()
        );
    })
    .await
    .expect("test timed out");
}
