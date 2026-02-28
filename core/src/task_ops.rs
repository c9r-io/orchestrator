use crate::config::LoopMode;
use crate::config_load::build_execution_plan;
use crate::config_load::{now_ts, read_active_config};
use crate::db::open_conn;
use crate::dto::{CreateTaskPayload, TaskSummary};
use crate::scheduler::load_task_summary;
use crate::ticket::{
    collect_target_files, collect_target_files_from_active_tickets,
    should_seed_targets_from_active_tickets,
};
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

pub fn create_task_impl(
    state: &crate::state::InnerState,
    payload: CreateTaskPayload,
) -> Result<TaskSummary> {
    let active = read_active_config(state)?;

    let project_id = payload
        .project_id
        .clone()
        .unwrap_or_else(|| active.default_project_id.clone());

    let workspace_id = payload
        .workspace_id
        .clone()
        .unwrap_or_else(|| active.default_workspace_id.clone());

    let workspace = active
        .projects
        .get(&project_id)
        .and_then(|p| p.workspaces.get(&workspace_id).cloned())
        .or_else(|| active.workspaces.get(&workspace_id).cloned())
        .with_context(|| {
            format!(
                "workspace not found: {} (checked project '{}' then global)",
                workspace_id, project_id
            )
        })?;

    let workflow_id = payload
        .workflow_id
        .clone()
        .unwrap_or_else(|| active.default_workflow_id.clone());

    let workflow = active
        .projects
        .get(&project_id)
        .and_then(|p| p.workflows.get(&workflow_id).cloned())
        .or_else(|| active.config.workflows.get(&workflow_id).cloned())
        .with_context(|| {
            format!(
                "workflow not found: {} (checked project '{}' then global)",
                workflow_id, project_id
            )
        })?;

    let execution_plan = build_execution_plan(&active.config, &workflow, &workflow_id)?;
    let execution_plan_json =
        serde_json::to_string(&execution_plan).context("serialize execution plan")?;
    let loop_mode = match execution_plan.loop_policy.mode {
        LoopMode::Once => "once",
        LoopMode::Fixed => "fixed",
        LoopMode::Infinite => "infinite",
    };

    let target_files_input = payload.target_files.clone();
    let seed_from_tickets =
        should_seed_targets_from_active_tickets(target_files_input.as_ref(), &execution_plan);
    let mut target_files = if seed_from_tickets {
        collect_target_files_from_active_tickets(&workspace.root_path, &workspace.ticket_dir)?
    } else {
        collect_target_files(
            &workspace.root_path,
            &workspace.qa_targets,
            target_files_input,
        )?
    };
    if target_files.is_empty() {
        if seed_from_tickets {
            target_files.push(crate::dto::UNASSIGNED_QA_FILE_PATH.to_string());
        } else {
            anyhow::bail!("No QA/Security markdown files found");
        }
    }

    let task_id = Uuid::new_v4().to_string();
    let created_at = now_ts();
    let task_name = payload
        .name
        .unwrap_or_else(|| format!("QA Sprint {}", Utc::now().format("%Y-%m-%d %H:%M:%S")));
    let goal = payload
        .goal
        .unwrap_or_else(|| "Automated QA workflow with fix and resume".to_string());

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tasks (id, name, status, started_at, completed_at, goal, target_files_json, mode, project_id, workspace_id, workflow_id, workspace_root, qa_targets_json, ticket_dir, execution_plan_json, loop_mode, current_cycle, init_done, resume_token, created_at, updated_at) VALUES (?1, ?2, 'pending', NULL, NULL, ?3, ?4, '', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, NULL, ?13, ?13)",
        params![
            task_id,
            task_name,
            goal,
            serde_json::to_string(&target_files)?,
            project_id,
            workspace_id,
            workflow_id,
            workspace.root_path.to_string_lossy().to_string(),
            serde_json::to_string(&workspace.qa_targets)?,
            workspace.ticket_dir,
            execution_plan_json,
            loop_mode,
            created_at
        ],
    )?;

    for (idx, path) in target_files.iter().enumerate() {
        let item_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5)",
            params![item_id, task_id, (idx as i64) + 1, path, created_at],
        )?;
    }
    tx.commit()?;

    load_task_summary(state, &task_id)
}

pub fn reset_task_item_for_retry(
    state: &crate::state::InnerState,
    task_item_id: &str,
) -> Result<String> {
    let conn = open_conn(&state.db_path)?;
    let task_id: String = conn.query_row(
        "SELECT task_id FROM task_items WHERE id = ?1",
        params![task_item_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE task_items SET status = 'pending', ticket_files_json = '[]', ticket_content_json = '[]', fix_required = 0, fixed = 0, last_error = '', started_at = NULL, completed_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![task_item_id, now_ts()],
    )?;
    Ok(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::test_utils::TestState;

    #[test]
    fn create_task_with_defaults() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create a QA file so target_files is non-empty
        let active = crate::config_load::read_active_config(&state).unwrap();
        let ws = active.workspaces.get("default").unwrap();
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("test-qa.md"), "# QA Test\n").unwrap();
        drop(active);

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_ok(), "create_task_impl should succeed: {:?}", result.err());
        let summary = result.unwrap();
        assert_eq!(summary.status, "pending");
        assert!(!summary.id.is_empty());
        assert!(summary.name.starts_with("QA Sprint"));
        assert_eq!(summary.goal, "Automated QA workflow with fix and resume");
        assert_eq!(summary.workspace_id, "default");
        assert_eq!(summary.workflow_id, "basic");
        assert!(summary.total_items >= 1);
    }

    #[test]
    fn create_task_with_custom_name_and_goal() {
        let mut ts = TestState::new();
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).unwrap();
        let ws = active.workspaces.get("default").unwrap();
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("custom-qa.md"), "# Custom QA\n").unwrap();
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("My Custom Task".to_string()),
            goal: Some("Custom goal description".to_string()),
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let result = create_task_impl(&state, payload).unwrap();
        assert_eq!(result.name, "My Custom Task");
        assert_eq!(result.goal, "Custom goal description");
    }

    #[test]
    fn create_task_with_nonexistent_workspace_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: Some("nonexistent-ws".to_string()),
            workflow_id: None,
            target_files: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("workspace not found"), "unexpected error: {}", err);
    }

    #[test]
    fn create_task_with_nonexistent_workflow_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: Some("nonexistent-wf".to_string()),
            target_files: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("workflow not found"), "unexpected error: {}", err);
    }

    #[test]
    fn create_task_with_no_qa_files_fails() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Don't create any qa files - the qa_targets dirs exist but are empty
        let payload = CreateTaskPayload {
            name: None,
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let result = create_task_impl(&state, payload);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No QA/Security markdown files found"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn create_task_with_explicit_target_files() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create target files
        let active = crate::config_load::read_active_config(&state).unwrap();
        let ws = active.workspaces.get("default").unwrap();
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        let file1 = qa_path.join("file1.md");
        let file2 = qa_path.join("file2.md");
        std::fs::write(&file1, "# File 1\n").unwrap();
        std::fs::write(&file2, "# File 2\n").unwrap();
        let rel1 = format!("{}/file1.md", qa_dir);
        let rel2 = format!("{}/file2.md", qa_dir);
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Targeted".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: Some(vec![rel1, rel2]),
        };
        let result = create_task_impl(&state, payload).unwrap();
        assert_eq!(result.total_items, 2, "should have 2 task items");
    }

    #[test]
    fn create_multiple_tasks_get_unique_ids() {
        let mut ts = TestState::new();
        let state = ts.build();

        let active = crate::config_load::read_active_config(&state).unwrap();
        let ws = active.workspaces.get("default").unwrap();
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("multi.md"), "# Multi\n").unwrap();
        drop(active);

        let payload1 = CreateTaskPayload {
            name: Some("Task 1".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let payload2 = CreateTaskPayload {
            name: Some("Task 2".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let t1 = create_task_impl(&state, payload1).unwrap();
        let t2 = create_task_impl(&state, payload2).unwrap();
        assert_ne!(t1.id, t2.id, "tasks should have unique ids");
    }

    #[test]
    fn reset_task_item_for_retry_resets_fields() {
        let mut ts = TestState::new();
        let state = ts.build();

        // Create a task first
        let active = crate::config_load::read_active_config(&state).unwrap();
        let ws = active.workspaces.get("default").unwrap();
        let qa_dir = &ws.qa_targets[0];
        let qa_path = ws.root_path.join(qa_dir);
        std::fs::create_dir_all(&qa_path).ok();
        std::fs::write(qa_path.join("retry.md"), "# Retry\n").unwrap();
        drop(active);

        let payload = CreateTaskPayload {
            name: Some("Retry Task".to_string()),
            goal: None,
            project_id: None,
            workspace_id: None,
            workflow_id: None,
            target_files: None,
        };
        let task = create_task_impl(&state, payload).unwrap();

        // Get an item id
        let conn = open_conn(&state.db_path).unwrap();
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 LIMIT 1",
                params![task.id],
                |row| row.get(0),
            )
            .unwrap();

        // Update item to simulate completed/failed state
        conn.execute(
            "UPDATE task_items SET status = 'failed', fix_required = 1, last_error = 'some error', started_at = '2024-01-01', completed_at = '2024-01-01' WHERE id = ?1",
            params![item_id],
        ).unwrap();

        // Reset it
        let returned_task_id = reset_task_item_for_retry(&state, &item_id).unwrap();
        assert_eq!(returned_task_id, task.id);

        // Verify reset
        let (status, fix_required, last_error, started_at, completed_at): (
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT status, fix_required, last_error, started_at, completed_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(fix_required, 0);
        assert_eq!(last_error, "");
        assert!(started_at.is_none());
        assert!(completed_at.is_none());
    }

    #[test]
    fn reset_task_item_for_retry_nonexistent_item_fails() {
        let mut ts = TestState::new();
        let state = ts.build();
        let result = reset_task_item_for_retry(&state, "nonexistent-item-id");
        assert!(result.is_err(), "should fail for nonexistent item");
    }
}
