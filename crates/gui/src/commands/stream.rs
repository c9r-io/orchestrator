use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_notification::NotificationExt;

use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub line: String,
    pub timestamp: String,
}

/// Start streaming task logs via Tauri events.
///
/// Each log line is emitted as a `task-follow-{task_id}` event.
#[tauri::command]
pub async fn start_task_follow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let resp = client
        .task_follow(orchestrator_proto::TaskFollowRequest {
            task_id: task_id.clone(),
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let mut stream = resp.into_inner();
    let cancel = state.register_stream(&task_id).await;
    let event_name = format!("task-follow-{}", task_id);

    let error_event = format!("stream-error-{}", task_id);
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                msg = stream.message() => {
                    match msg {
                        Ok(Some(log_entry)) => {
                            let payload = LogLine {
                                line: log_entry.line,
                                timestamp: log_entry.timestamp,
                            };
                            let _ = app.emit(&event_name, &payload);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let msg = crate::errors::humanize_grpc_error(&e);
                            let _ = app.emit(&error_event, &msg);
                            break;
                        }
                    }
                }
                _ = cancel.cancelled() => break,
            }
        }
    });

    Ok(())
}

/// Stop streaming task logs.
#[tauri::command]
pub async fn stop_task_follow(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<(), String> {
    state.cancel_stream(&task_id).await;
    Ok(())
}

/// Snapshot of a task's current state, emitted by TaskWatch streaming.
#[derive(Debug, Clone, Serialize)]
pub struct WatchSnapshot {
    pub task: super::task::TaskSummary,
    pub items: Vec<super::task::TaskItemSummary>,
}

/// Start watching task status updates via Tauri events.
///
/// Each snapshot is emitted as a `task-watch-{task_id}` event.
#[tauri::command]
pub async fn start_task_watch(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    task_id: String,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let resp = client
        .task_watch(orchestrator_proto::TaskWatchRequest {
            task_id: task_id.clone(),
            interval_secs: interval_secs.unwrap_or(2),
            timeout_secs: 0, // no timeout
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let mut stream = resp.into_inner();
    let watch_key = format!("watch-{}", task_id);
    let cancel = state.register_stream(&watch_key).await;
    let event_name = format!("task-watch-{}", task_id);

    let error_event = format!("stream-error-watch-{}", task_id);
    tauri::async_runtime::spawn(async move {
        let mut prev_status = String::new();
        loop {
            tokio::select! {
                msg = stream.message() => {
                    match msg {
                        Ok(Some(snapshot)) => {
                            let task = snapshot.task.map(|t| super::task::TaskSummary {
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
                            });
                            if let Some(task) = task {
                                // Detect status transitions for notifications.
                                if !prev_status.is_empty() && task.status != prev_status {
                                    send_task_notification(&app, &task.name, &task.status, &task.project_id);
                                }
                                prev_status.clone_from(&task.status);

                                let items: Vec<_> = snapshot.items.into_iter().map(|i| {
                                    super::task::TaskItemSummary {
                                        id: i.id,
                                        qa_file_path: i.qa_file_path,
                                        status: i.status,
                                        order_no: i.order_no,
                                    }
                                }).collect();
                                let payload = WatchSnapshot { task, items };
                                let _ = app.emit(&event_name, &payload);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let msg = crate::errors::humanize_grpc_error(&e);
                            let _ = app.emit(&error_event, &msg);
                            break;
                        }
                    }
                }
                _ = cancel.cancelled() => break,
            }
        }
    });

    Ok(())
}

/// Send OS notification for task status transitions.
fn send_task_notification(app: &AppHandle, task_name: &str, status: &str, project_id: &str) {
    let (title, body) = match status {
        "completed" | "succeeded" => {
            if project_id == "wish-pool" {
                (
                    "FR 草稿就绪".to_string(),
                    format!("「{}」的需求方案已生成，等待确认", task_name),
                )
            } else {
                (
                    "任务完成".to_string(),
                    format!("「{}」已成功完成", task_name),
                )
            }
        }
        "failed" | "error" => ("任务失败".to_string(), format!("「{}」执行失败", task_name)),
        _ => return,
    };

    let _ = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show();
}

/// Stop watching task status updates.
#[tauri::command]
pub async fn stop_task_watch(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<(), String> {
    let watch_key = format!("watch-{}", task_id);
    state.cancel_stream(&watch_key).await;
    Ok(())
}

/// A chunk of historical task logs.
#[derive(Debug, Clone, Serialize)]
pub struct TaskLogChunk {
    pub run_id: String,
    pub phase: String,
    pub content: String,
    pub started_at: Option<String>,
}

/// Get historical task logs (collects all chunks from the streaming RPC).
#[tauri::command]
pub async fn task_logs(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    tail: Option<u64>,
) -> Result<Vec<TaskLogChunk>, String> {
    let mut client = state.client().await?;
    let resp = client
        .task_logs(orchestrator_proto::TaskLogsRequest {
            task_id,
            tail: tail.unwrap_or(0),
            timestamps: false,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let mut stream = resp.into_inner();
    let mut chunks = Vec::new();
    while let Some(chunk) = stream
        .message()
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?
    {
        chunks.push(TaskLogChunk {
            run_id: chunk.run_id,
            phase: chunk.phase,
            content: chunk.content,
            started_at: chunk.started_at,
        });
    }
    Ok(chunks)
}
