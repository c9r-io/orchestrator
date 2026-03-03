//! Task CRUD query operations.

use crate::dto::{TaskDetail, TaskSummary};
use crate::state::InnerState;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::Result;

use super::retry_query;

/// Load a full task detail snapshot (summary + items + runs + events).
pub fn load_task_detail_snapshot(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    retry_query("load task details", || {
        let task = load_task_summary(state, task_id)?;
        let repo = SqliteTaskRepository::new(state.database.clone());
        let (items, runs, events) = repo.load_task_detail_rows(&task.id)?;

        Ok(TaskDetail {
            task,
            items,
            runs,
            events,
        })
    })
}

/// Resolve a task ID (exact match or prefix) to its full ID.
pub fn resolve_task_id(state: &InnerState, task_id: &str) -> Result<String> {
    SqliteTaskRepository::new(state.database.clone()).resolve_task_id(task_id)
}

/// Load a task summary with item counts.
pub fn load_task_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary> {
    retry_query("load task summary", || {
        let resolved_id = resolve_task_id(state, task_id)?;
        let repo = SqliteTaskRepository::new(state.database.clone());
        let mut summary = repo.load_task_summary(&resolved_id)?;
        let (total, finished, failed) = repo.load_task_item_counts(&resolved_id)?;

        summary.total_items = total;
        summary.finished_items = finished;
        summary.failed_items = failed;
        Ok(summary)
    })
}

/// List all tasks ordered by creation date (most recent first).
pub fn list_tasks_impl(state: &InnerState) -> Result<Vec<TaskSummary>> {
    let repo = SqliteTaskRepository::new(state.database.clone());
    let ids = repo.list_task_ids_ordered_by_created_desc()?;

    let mut result = Vec::new();
    for id in ids {
        result.push(load_task_summary(state, &id)?);
    }
    Ok(result)
}

/// Get full task details including items, runs, and events.
pub fn get_task_details_impl(state: &InnerState, task_id: &str) -> Result<TaskDetail> {
    load_task_detail_snapshot(state, task_id)
}

/// Delete a task and its associated log files.
pub fn delete_task_impl(state: &InnerState, task_id: &str) -> Result<()> {
    let resolved_id = resolve_task_id(state, task_id)?;
    let repo = SqliteTaskRepository::new(state.database.clone());
    let log_paths = repo.delete_task_and_collect_log_paths(&resolved_id)?;

    for path in log_paths {
        let _ = std::fs::remove_file(path);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::{first_item_id, seed_task, test_dir};
    use super::*;
    use crate::config_load::now_ts;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::task_repository::{NewCommandRun, SqliteTaskRepository, TaskRepository};
    use crate::test_utils::TestState;

    #[test]
    fn resolve_task_id_exact_match() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let resolved = resolve_task_id(&state, &task_id).expect("resolve exact id");
        assert_eq!(resolved, task_id);
    }

    #[test]
    fn resolve_task_id_prefix_match() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let prefix = &task_id[..8];
        let resolved = resolve_task_id(&state, prefix).expect("resolve prefix id");
        assert_eq!(resolved, task_id);
    }

    #[test]
    fn resolve_task_id_not_found() {
        let mut fixture = TestState::new();
        let (state, _task_id) = seed_task(&mut fixture);
        let result = resolve_task_id(&state, "nonexistent-id-00000000");
        assert!(result.is_err());
    }

    #[test]
    fn load_task_summary_returns_counts() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let summary = load_task_summary(&state, &task_id).expect("load task summary");
        assert_eq!(summary.id, task_id);
        assert_eq!(summary.name, "query-test");
        assert_eq!(summary.goal, "query-test-goal");
        // The task should have at least 1 item (the seeded qa file)
        assert!(summary.total_items >= 1, "expected at least 1 total_items");
        // Initially nothing is finished or failed
        assert_eq!(summary.finished_items, 0);
        assert_eq!(summary.failed_items, 0);
    }

    #[test]
    fn load_task_summary_with_prefix() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let prefix = &task_id[..8];
        let summary = load_task_summary(&state, prefix).expect("load summary by prefix");
        assert_eq!(summary.id, task_id);
    }

    #[test]
    fn list_tasks_impl_returns_seeded_task() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].name, "query-test");
    }

    #[test]
    fn list_tasks_impl_empty_when_no_tasks() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_tasks_impl_multiple_tasks_ordered_desc() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/multi_test.md");
        std::fs::write(&qa_file, "# multi test\n").expect("seed qa file");

        let t1 = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-1".to_string()),
                ..Default::default()
            },
        )
        .expect("create task 1");

        let t2 = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-2".to_string()),
                ..Default::default()
            },
        )
        .expect("create task 2");

        let tasks = list_tasks_impl(&state).expect("list tasks");
        assert_eq!(tasks.len(), 2);
        // Most recent first
        assert_eq!(tasks[0].id, t2.id);
        assert_eq!(tasks[1].id, t1.id);
    }

    #[test]
    fn get_task_details_impl_returns_items_and_empty_runs() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let detail = get_task_details_impl(&state, &task_id).expect("get task details");
        assert_eq!(detail.task.id, task_id);
        assert!(!detail.items.is_empty(), "should have at least 1 item");
        // No command runs yet
        assert!(detail.runs.is_empty());
    }

    #[test]
    fn get_task_details_impl_with_command_run() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        let dir = test_dir("details-run");
        let stdout_path = dir.join("stdout.log");
        let stderr_path = dir.join("stderr.log");
        std::fs::write(&stdout_path, "output").expect("write stdout");
        std::fs::write(&stderr_path, "").expect("write stderr");

        let repo = SqliteTaskRepository::new(state.database.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-detail-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo test".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        let detail = get_task_details_impl(&state, &task_id).expect("get task details");
        assert_eq!(detail.runs.len(), 1);
        assert_eq!(detail.runs[0].id, "run-detail-1");
        assert_eq!(detail.runs[0].phase, "qa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_task_impl_removes_task_and_log_files() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let item_id = first_item_id(&state, &task_id);

        // Create log files on disk
        let dir = test_dir("delete-logs");
        let stdout_path = dir.join("delete_stdout.log");
        let stderr_path = dir.join("delete_stderr.log");
        std::fs::write(&stdout_path, "stdout data").expect("write stdout");
        std::fs::write(&stderr_path, "stderr data").expect("write stderr");

        let repo = SqliteTaskRepository::new(state.database.clone());
        repo.insert_command_run(&NewCommandRun {
            id: "run-delete-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo delete".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.to_string_lossy().to_string(),
            stderr_path: stderr_path.to_string_lossy().to_string(),
            started_at: now_ts(),
            ended_at: now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: None,
            quality_score: None,
            validation_status: "unknown".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .expect("insert command run");

        assert!(stdout_path.exists());
        assert!(stderr_path.exists());

        delete_task_impl(&state, &task_id).expect("delete task");

        // Log files should be cleaned up
        assert!(!stdout_path.exists(), "stdout log should be deleted");
        assert!(!stderr_path.exists(), "stderr log should be deleted");

        // Task should no longer be listable
        let tasks = list_tasks_impl(&state).expect("list after delete");
        assert!(tasks.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_task_impl_nonexistent_returns_error() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let result = delete_task_impl(&state, "nonexistent-task-id");
        assert!(result.is_err());
    }
}
