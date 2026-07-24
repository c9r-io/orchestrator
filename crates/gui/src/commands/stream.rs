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

#[derive(Debug, Clone, Serialize)]
pub struct TimelineDelta {
    pub kind: String,
    pub entry: Option<super::task::TimelineEntry>,
    pub snapshot_max_event_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionDelta {
    pub kind: String,
    pub change_id: i64,
    pub item: Option<super::attention::AttentionItem>,
    pub notification: Option<AttentionNotification>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionNotification {
    pub dedupe_key: String,
    pub attention_item_id: String,
    pub item_version: i64,
    pub title: String,
    pub severity: String,
    pub process_id: String,
    pub deep_link: String,
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
pub async fn start_attention_follow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    after_change_id: Option<i64>,
    project_id: Option<String>,
    item_state: Option<String>,
    kind: Option<String>,
    severity: Option<String>,
    assignee: Option<String>,
    task_id: Option<String>,
    active_only: Option<bool>,
    native_notifications_enabled: Option<bool>,
) -> Result<(), crate::errors::SafeGrpcError> {
    let mut client = state.client().await?;
    let response = client
        .attention_follow(orchestrator_proto::AttentionFollowRequest {
            after_change_id: after_change_id.unwrap_or_default(),
            project_id,
            interval_millis: 500,
            state: item_state,
            kind,
            severity,
            assignee,
            task_id,
            active_only: active_only.unwrap_or(false),
        })
        .await
        .map_err(|error| crate::errors::safe_grpc_error(&error))?;
    let mut stream = response.into_inner();
    let cancel = state.register_stream("attention").await;
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                message = stream.message() => {
                    match message {
                        Ok(Some(delta)) => {
                            let notification = delta.notification.map(|value| AttentionNotification {
                                dedupe_key: value.dedupe_key,
                                attention_item_id: value.attention_item_id,
                                item_version: value.item_version,
                                title: value.title,
                                severity: value.severity,
                                process_id: value.process_id,
                                deep_link: value.deep_link,
                            });
                            let payload = AttentionDelta {
                                kind: delta.kind,
                                change_id: delta.change_id,
                                item: delta.item.map(super::attention::item_from_proto),
                                notification: notification.clone(),
                            };
                            let _ = app.emit("attention-delta", &payload);
                            if let Some(notification) = notification {
                                let first_delivery = app_state
                                    .record_attention_notification(notification.dedupe_key.clone())
                                    .await;
                                if first_delivery {
                                    if native_notifications_enabled.unwrap_or(false) {
                                        let body = format!(
                                            "{} · process {}",
                                            notification.severity, notification.process_id
                                        );
                                        let result = app
                                            .notification()
                                            .builder()
                                            .title(&notification.title)
                                            .body(body)
                                            .extra("deep_link", &notification.deep_link)
                                            .show();
                                        if result.is_err() {
                                            let _ = app.emit(
                                                "attention-notification-fallback",
                                                "Desktop notification failed; in-app Attention remains active.",
                                            );
                                        }
                                    } else {
                                        let _ = app.emit(
                                            "attention-notification-fallback",
                                            "Desktop notifications are unavailable; in-app Attention remains active.",
                                        );
                                    }
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let safe_error = crate::errors::safe_grpc_error(&error);
                            let _ = app.emit("stream-error-attention", &safe_error);
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

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_attention_follow(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.cancel_stream("attention").await;
    Ok(())
}

/// Start streaming semantic timeline updates via Tauri events.
#[tauri::command(rename_all = "snake_case")]
pub async fn start_task_timeline_follow(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    task_id: String,
    after_event_id: Option<i64>,
    categories: Option<Vec<String>>,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let response = client
        .task_timeline_follow(orchestrator_proto::TaskTimelineFollowRequest {
            task_id: task_id.clone(),
            after_event_id: after_event_id.unwrap_or_default(),
            categories: categories.unwrap_or_default(),
            interval_millis: 500,
        })
        .await
        .map_err(|e| crate::errors::humanize_grpc_error(&e))?;

    let mut stream = response.into_inner();
    let stream_key = format!("timeline-{task_id}");
    let cancel = state.register_stream(&stream_key).await;
    let event_name = format!("task-timeline-{task_id}");
    let error_event = format!("stream-error-timeline-{task_id}");
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                message = stream.message() => {
                    match message {
                        Ok(Some(delta)) => {
                            let payload = TimelineDelta {
                                kind: delta.kind,
                                entry: delta.entry.map(super::task::timeline_entry_from_proto),
                                snapshot_max_event_id: delta.snapshot_max_event_id,
                            };
                            let _ = app.emit(&event_name, &payload);
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let message = crate::errors::humanize_grpc_error(&error);
                            let _ = app.emit(&error_event, &message);
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

/// Stop streaming semantic timeline updates.
#[tauri::command(rename_all = "snake_case")]
pub async fn stop_task_timeline_follow(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<(), String> {
    state.cancel_stream(&format!("timeline-{task_id}")).await;
    Ok(())
}

/// Start streaming task logs via Tauri events.
///
/// Each log line is emitted as a `task-follow-{task_id}` event.
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
                                        item_kind: super::task::item_kind(&i.qa_file_path).to_string(),
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
#[tauri::command(rename_all = "snake_case")]
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
#[tauri::command(rename_all = "snake_case")]
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
