use serde::Serialize;
use tauri::State;

use std::sync::Arc;

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
    pub project_id: String,
    pub workflow_id: String,
    pub goal: String,
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
    pub project_id: String,
    pub workflow_id: String,
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
    state: State<'_, Arc<AppState>>,
    status_filter: Option<String>,
    project_filter: Option<String>,
) -> Result<Vec<TaskSummary>, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_list(orchestrator_proto::TaskListRequest {
            status_filter,
            project_filter,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

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
            project_id: t.project_id,
            workflow_id: t.workflow_id,
            goal: t.goal,
        })
        .collect();
    Ok(tasks)
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskCreateResult {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskActionResult {
    pub message: String,
}

/// Create a new task (wish pool → FR drafting or development).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn task_create(
    state: State<'_, Arc<AppState>>,
    name: Option<String>,
    goal: Option<String>,
    project_id: Option<String>,
    workspace_id: Option<String>,
    workflow_id: Option<String>,
    target_files: Option<Vec<String>>,
    no_start: Option<bool>,
) -> Result<TaskCreateResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_create(orchestrator_proto::TaskCreateRequest {
            name,
            goal,
            project_id,
            workspace_id,
            workflow_id,
            target_files: target_files.unwrap_or_default(),
            no_start: no_start.unwrap_or(false),
            step_filter: vec![],
            initial_vars: Default::default(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(TaskCreateResult {
        task_id: inner.task_id,
        status: inner.status,
        message: inner.message,
    })
}

/// Start a pending task.
#[tauri::command]
pub async fn task_start(
    state: State<'_, Arc<AppState>>,
    task_id: Option<String>,
    latest: Option<bool>,
) -> Result<TaskCreateResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_start(orchestrator_proto::TaskStartRequest {
            task_id,
            latest: latest.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(TaskCreateResult {
        task_id: inner.task_id,
        status: inner.status,
        message: inner.message,
    })
}

/// Pause a running task (operator+).
#[tauri::command]
pub async fn task_pause(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<TaskActionResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_pause(orchestrator_proto::TaskPauseRequest { task_id })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(TaskActionResult {
        message: resp.into_inner().message,
    })
}

/// Resume a paused task (operator+).
#[tauri::command]
pub async fn task_resume(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    reset_blocked: Option<bool>,
) -> Result<TaskActionResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_resume(orchestrator_proto::TaskResumeRequest {
            task_id,
            reset_blocked: reset_blocked.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(TaskActionResult {
        message: resp.into_inner().message,
    })
}

/// Retry a failed task item (operator+).
#[tauri::command]
pub async fn task_retry(
    state: State<'_, Arc<AppState>>,
    task_item_id: String,
    force: Option<bool>,
) -> Result<TaskActionResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_retry(orchestrator_proto::TaskRetryRequest {
            task_item_id,
            force: force.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(TaskActionResult {
        message: resp.into_inner().message,
    })
}

/// Delete a task (admin).
#[tauri::command]
pub async fn task_delete(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    force: Option<bool>,
) -> Result<TaskActionResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_delete(orchestrator_proto::TaskDeleteRequest {
            task_id,
            force: force.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(TaskActionResult {
        message: resp.into_inner().message,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskTraceResult {
    pub trace_json: String,
}

/// Get task execution trace (read_only+).
#[tauri::command]
pub async fn task_trace(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    verbose: Option<bool>,
) -> Result<TaskTraceResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_trace(orchestrator_proto::TaskTraceRequest {
            task_id,
            verbose: verbose.unwrap_or(false),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    Ok(TaskTraceResult {
        trace_json: resp.into_inner().trace_json,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRecoverResult {
    pub task_id: String,
    pub recovered_items: u64,
    pub message: String,
}

/// Recover a task from error state (operator+).
#[tauri::command]
pub async fn task_recover(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<TaskRecoverResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_recover(orchestrator_proto::TaskRecoverRequest { task_id })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(TaskRecoverResult {
        task_id: inner.task_id,
        recovered_items: inner.recovered_items,
        message: inner.message,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct BulkDeleteResult {
    pub deleted: i32,
    pub failed: i32,
    pub errors: Vec<String>,
    pub message: String,
}

/// Bulk delete tasks (admin).
#[tauri::command]
pub async fn task_delete_bulk(
    state: State<'_, Arc<AppState>>,
    task_ids: Option<Vec<String>>,
    force: Option<bool>,
    status_filter: Option<String>,
    project_filter: Option<String>,
) -> Result<BulkDeleteResult, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_delete_bulk(orchestrator_proto::TaskDeleteBulkRequest {
            task_ids: task_ids.unwrap_or_default(),
            force: force.unwrap_or(false),
            status_filter: status_filter.unwrap_or_default(),
            project_filter: project_filter.unwrap_or_default(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;
    let inner = resp.into_inner();
    Ok(BulkDeleteResult {
        deleted: inner.deleted,
        failed: inner.failed,
        errors: inner.errors,
        message: inner.message,
    })
}

/// Get detailed info for a single task.
#[tauri::command]
pub async fn task_info(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<TaskDetail, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_info(orchestrator_proto::TaskInfoRequest { task_id })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

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
        project_id: task.project_id,
        workflow_id: task.workflow_id,
        items,
    })
}
