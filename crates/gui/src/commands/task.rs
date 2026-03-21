use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub total_items: i64,
    pub finished_items: i64,
    pub failed_items: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDetail {
    pub id: String,
    pub name: String,
    pub status: String,
    pub goal: String,
    pub total_items: i64,
    pub finished_items: i64,
    pub failed_items: i64,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<TaskItemSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskItemSummary {
    pub id: String,
    pub qa_file_path: String,
    pub status: String,
    pub order_no: i64,
}

/// List all tasks with optional status filter.
#[tauri::command]
pub async fn task_list(
    state: State<'_, AppState>,
    status_filter: Option<String>,
) -> Result<Vec<TaskSummary>, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_list(orchestrator_proto::TaskListRequest {
            status_filter,
            project_filter: None,
        })
        .await
        .map_err(|e| e.message().to_string())?;

    let tasks = resp
        .into_inner()
        .tasks
        .into_iter()
        .map(|t| TaskSummary {
            id: t.id,
            name: t.name,
            status: t.status,
            total_items: t.total_items,
            finished_items: t.finished_items,
            failed_items: t.failed_items,
            created_at: t.created_at,
            updated_at: t.updated_at,
        })
        .collect();
    Ok(tasks)
}

/// Get detailed info for a single task.
#[tauri::command]
pub async fn task_info(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<TaskDetail, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_info(orchestrator_proto::TaskInfoRequest { task_id })
        .await
        .map_err(|e| e.message().to_string())?;

    let inner = resp.into_inner();
    let task = inner.task.ok_or("task not found")?;
    let items = inner
        .items
        .into_iter()
        .map(|i| TaskItemSummary {
            id: i.id,
            qa_file_path: i.qa_file_path,
            status: i.status,
            order_no: i.order_no,
        })
        .collect();

    Ok(TaskDetail {
        id: task.id,
        name: task.name,
        status: task.status,
        goal: task.goal,
        total_items: task.total_items,
        finished_items: task.finished_items,
        failed_items: task.failed_items,
        created_at: task.created_at,
        updated_at: task.updated_at,
        items,
    })
}
