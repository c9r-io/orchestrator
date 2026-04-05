//! Integration tests for the trigger firing chain.
//!
//! Verifies that the canonical trigger fire path enforces all engine semantics
//! (suspend, throttle, concurrency, goal construction, trigger-state tracking),
//! that webhook / gRPC fires create exactly one task, and that cross-project
//! scoping prevents trigger leakage.

mod common;

use agent_orchestrator::config::DEFAULT_PROJECT_ID;
use agent_orchestrator::trigger_engine::{
    TriggerEventPayload, broadcast_task_event, fire_trigger_canonical,
};
use orchestrator_integration_tests::TestHarness;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Load trigger config from the active config snapshot by name.
fn load_trigger_cfg(
    state: &agent_orchestrator::state::InnerState,
    project: &str,
    trigger_name: &str,
) -> agent_orchestrator::config::TriggerConfig {
    let snap = state.config_runtime.load();
    snap.active_config
        .config
        .projects
        .get(project)
        .and_then(|p| p.triggers.get(trigger_name))
        .unwrap_or_else(|| panic!("trigger '{}' not found in project '{}'", trigger_name, project))
        .clone()
}

/// Count tasks in the DB whose name matches the trigger task naming convention.
async fn count_trigger_tasks(
    state: &agent_orchestrator::state::InnerState,
    trigger_name: &str,
) -> usize {
    let pattern = format!("trigger-{trigger_name}");
    state
        .async_database
        .reader()
        .call(move |conn| {
            let count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM tasks WHERE name = ?1",
                    rusqlite::params![pattern],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            Ok(count)
        })
        .await
        .unwrap_or(0)
}

/// Read the trigger_state row for a given trigger.
async fn read_trigger_state(
    state: &agent_orchestrator::state::InnerState,
    trigger_name: &str,
    project: &str,
) -> Option<(String, i64)> {
    let name = trigger_name.to_string();
    let proj = project.to_string();
    state
        .async_database
        .reader()
        .call(move |conn| {
            let row = conn
                .query_row(
                    "SELECT last_task_id, fire_count FROM trigger_state WHERE trigger_name = ?1 AND project = ?2",
                    rusqlite::params![name, proj],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .ok();
            Ok(row)
        })
        .await
        .ok()
        .flatten()
}

/// Read the goal of a task by ID.
async fn read_task_goal(
    state: &agent_orchestrator::state::InnerState,
    task_id: &str,
) -> Option<String> {
    let tid = task_id.to_string();
    state
        .async_database
        .reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT goal FROM tasks WHERE id = ?1",
                    rusqlite::params![tid],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .await
        .ok()
        .flatten()
}

/// Read the status of a task by ID.
async fn read_task_status(
    state: &agent_orchestrator::state::InnerState,
    task_id: &str,
) -> Option<String> {
    let tid = task_id.to_string();
    state
        .async_database
        .reader()
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT status FROM tasks WHERE id = ?1",
                    rusqlite::params![tid],
                    |row| row.get::<_, String>(0),
                )
                .ok())
        })
        .await
        .ok()
        .flatten()
}

// ── Canonical fire creates exactly one task ─────────────────────────────────

#[tokio::test]
async fn canonical_fire_creates_single_task() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "webhook-trigger");
        let task_id = fire_trigger_canonical(
            state,
            "webhook-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("fire_trigger_canonical should succeed");

        assert!(!task_id.is_empty(), "task_id should be non-empty");
        assert_eq!(
            count_trigger_tasks(state, "webhook-trigger").await,
            1,
            "exactly one task should exist"
        );
    })
    .await
    .expect("test timed out");
}

// ── Webhook payload included in goal ────────────────────────────────────────

#[tokio::test]
async fn fire_with_payload_populates_goal() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let payload = json!({"ref": "refs/heads/main", "action": "push"});
        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "webhook-trigger");
        let task_id = fire_trigger_canonical(
            state,
            "webhook-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            Some(&payload),
        )
        .await
        .expect("fire should succeed");

        let goal = read_task_goal(state, &task_id)
            .await
            .expect("task should have a goal");
        assert!(
            goal.contains("webhook-trigger"),
            "goal should reference the trigger name: {goal}"
        );
        assert!(
            goal.contains("refs/heads/main"),
            "goal should include payload content: {goal}"
        );
    })
    .await
    .expect("test timed out");
}

// ── Trigger state tracking ──────────────────────────────────────────────────

#[tokio::test]
async fn fire_updates_trigger_state() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        // No state before first fire.
        assert!(
            read_trigger_state(state, "webhook-trigger", DEFAULT_PROJECT_ID)
                .await
                .is_none(),
            "trigger_state should be empty before first fire"
        );

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "webhook-trigger");
        let task_id = fire_trigger_canonical(
            state,
            "webhook-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("fire should succeed");

        let (last_task_id, fire_count) =
            read_trigger_state(state, "webhook-trigger", DEFAULT_PROJECT_ID)
                .await
                .expect("trigger_state should exist after fire");
        assert_eq!(last_task_id, task_id);
        assert_eq!(fire_count, 1);

        // Second fire should increment count.
        let task_id_2 = fire_trigger_canonical(
            state,
            "webhook-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("second fire should succeed");

        let (last_task_id, fire_count) =
            read_trigger_state(state, "webhook-trigger", DEFAULT_PROJECT_ID)
                .await
                .expect("trigger_state should exist");
        assert_eq!(last_task_id, task_id_2);
        assert_eq!(fire_count, 2);
    })
    .await
    .expect("test timed out");
}

// ── Suspend blocks fire ─────────────────────────────────────────────────────

#[tokio::test]
async fn suspended_trigger_is_rejected() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "suspended-trigger");
        let result = fire_trigger_canonical(
            state,
            "suspended-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await;

        assert!(result.is_err(), "suspended trigger should return error");
        assert!(
            result.unwrap_err().to_string().contains("suspended"),
            "error should mention suspension"
        );
        assert_eq!(
            count_trigger_tasks(state, "suspended-trigger").await,
            0,
            "no task should be created for suspended trigger"
        );
    })
    .await
    .expect("test timed out");
}

// ── Throttle blocks rapid re-fire ───────────────────────────────────────────

#[tokio::test]
async fn throttle_blocks_second_fire() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "throttled-trigger");

        // First fire succeeds.
        fire_trigger_canonical(
            state,
            "throttled-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("first fire should succeed");

        // Immediate second fire should be throttled (minInterval = 3600s).
        let result = fire_trigger_canonical(
            state,
            "throttled-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await;

        assert!(result.is_err(), "throttled fire should return error");
        assert!(
            result.unwrap_err().to_string().contains("throttled"),
            "error should mention throttling"
        );
        assert_eq!(
            count_trigger_tasks(state, "throttled-trigger").await,
            1,
            "only the first task should exist"
        );
    })
    .await
    .expect("test timed out");
}

// ── Concurrency Forbid blocks when active task exists ───────────────────────

#[tokio::test]
async fn concurrency_forbid_blocks_second_fire() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "forbid-trigger");

        // First fire succeeds — task is in "created" status (an active state).
        let task_id = fire_trigger_canonical(
            state,
            "forbid-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("first fire should succeed");

        let status = read_task_status(state, &task_id)
            .await
            .expect("task should exist");
        assert_eq!(status, "created", "task should be in 'created' state");

        // Second fire should be blocked by Forbid policy.
        let result = fire_trigger_canonical(
            state,
            "forbid-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await;

        assert!(result.is_err(), "forbid policy should block second fire");
        assert!(
            result.unwrap_err().to_string().contains("Forbid"),
            "error should mention Forbid policy"
        );
        assert_eq!(
            count_trigger_tasks(state, "forbid-trigger").await,
            1,
            "only one task should exist"
        );
    })
    .await
    .expect("test timed out");
}

// ── Concurrency Forbid allows after previous task completes ─────────────────

#[tokio::test]
async fn concurrency_forbid_allows_after_completion() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "forbid-trigger");

        let task_id = fire_trigger_canonical(
            state,
            "forbid-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("first fire should succeed");

        // Mark the task as completed so the Forbid check passes.
        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set_task_status should succeed");

        // Second fire should succeed now.
        let task_id_2 = fire_trigger_canonical(
            state,
            "forbid-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            None,
        )
        .await
        .expect("second fire should succeed after completion");

        assert_ne!(task_id, task_id_2, "should create a new task");
        assert_eq!(
            count_trigger_tasks(state, "forbid-trigger").await,
            2,
            "both tasks should exist"
        );
    })
    .await
    .expect("test timed out");
}

// ── Cross-project isolation via broadcast ───────────────────────────────────

#[tokio::test]
async fn cross_project_broadcast_does_not_fire_other_project() {
    timeout(TEST_TIMEOUT, async {
        // Set up two projects, each with a webhook trigger.
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        // Apply a second project with its own trigger + workspace + workflow.
        let project_b_manifest = format!(
            r#"
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: project-b
spec:
  description: "second project for cross-project test"
---
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: ws-b
  project: project-b
spec:
  root_path: "{root}"
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: echo
  project: project-b
spec:
  capabilities:
    - qa
  command: "echo '{{\"confidence\":0.95,\"quality_score\":0.9,\"artifacts\":[{{\"kind\":\"analysis\",\"findings\":[{{\"title\":\"pass\",\"description\":\"ok\",\"severity\":\"info\"}}]}}]}}'"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: qa_only
  project: project-b
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
  loop:
    mode: once
---
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: webhook-trigger-b
  project: project-b
spec:
  event:
    source: webhook
  action:
    workflow: qa_only
    workspace: ws-b
    start: false
  concurrencyPolicy: Allow
"#,
            root = state.data_dir.join("workspace/default").display()
        );
        agent_orchestrator::service::resource::apply_manifests(
            state,
            &project_b_manifest,
            false,
            None,
            false,
        )
        .expect("apply project-b manifest");

        // Start a TriggerEngine so broadcast events get processed.
        let (engine, handle) = agent_orchestrator::trigger_engine::TriggerEngine::new(state.clone());
        {
            let mut guard = state.trigger_engine_handle.lock().unwrap();
            *guard = Some(handle.clone());
        }
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let engine_task = tokio::spawn(async move { engine.run(shutdown_rx).await });

        // Reload twice to stabilize triggers.
        handle.reload().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.reload().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Broadcast a webhook event scoped to DEFAULT_PROJECT_ID only.
        broadcast_task_event(
            state,
            TriggerEventPayload {
                event_type: "webhook".to_string(),
                task_id: String::new(),
                payload: Some(json!({"test": true})),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                exclude_trigger: None,
            },
        );

        // Wait for engine to process.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Default project triggers should have fired.
        let default_count = count_trigger_tasks(state, "webhook-trigger").await;
        assert!(
            default_count >= 1,
            "default project trigger should have fired, got {default_count}"
        );

        // Project-B trigger should NOT have fired.
        let proj_b_count = count_trigger_tasks(state, "webhook-trigger-b").await;
        assert_eq!(
            proj_b_count, 0,
            "project-b trigger should not fire for default project broadcast"
        );

        // Clean up engine.
        let _ = shutdown_tx.send(true);
        let _ = engine_task.await;
    })
    .await
    .expect("test timed out");
}

// ── exclude_trigger prevents duplicate task creation ────────────────────────

#[tokio::test]
async fn exclude_trigger_prevents_duplicate_via_broadcast() {
    timeout(TEST_TIMEOUT, async {
        let manifest = common::load_manifest("trigger-basic.yaml");
        let harness = TestHarness::start_with_manifest(&manifest).await;
        harness.seed_qa_file();
        let state = harness.state();

        // Start engine and stabilize.
        let (engine, handle) = agent_orchestrator::trigger_engine::TriggerEngine::new(state.clone());
        {
            let mut guard = state.trigger_engine_handle.lock().unwrap();
            *guard = Some(handle.clone());
        }
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let engine_task = tokio::spawn(async move { engine.run(shutdown_rx).await });
        handle.reload().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.reload().await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Directly fire the trigger (simulates canonical fire in webhook handler).
        let trigger_cfg = load_trigger_cfg(state, DEFAULT_PROJECT_ID, "webhook-trigger");
        let task_id = fire_trigger_canonical(
            state,
            "webhook-trigger",
            DEFAULT_PROJECT_ID,
            &trigger_cfg,
            Some(&json!({"direct": true})),
        )
        .await
        .expect("direct canonical fire should succeed");

        // Then broadcast with exclude_trigger set (simulates webhook handler).
        broadcast_task_event(
            state,
            TriggerEventPayload {
                event_type: "webhook".to_string(),
                task_id: String::new(),
                payload: Some(json!({"direct": true})),
                project: Some(DEFAULT_PROJECT_ID.to_string()),
                exclude_trigger: Some((
                    "webhook-trigger".to_string(),
                    DEFAULT_PROJECT_ID.to_string(),
                )),
            },
        );

        // Wait for engine to process the broadcast.
        tokio::time::sleep(Duration::from_millis(300)).await;

        // webhook-trigger should have exactly 1 task (the direct fire), not 2.
        // Note: the broadcast may fire OTHER webhook triggers (throttled-trigger,
        // forbid-trigger etc.), but the excluded webhook-trigger itself must not
        // be fired a second time by the engine.
        let tasks: Vec<String> = {
            let pattern = "trigger-webhook-trigger".to_string();
            state
                .async_database
                .reader()
                .call(move |conn| {
                    let mut stmt = conn
                        .prepare("SELECT id FROM tasks WHERE name = ?1")
                        .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?;
                    let rows: Vec<String> = stmt
                        .query_map(rusqlite::params![pattern], |row: &rusqlite::Row| {
                            row.get::<_, String>(0)
                        })
                        .map_err(|e| tokio_rusqlite::Error::Other(Box::new(e)))?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok(rows)
                })
                .await
                .unwrap_or_default()
        };

        assert_eq!(
            tasks.len(),
            1,
            "webhook-trigger should have exactly 1 task (direct fire); got {} — IDs: {:?}",
            tasks.len(),
            tasks
        );
        assert_eq!(tasks[0], task_id, "the single task should be the directly fired one");

        let _ = shutdown_tx.send(true);
        let _ = engine_task.await;
    })
    .await
    .expect("test timed out");
}
