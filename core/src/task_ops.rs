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

pub fn create_task_impl(state: &crate::state::InnerState, payload: CreateTaskPayload) -> Result<TaskSummary> {
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

pub fn reset_task_item_for_retry(state: &crate::state::InnerState, task_item_id: &str) -> Result<String> {
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
