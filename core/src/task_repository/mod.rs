//! Task-execution persistence: tasks, items, command runs and events.
//!
//! The implementation moved to `orchestrator-persistence` (FR-130 Phase A) and
//! is re-exported here so every existing `crate::task_repository::*` and
//! `agent_orchestrator::task_repository::*` path keeps resolving.
//!
//! The tests stay in core. They are not unit tests of the repository — they
//! create a real task through `crate::task_ops::create_task_impl` against a
//! `crate::test_utils::TestState` fixture and then assert on what the
//! repository persisted. That makes them the end-to-end evidence that the
//! extraction preserved behaviour rather than only moved symbols, and it also
//! makes them unmovable: the domain machinery they drive sits above this layer.

pub use orchestrator_persistence::task_repository::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod async_wrapper_tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/async-repo.md");
        std::fs::write(&qa_file, "# async repo\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("async-repo".to_string()),
                goal: Some("async-repo-goal".to_string()),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create task");
        (state, created.id)
    }

    fn first_item_id(state: &crate::state::InnerState, task_id: &str) -> String {
        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("load item id")
    }

    #[tokio::test]
    async fn async_repository_read_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;

        let resolved = repo
            .resolve_task_id(&task_id[..8])
            .await
            .expect("resolve task id");
        assert_eq!(resolved, task_id);

        let summary = repo
            .load_task_summary(&task_id)
            .await
            .expect("load summary");
        assert_eq!(summary.name, "async-repo");

        let detail = repo
            .load_task_detail_rows(&task_id)
            .await
            .expect("load detail rows");
        assert!(!detail.0.is_empty());

        let counts = repo
            .load_task_item_counts(&task_id)
            .await
            .expect("load item counts");
        assert!(counts.0 >= 1);

        let ids = repo
            .list_task_ids_ordered_by_created_desc()
            .await
            .expect("list task ids");
        assert_eq!(ids[0], task_id);

        let resumable = repo
            .find_latest_resumable_task_id(true)
            .await
            .expect("find latest resumable");
        assert_eq!(resumable.as_deref(), Some(task_id.as_str()));

        let runtime = repo
            .load_task_runtime_row(&task_id)
            .await
            .expect("load runtime row");
        assert_eq!(runtime.goal, "async-repo-goal");

        let item_id = repo
            .first_task_item_id(&task_id)
            .await
            .expect("first item id query");
        assert!(item_id.is_some());

        let unresolved = repo
            .count_unresolved_items(&task_id)
            .await
            .expect("count unresolved");
        assert_eq!(unresolved, 0);

        let items = repo
            .list_task_items_for_cycle(&task_id)
            .await
            .expect("list items for cycle");
        assert!(!items.is_empty());

        let status = repo.load_task_status(&task_id).await.expect("load status");
        assert_eq!(status.as_deref(), Some("created"));

        let name = repo.load_task_name(&task_id).await.expect("load task name");
        assert_eq!(name.as_deref(), Some("async-repo"));

        let runs = repo
            .list_task_log_runs(&task_id, 10)
            .await
            .expect("list log runs");
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn async_repository_write_wrappers_update_task_and_item_state() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;
        let item_id = first_item_id(&state, &task_id);

        repo.mark_task_item_running(&item_id)
            .await
            .expect("mark item running");
        repo.update_task_item_status(&item_id, "qa_failed")
            .await
            .expect("update item status");
        repo.set_task_item_terminal_status(&item_id, "completed")
            .await
            .expect("set terminal status");
        repo.set_task_status(&task_id, "running", false)
            .await
            .expect("set task status");
        repo.update_task_cycle_state(&task_id, 3, true)
            .await
            .expect("update task cycle state");

        let conn =
            orchestrator_persistence::test_support::open_conn(&state.db_path).expect("open sqlite");
        let task_row: (String, i64, i64) = conn
            .query_row(
                "SELECT status, current_cycle, init_done FROM tasks WHERE id = ?1",
                rusqlite::params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("load task row");
        assert_eq!(task_row.0, "running");
        assert_eq!(task_row.1, 3);
        assert_eq!(task_row.2, 1);

        let item_status: String = conn
            .query_row(
                "SELECT status FROM task_items WHERE id = ?1",
                rusqlite::params![item_id],
                |row| row.get(0),
            )
            .expect("load item status");
        assert_eq!(item_status, "completed");

        repo.set_task_status(&task_id, "paused", false)
            .await
            .expect("pause task before prepare");
        repo.prepare_task_for_start_batch(&task_id)
            .await
            .expect("prepare task for start");
        let prepared_status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .expect("reload task status");
        assert_eq!(prepared_status, "running");
    }

    #[tokio::test]
    async fn async_repository_insert_and_delete_command_runs() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;
        let item_id = first_item_id(&state, &task_id);
        let stdout_path = state.data_dir.join("logs/async-wrapper-stdout.log");
        let stderr_path = state.data_dir.join("logs/async-wrapper-stderr.log");
        std::fs::create_dir_all(
            stdout_path
                .parent()
                .expect("stdout parent directory should exist"),
        )
        .expect("create logs dir");
        std::fs::write(&stdout_path, "stdout").expect("write stdout");
        std::fs::write(&stderr_path, "stderr").expect("write stderr");

        repo.insert_command_run(NewCommandRun {
            id: "async-run-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo repo".to_string(),
            command_template: None,
            cwd: state.data_dir.display().to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.display().to_string(),
            stderr_path: stderr_path.display().to_string(),
            started_at: crate::config_load::now_ts(),
            ended_at: crate::config_load::now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: Some(1.0),
            quality_score: Some(1.0),
            validation_status: "passed".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
            command_rule_index: None,
        })
        .await
        .expect("insert command run");

        let runs = repo
            .list_task_log_runs(&task_id, 10)
            .await
            .expect("list runs after insert");
        assert_eq!(runs.len(), 1);

        let paths = repo
            .delete_task_and_collect_log_paths(&task_id)
            .await
            .expect("delete task and collect log paths");
        assert_eq!(paths.len(), 2);
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("async-wrapper-stdout.log"))
        );
    }

    #[test]
    fn sqlite_repository_graph_debug_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo =
            SqliteTaskRepository::new(types::TaskRepositorySource::from(state.db_path.clone()));

        repo.insert_task_graph_run(&NewTaskGraphRun {
            graph_run_id: "sync-graph-run".to_string(),
            task_id: task_id.clone(),
            cycle: 4,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: Some("static_segment".to_string()),
            planner_failure_class: None,
            planner_failure_message: None,
            entry_node_id: Some("qa".to_string()),
            node_count: 2,
            edge_count: 1,
        })
        .expect("insert graph run");
        repo.update_task_graph_run_status("sync-graph-run", "completed")
            .expect("update graph run");
        repo.insert_task_graph_snapshot(&NewTaskGraphSnapshot {
            graph_run_id: "sync-graph-run".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"qa\"}".to_string(),
        })
        .expect("insert graph snapshot");

        let bundles = repo
            .load_task_graph_debug_bundles(&task_id)
            .expect("load graph bundles");
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].graph_run_id, "sync-graph-run");
        assert_eq!(bundles[0].status, "completed");
        assert_eq!(bundles[0].effective_graph_json, "{\"entry\":\"qa\"}");
    }

    #[tokio::test]
    async fn async_repository_graph_debug_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;

        repo.insert_task_graph_run(NewTaskGraphRun {
            graph_run_id: "async-graph-run".to_string(),
            task_id: task_id.clone(),
            cycle: 5,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: Some("deterministic_dag".to_string()),
            planner_failure_class: Some("invalid_json".to_string()),
            planner_failure_message: Some("planner output broken".to_string()),
            entry_node_id: Some("fix".to_string()),
            node_count: 3,
            edge_count: 2,
        })
        .await
        .expect("insert graph run");
        repo.update_task_graph_run_status("async-graph-run", "completed")
            .await
            .expect("update graph run");
        repo.insert_task_graph_snapshot(NewTaskGraphSnapshot {
            graph_run_id: "async-graph-run".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"fix\"}".to_string(),
        })
        .await
        .expect("insert graph snapshot");

        let bundles = repo
            .load_task_graph_debug_bundles(&task_id)
            .await
            .expect("load graph bundles");
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].graph_run_id, "async-graph-run");
        assert_eq!(
            bundles[0].fallback_mode.as_deref(),
            Some("deterministic_dag")
        );
        assert_eq!(bundles[0].effective_graph_json, "{\"entry\":\"fix\"}");
    }
}
