use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::params;
use serde_json::Value;

/// Trait for emitting real-time events to listeners (UI, logging, etc.)
/// Separate from `insert_event` which persists to DB.
pub trait EventSink: Send + Sync {
    fn emit(&self, task_id: &str, task_item_id: Option<&str>, event_type: &str, payload: Value);
}

/// No-op implementation for CLI mode - events are persisted to DB but not pushed to any UI.
pub struct NoopSink;

impl EventSink for NoopSink {
    fn emit(
        &self,
        _task_id: &str,
        _task_item_id: Option<&str>,
        _event_type: &str,
        _payload: Value,
    ) {
    }
}

#[cfg(feature = "ui")]
pub struct TauriSink {
    app: tauri::AppHandle,
}

#[cfg(feature = "ui")]
impl TauriSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

#[cfg(feature = "ui")]
impl EventSink for TauriSink {
    fn emit(&self, task_id: &str, task_item_id: Option<&str>, event_type: &str, payload: Value) {
        use tauri::Manager;

        let _ = self.app.emit_all(
            "task-event",
            serde_json::json!({
                "task_id": task_id,
                "task_item_id": task_item_id,
                "event_type": event_type,
                "payload": payload,
                "ts": now_ts()
            }),
        );
    }
}

pub fn insert_event(
    state: &InnerState,
    task_id: &str,
    task_item_id: Option<&str>,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    let conn = open_conn(&state.db_path)?;
    conn.execute(
        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task_id,
            task_item_id,
            event_type,
            serde_json::to_string(&payload)?,
            now_ts()
        ],
    )?;
    Ok(())
}
