use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

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
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let resp = client
        .task_follow(orchestrator_proto::TaskFollowRequest {
            task_id: task_id.clone(),
        })
        .await
        .map_err(|e| e.message().to_string())?;

    let mut stream = resp.into_inner();
    let cancel = state.register_stream(&task_id).await;
    let event_name = format!("task-follow-{}", task_id);

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
                        Err(_) => break,
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
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    state.cancel_stream(&task_id).await;
    Ok(())
}
