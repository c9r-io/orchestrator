use super::super::trait_def::TaskRepository;
use super::super::types::{NewTaskGraphRun, NewTaskGraphSnapshot, TaskRepositorySource};
use super::super::SqliteTaskRepository;
use super::fixtures::seed_task;
use crate::db::open_conn;
use crate::dto::CreateTaskPayload;
use crate::task_ops::create_task_impl;
use crate::test_utils::TestState;
use rusqlite::{params, Connection};

#[test]
fn resolve_task_id_supports_exact_and_prefix() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let exact = repo
        .resolve_task_id(&task_id)
        .expect("exact id must resolve");
    assert_eq!(exact, task_id);

    let prefix = &task_id[0..8];
    let by_prefix = repo
        .resolve_task_id(prefix)
        .expect("single prefix match must resolve");
    assert_eq!(by_prefix, task_id);
}

#[test]
fn resolve_task_id_errors_for_not_found() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let err = repo
        .resolve_task_id("nonexistent")
        .expect_err("should error for nonexistent task");
    assert!(err.to_string().contains("task not found"));
}

#[test]
fn resolve_task_id_errors_for_multiple_matches() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert two tasks with IDs that share a common prefix
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, created_at, updated_at) VALUES ('prefix-aaa-1', 'task1', 'pending', 'test', '[]', 'default', 'default', 'default', '/tmp', '[]', '/tmp', '{}', 'once', 0, 0, ?1, ?1)",
        params![ts],
    )
    .expect("insert task 1");
    conn.execute(
        "INSERT INTO tasks (id, name, status, goal, target_files_json, mode, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, created_at, updated_at) VALUES ('prefix-aaa-2', 'task2', 'pending', 'test', '[]', 'default', 'default', 'default', '/tmp', '[]', '/tmp', '{}', 'once', 0, 0, ?1, ?1)",
        params![ts],
    )
    .expect("insert task 2");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let err = repo
        .resolve_task_id("prefix-aaa")
        .expect_err("should error for multiple matches");
    assert!(
        err.to_string().contains("multiple tasks match prefix"),
        "error should mention multiple matches, got: {}",
        err
    );
}

#[test]
fn load_task_summary_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let result = repo.load_task_summary("nonexistent-id");
    assert!(result.is_err());
}

#[test]
fn load_task_summary_errors_for_invalid_target_files_json() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET target_files_json = 'not-json' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("corrupt target files json");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let err = repo
        .load_task_summary(&task_id)
        .expect_err("invalid target files json should fail");
    assert!(err.to_string().contains("load task summary"));
}

// ── load_task_detail_rows ─────────────────────────────────────────────

#[test]
fn load_task_detail_rows_returns_items_runs_and_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let (items, runs, events, graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    // Seeded task should have at least one item
    assert!(!items.is_empty(), "should have task items");
    assert!(runs.is_empty(), "should have no command runs initially");
    assert!(events.is_empty(), "should have no events initially");
    assert!(
        graph_debug.is_empty(),
        "should have no graph debug initially"
    );

    // Verify item fields
    let first_item = &items[0];
    assert!(!first_item.id.is_empty());
    assert_eq!(first_item.task_id, task_id);
    assert!(!first_item.qa_file_path.is_empty());
}

#[test]
fn load_task_detail_rows_includes_command_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task item exists");

    // Insert a command run
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES ('cr-1', ?1, 'qa', 'echo test', '/tmp', 'default', 'agent', 0, '/tmp/out.log', '/tmp/err.log', '{}', '[]', NULL, NULL, 'unknown', ?2, ?2, 0)",
        params![item_id, ts],
    )
    .expect("insert command run");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (items, runs, events, graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    assert!(!items.is_empty());
    assert_eq!(runs.len(), 1, "should have one command run");
    assert!(events.is_empty());
    assert!(graph_debug.is_empty());

    let run = &runs[0];
    assert_eq!(run.id, "cr-1");
    assert_eq!(run.phase, "qa");
    assert_eq!(run.command, "echo test");
    assert!(!run.interrupted);
}

#[test]
fn load_task_detail_rows_includes_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert an event
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO events (id, task_id, task_item_id, event_type, payload_json, created_at) VALUES (1, ?1, NULL, 'status_change', '{\"from\":\"pending\",\"to\":\"running\"}', ?2)",
        params![task_id, ts],
    )
    .expect("insert event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (items, runs, events, graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    assert!(!items.is_empty());
    assert!(runs.is_empty());
    assert_eq!(events.len(), 1, "should have one event");
    assert!(graph_debug.is_empty());

    let event = &events[0];
    assert_eq!(event.event_type, "status_change");
    assert!(event.payload.is_object());
}

#[test]
fn load_task_detail_rows_returns_empty_for_unknown_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let (items, runs, events, graph_debug) = repo
        .load_task_detail_rows("nonexistent-task-id")
        .expect("should succeed even for unknown task");

    assert!(items.is_empty());
    assert!(runs.is_empty());
    assert!(events.is_empty());
    assert!(graph_debug.is_empty());
}

#[test]
fn load_task_detail_rows_includes_graph_debug_bundles() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_graph_runs (
            graph_run_id, task_id, cycle, mode, source, status, fallback_mode,
            planner_failure_class, planner_failure_message, entry_node_id,
            node_count, edge_count, created_at, updated_at
        ) VALUES ('graph-1', ?1, 2, 'dynamic_dag', 'adaptive_planner', 'completed', NULL, NULL, NULL, 'qa', 2, 1, ?2, ?2)",
        params![task_id, ts],
    )
    .expect("insert graph run");
    conn.execute(
        "INSERT INTO task_graph_snapshots (graph_run_id, task_id, snapshot_kind, payload_json, created_at)
         VALUES ('graph-1', ?1, 'effective_graph', '{\"entry\":\"qa\"}', ?2)",
        params![task_id, ts],
    )
    .expect("insert graph snapshot");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (_items, _runs, _events, graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load detail rows");

    assert_eq!(graph_debug.len(), 1);
    assert_eq!(graph_debug[0].graph_run_id, "graph-1");
    assert_eq!(graph_debug[0].cycle, 2);
    assert_eq!(graph_debug[0].source, "adaptive_planner");
    assert_eq!(graph_debug[0].effective_graph_json, "{\"entry\":\"qa\"}");
}

#[test]
fn load_task_detail_rows_falls_back_to_legacy_graph_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, NULL, 'dynamic_plan_materialized', ?2, ?3)",
        params![
            task_id,
            r#"{"cycle":3,"source":"legacy_planner","fallback_mode":"static_segment","graph":{"entry":"qa","nodes":{"qa":{"id":"qa"}}}}"#,
            ts
        ],
    )
    .expect("insert legacy graph event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (_items, _runs, _events, graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("should load legacy graph debug");

    assert_eq!(graph_debug.len(), 1);
    assert_eq!(graph_debug[0].graph_run_id, "legacy-event-1");
    assert_eq!(graph_debug[0].cycle, 3);
    assert_eq!(graph_debug[0].source, "legacy_planner");
    assert_eq!(
        graph_debug[0].fallback_mode.as_deref(),
        Some("static_segment")
    );
    assert_eq!(
        graph_debug[0].effective_graph_json,
        r#"{"entry":"qa","nodes":{"qa":{"id":"qa"}}}"#
    );
}

#[test]
fn load_task_detail_rows_tolerates_invalid_json_and_interrupted_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("task item exists");

    conn.execute(
        "UPDATE task_items
         SET ticket_files_json = 'broken-files',
             ticket_content_json = 'broken-content'
         WHERE id = ?1",
        params![item_id.clone()],
    )
    .expect("corrupt item json");
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted)
         VALUES ('cr-interrupted', ?1, 'qa', 'echo stop', '/tmp', 'default', 'agent', 130, '/tmp/out-stop.log', '/tmp/err-stop.log', '{}', '[]', NULL, NULL, 'unknown', '2026-01-01T00:00:00Z', NULL, 1)",
        params![item_id],
    )
    .expect("insert interrupted run");
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, NULL, 'status_change', 'not-json', '2026-01-01T00:00:01Z')",
        params![task_id.clone()],
    )
    .expect("insert invalid event payload");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (items, runs, events, _graph_debug) = repo
        .load_task_detail_rows(&task_id)
        .expect("detail rows should still load");

    assert_eq!(items[0].ticket_files, Vec::<String>::new());
    assert_eq!(items[0].ticket_content, Vec::<serde_json::Value>::new());
    assert_eq!(runs.len(), 1);
    assert!(runs[0].interrupted);
    assert!(runs[0].ended_at.is_none());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload, serde_json::json!({}));
}

// ── list_task_ids_ordered_by_created_desc ─────────────────────────────

#[test]
fn list_task_ids_ordered_by_created_desc_returns_tasks() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let ids = repo
        .list_task_ids_ordered_by_created_desc()
        .expect("should list task ids");

    assert!(!ids.is_empty(), "should have at least one task");
}

#[test]
fn list_task_ids_ordered_by_created_desc_respects_order() {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let qa_file = state
        .app_root
        .join("workspace/default/docs/qa/repo_test.md");
    std::fs::write(&qa_file, "# repository test\n").expect("seed qa file");

    // Create tasks with small delays to ensure different timestamps
    let task1 = create_task_impl(
        &state,
        CreateTaskPayload {
            name: Some("task-first".to_string()),
            ..Default::default()
        },
    )
    .expect("task1 should be created");

    std::thread::sleep(std::time::Duration::from_millis(10));

    let task2 = create_task_impl(
        &state,
        CreateTaskPayload {
            name: Some("task-second".to_string()),
            ..Default::default()
        },
    )
    .expect("task2 should be created");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let ids = repo
        .list_task_ids_ordered_by_created_desc()
        .expect("should list task ids");

    // Most recent task should be first
    assert_eq!(ids.first(), Some(&task2.id));
    // Older task should come later
    assert!(ids.contains(&task1.id));
}

// ── list_task_log_runs edge cases ─────────────────────────────────────

#[test]
fn list_task_log_runs_returns_empty_when_no_runs() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let runs = repo
        .list_task_log_runs(&task_id, 10)
        .expect("should succeed");

    assert!(runs.is_empty(), "should have no runs initially");
}

#[test]
fn list_task_log_runs_respects_limit() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .expect("task item exists");

    // Insert 5 command runs
    let ts = crate::config_load::now_ts();
    for i in 0..5 {
        conn.execute(
            "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted) VALUES (?1, ?2, 'qa', 'echo test', '/tmp', 'default', 'agent', 0, '/tmp/out.log', '/tmp/err.log', '{}', '[]', NULL, NULL, 'unknown', ?3, ?3, 0)",
            params![format!("cr-limit-{}", i), item_id, ts],
        )
        .expect("insert command run");
    }

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let runs = repo
        .list_task_log_runs(&task_id, 3)
        .expect("should succeed");

    assert_eq!(runs.len(), 3, "should respect limit");
}

#[test]
fn list_task_log_runs_orders_newest_first() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let item_id: String = conn
        .query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            params![task_id.clone()],
            |row| row.get(0),
        )
        .expect("task item exists");

    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted)
         VALUES ('cr-older', ?1, 'qa', 'echo old', '/tmp', 'default', 'agent', 0, '/tmp/out-old.log', '/tmp/err-old.log', '{}', '[]', NULL, NULL, 'unknown', '2026-01-01T00:00:00Z', '2026-01-01T00:00:01Z', 0)",
        params![item_id.clone()],
    )
    .expect("insert older run");
    conn.execute(
        "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted)
         VALUES ('cr-newer', ?1, 'fix', 'echo new', '/tmp', 'default', 'agent', 0, '/tmp/out-new.log', '/tmp/err-new.log', '{}', '[]', NULL, NULL, 'unknown', '2026-01-01T00:00:02Z', '2026-01-01T00:00:03Z', 0)",
        params![item_id],
    )
    .expect("insert newer run");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let runs = repo
        .list_task_log_runs(&task_id, 10)
        .expect("should list runs");

    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].run_id, "cr-newer");
    assert_eq!(runs[1].run_id, "cr-older");
}

#[test]
fn load_task_summary_and_counts_are_consistent() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let summary = repo
        .load_task_summary(&task_id)
        .expect("summary should load from repo");
    assert_eq!(summary.id, task_id);
    assert!(!summary.created_at.is_empty());
    assert!(!summary.updated_at.is_empty());

    let (total, finished, failed) = repo
        .load_task_item_counts(&summary.id)
        .expect("item counts should load");
    assert!(total >= 1);
    assert_eq!(finished, 0);
    assert_eq!(failed, 0);
}

#[test]
fn load_task_item_counts_handles_finished_and_failed_statuses() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let ts = crate::config_load::now_ts();

    conn.execute(
        "UPDATE task_items SET status='verified' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("update seeded item");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at)
         VALUES ('item-failed-count', ?1, 20, '/tmp/qa-failed.md', 'qa_failed', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert failed item");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at)
         VALUES ('item-unresolved-count', ?1, 21, '/tmp/qa-unresolved.md', 'unresolved', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert unresolved item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let (total, finished, failed) = repo
        .load_task_item_counts(&task_id)
        .expect("item counts should load");

    assert_eq!(total, 3);
    assert_eq!(finished, 2);
    assert_eq!(failed, 2);
}

#[test]
fn graph_run_queries_insert_update_and_upsert_snapshots() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    super::super::queries::insert_task_graph_run(
        &conn,
        &NewTaskGraphRun {
            graph_run_id: "graph-upsert".to_string(),
            task_id: task_id.clone(),
            cycle: 7,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: Some("deterministic_dag".to_string()),
            planner_failure_class: Some("invalid_json".to_string()),
            planner_failure_message: Some("broken planner output".to_string()),
            entry_node_id: Some("qa".to_string()),
            node_count: 2,
            edge_count: 1,
        },
    )
    .expect("insert graph run");
    super::super::queries::update_task_graph_run_status(&conn, "graph-upsert", "completed")
        .expect("update graph run");
    super::super::queries::insert_task_graph_snapshot(
        &conn,
        &NewTaskGraphSnapshot {
            graph_run_id: "graph-upsert".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"qa\"}".to_string(),
        },
    )
    .expect("insert graph snapshot");
    super::super::queries::insert_task_graph_snapshot(
        &conn,
        &NewTaskGraphSnapshot {
            graph_run_id: "graph-upsert".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"fix\"}".to_string(),
        },
    )
    .expect("upsert graph snapshot");
    super::super::queries::insert_task_graph_snapshot(
        &conn,
        &NewTaskGraphSnapshot {
            graph_run_id: "graph-upsert".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "execution_replay".to_string(),
            payload_json: "{\"node_execution_order\":[\"qa\"]}".to_string(),
        },
    )
    .expect("insert replay snapshot");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let bundles = repo
        .load_task_graph_debug_bundles(&task_id)
        .expect("load graph bundles");

    assert_eq!(bundles.len(), 1);
    assert_eq!(bundles[0].graph_run_id, "graph-upsert");
    assert_eq!(bundles[0].status, "completed");
    assert_eq!(
        bundles[0].fallback_mode.as_deref(),
        Some("deterministic_dag")
    );
    assert_eq!(
        bundles[0].planner_failure_class.as_deref(),
        Some("invalid_json")
    );
    assert_eq!(
        bundles[0].planner_failure_message.as_deref(),
        Some("broken planner output")
    );
    assert_eq!(bundles[0].effective_graph_json, "{\"entry\":\"fix\"}");
    assert_eq!(
        bundles[0].execution_replay_json.as_deref(),
        Some("{\"node_execution_order\":[\"qa\"]}")
    );
}

#[test]
fn load_task_graph_debug_bundles_uses_defaults_for_invalid_legacy_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "INSERT INTO events (id, task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (77, ?1, NULL, 'dynamic_plan_materialized', 'not-json', '2026-01-01T00:00:00Z')",
        params![task_id.clone()],
    )
    .expect("insert invalid legacy event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let bundles = repo
        .load_task_graph_debug_bundles(&task_id)
        .expect("legacy fallback should succeed");

    assert_eq!(bundles.len(), 1);
    assert_eq!(bundles[0].graph_run_id, "legacy-event-77");
    assert_eq!(bundles[0].cycle, 0);
    assert_eq!(bundles[0].source, "unknown");
    assert!(bundles[0].fallback_mode.is_none());
    assert_eq!(bundles[0].effective_graph_json, "{}");
}

// ── find_latest_resumable_task_id ──────────────────────────────────

#[test]
fn find_latest_resumable_task_id_returns_running_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set running");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_interrupted_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='interrupted' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set interrupted");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_paused_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='paused' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set paused");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_includes_pending_when_flag_set() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    // task is created with status 'pending' by default
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    // Without include_pending, pending task should NOT be found
    let without = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(without, None);

    // With include_pending, pending task SHOULD be found
    let with = repo
        .find_latest_resumable_task_id(true)
        .expect("query should succeed");
    assert_eq!(with, Some(task_id));
}

#[test]
fn find_latest_resumable_task_id_returns_none_when_all_completed() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='completed' WHERE id = ?1",
        params![task_id],
    )
    .expect("set completed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, None);
}

#[test]
fn find_latest_resumable_task_id_returns_none_for_failed_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='failed' WHERE id = ?1",
        params![task_id],
    )
    .expect("set failed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, None);
}

#[test]
fn find_latest_resumable_task_id_includes_restart_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='restart_pending' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set restart_pending");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let found = repo
        .find_latest_resumable_task_id(false)
        .expect("query should succeed");
    assert_eq!(found, Some(task_id));
}

// ── load_task_runtime_row ──────────────────────────────────────────

#[test]
fn load_task_runtime_row_returns_expected_fields() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let row = repo
        .load_task_runtime_row(&task_id)
        .expect("runtime row should load");
    assert!(!row.workspace_id.is_empty());
    assert!(!row.workflow_id.is_empty());
    assert!(!row.workspace_root_raw.is_empty());
    assert_eq!(row.goal, "repo-test-goal");
    assert_eq!(row.current_cycle, 0);
    assert_eq!(row.init_done, 0);
}

#[test]
fn load_task_runtime_row_errors_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let result = repo.load_task_runtime_row("nonexistent-id");
    assert!(result.is_err());
}

// ── first_task_item_id ─────────────────────────────────────────────

#[test]
fn first_task_item_id_returns_item() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let item_id = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert!(item_id.is_some(), "seeded task must have at least one item");
}

#[test]
fn first_task_item_id_returns_none_for_empty_task() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "DELETE FROM task_items WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("delete items");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_id = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert_eq!(item_id, None);
}

#[test]
fn first_task_item_id_respects_order_no() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert a second item with a lower order_no so it should be returned first
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-low-order', ?1, -1, '/tmp/qa.md', 'pending', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert low order item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let first = repo
        .first_task_item_id(&task_id)
        .expect("query should succeed");
    assert_eq!(first, Some("item-low-order".to_string()));
}

// ── count_unresolved_items ─────────────────────────────────────────

#[test]
fn count_unresolved_items_zero_when_all_pending() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert_eq!(count, 0);
}

#[test]
fn count_unresolved_items_counts_unresolved_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE task_items SET status='unresolved' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("set unresolved");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert!(count >= 1);
}

#[test]
fn count_unresolved_items_counts_qa_failed_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE task_items SET status='qa_failed' WHERE task_id = ?1",
        params![task_id.clone()],
    )
    .expect("set qa_failed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    assert!(count >= 1);
}

#[test]
fn count_unresolved_items_mixed_statuses() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Add two more items with different statuses
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-unresolved', ?1, 10, '/tmp/qa1.md', 'unresolved', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert unresolved");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-qa-failed', ?1, 11, '/tmp/qa2.md', 'qa_failed', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert qa_failed");
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-passed', ?1, 12, '/tmp/qa3.md', 'qa_passed', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert qa_passed");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_unresolved_items(&task_id)
        .expect("count should succeed");
    // Only 'unresolved' and 'qa_failed' should be counted (original item is still 'pending')
    assert_eq!(count, 2);
}

// ── list_task_items_for_cycle ───────────────────────────────────────

#[test]
fn list_task_items_for_cycle_returns_items_in_order() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Add a second item with a higher order_no
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at) VALUES ('item-second', ?1, 99, '/tmp/qa_second.md', 'pending', '[]', '[]', 0, 0, '', '', '', ?2, ?2)",
        params![task_id.clone(), ts],
    )
    .expect("insert second item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let items = repo
        .list_task_items_for_cycle(&task_id)
        .expect("list should succeed");
    assert!(items.len() >= 2, "should have at least 2 items");
    // Last item should be our inserted one
    let last = items.last().expect("last item should exist");
    assert_eq!(last.id, "item-second");
    assert_eq!(last.qa_file_path, "/tmp/qa_second.md");
}

#[test]
fn list_task_items_for_cycle_returns_empty_for_unknown_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let items = repo
        .list_task_items_for_cycle("nonexistent-task-id")
        .expect("list should succeed even for unknown task");
    assert!(items.is_empty());
}

#[test]
fn list_task_items_for_cycle_preserves_dynamic_metadata() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    let ts = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, updated_at, created_at, dynamic_vars_json, label, source)
         VALUES ('item-dynamic-meta', ?1, 50, '/tmp/dynamic.md', 'pending', '[]', '[]', 0, 0, '', '', '', ?2, ?2, '{\"ticket\":\"A-1\"}', 'Generated item', 'dynamic')",
        params![task_id.clone(), ts],
    )
    .expect("insert dynamic item");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let items = repo
        .list_task_items_for_cycle(&task_id)
        .expect("list should succeed");
    let dynamic = items
        .into_iter()
        .find(|item| item.id == "item-dynamic-meta")
        .expect("dynamic item should be present");
    assert_eq!(
        dynamic.dynamic_vars_json.as_deref(),
        Some("{\"ticket\":\"A-1\"}")
    );
    assert_eq!(dynamic.label.as_deref(), Some("Generated item"));
    assert_eq!(dynamic.source, "dynamic");
}

#[test]
fn query_helpers_error_on_missing_schema() {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");

    assert!(super::super::queries::load_task_summary(&conn, "missing").is_err());
    assert!(super::super::queries::load_task_detail_rows(&conn, "missing").is_err());
    assert!(super::super::queries::list_task_items_for_cycle(&conn, "missing").is_err());
    assert!(super::super::queries::list_task_log_runs(&conn, "missing", 5).is_err());
    assert!(super::super::queries::insert_task_graph_run(
        &conn,
        &NewTaskGraphRun {
            graph_run_id: "graph-missing".to_string(),
            task_id: "task-missing".to_string(),
            cycle: 1,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: None,
            planner_failure_class: None,
            planner_failure_message: None,
            entry_node_id: None,
            node_count: 0,
            edge_count: 0,
        },
    )
    .is_err());
    assert!(super::super::queries::update_task_graph_run_status(
        &conn,
        "graph-missing",
        "completed",
    )
    .is_err());
    assert!(super::super::queries::insert_task_graph_snapshot(
        &conn,
        &NewTaskGraphSnapshot {
            graph_run_id: "graph-missing".to_string(),
            task_id: "task-missing".to_string(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{}".to_string(),
        },
    )
    .is_err());
}

#[test]
fn load_task_graph_debug_bundles_errors_when_event_table_is_missing() {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch(
        "CREATE TABLE task_graph_runs (
            graph_run_id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            cycle INTEGER NOT NULL,
            mode TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            fallback_mode TEXT,
            planner_failure_class TEXT,
            planner_failure_message TEXT,
            entry_node_id TEXT,
            node_count INTEGER NOT NULL,
            edge_count INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE task_graph_snapshots (
            graph_run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            snapshot_kind TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (graph_run_id, snapshot_kind)
        );",
    )
    .expect("create graph tables");

    let err = super::super::queries::load_task_graph_debug_bundles(&conn, "missing")
        .expect_err("missing events table should fail during legacy fallback");
    assert!(err.to_string().contains("no such table"));
}

// ── load_task_name ─────────────────────────────────────────────────

#[test]
fn load_task_name_returns_name() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let name = repo
        .load_task_name(&task_id)
        .expect("load_task_name should succeed");
    assert_eq!(name, Some("repo-test".to_string()));
}

#[test]
fn load_task_name_returns_none_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let name = repo
        .load_task_name("nonexistent-id")
        .expect("load_task_name should succeed");
    assert_eq!(name, None);
}

// ── load_task_status ───────────────────────────────────────────────

#[test]
fn load_task_status_returns_current_status() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let status = repo
        .load_task_status(&task_id)
        .expect("load_task_status should succeed");
    assert_eq!(status, Some("created".to_string()));
}

#[test]
fn load_task_status_reflects_status_change() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.execute(
        "UPDATE tasks SET status='running' WHERE id = ?1",
        params![task_id.clone()],
    )
    .expect("set running");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let status = repo
        .load_task_status(&task_id)
        .expect("load_task_status should succeed");
    assert_eq!(status, Some("running".to_string()));
}

#[test]
fn load_task_status_returns_none_for_missing_task() {
    let mut fixture = TestState::new();
    let (state, _task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));

    let status = repo
        .load_task_status("nonexistent-id")
        .expect("load_task_status should succeed");
    assert_eq!(status, None);
}

// ── FR-052: count_recent_heartbeats_for_items ──────────

#[test]
fn fr052_count_recent_heartbeats_returns_zero_when_no_events() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_ids = vec!["item-1".to_string()];
    let cutoff = "2000-01-01T00:00:00+00:00";
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &item_ids, cutoff)
        .expect("query should succeed");
    assert_eq!(count, 0);
}

#[test]
fn fr052_count_recent_heartbeats_returns_zero_for_empty_item_ids() {
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &[], "2000-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 0);
}

#[test]
fn fr052_count_recent_heartbeats_finds_matching_events() {
    use super::fixtures::get_item_id;
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let now = crate::config_load::now_ts();
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, 'step_heartbeat', '{}', ?3)",
        params![task_id, item_id, now],
    )
    .expect("insert heartbeat event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_ids = vec![item_id.clone()];
    // Cutoff far in the past — should find the heartbeat
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &item_ids, "2000-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 1);
}

#[test]
fn fr052_count_recent_heartbeats_excludes_old_events() {
    use super::fixtures::get_item_id;
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    // Insert heartbeat with an old timestamp
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, 'step_heartbeat', '{}', '2020-01-01T00:00:00+00:00')",
        params![task_id, item_id],
    )
    .expect("insert old heartbeat event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_ids = vec![item_id.clone()];
    // Cutoff after the event — should NOT find it
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &item_ids, "2025-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 0);
}

#[test]
fn fr052_count_recent_heartbeats_ignores_non_heartbeat_events() {
    use super::fixtures::get_item_id;
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let now = crate::config_load::now_ts();
    // Insert a non-heartbeat event
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, 'step_completed', '{}', ?3)",
        params![task_id, item_id, now],
    )
    .expect("insert non-heartbeat event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    let item_ids = vec![item_id.clone()];
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &item_ids, "2000-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 0, "non-heartbeat events should be excluded");
}

#[test]
fn fr052_count_recent_heartbeats_filters_by_item_id() {
    use super::fixtures::get_item_id;
    let mut fixture = TestState::new();
    let (state, task_id) = seed_task(&mut fixture);
    let item_id = get_item_id(&state, &task_id);
    let conn = open_conn(&state.db_path).expect("open sqlite");

    let now = crate::config_load::now_ts();
    // Insert heartbeat for our item
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, 'step_heartbeat', '{}', ?3)",
        params![task_id, item_id, now],
    )
    .expect("insert heartbeat event");

    let repo = SqliteTaskRepository::new(TaskRepositorySource::from(state.db_path.clone()));
    // Query with a different item_id — should not find anything
    let other_ids = vec!["nonexistent-item".to_string()];
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &other_ids, "2000-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 0, "should not match different item IDs");

    // Query with the correct item_id — should find it
    let correct_ids = vec![item_id];
    let count = repo
        .count_recent_heartbeats_for_items(&task_id, &correct_ids, "2000-01-01T00:00:00+00:00")
        .expect("query should succeed");
    assert_eq!(count, 1);
}
