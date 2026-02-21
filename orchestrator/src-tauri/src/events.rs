use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::state::InnerState;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Manager};

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

pub fn emit_event(
    app: &AppHandle,
    task_id: &str,
    task_item_id: Option<&str>,
    event_type: &str,
    payload: Value,
) {
    let _ = app.emit_all(
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
