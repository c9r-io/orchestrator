use crate::db::open_conn;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::params;
use serde_json::Value;
use std::path::Path;

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

pub fn insert_event(
    state: &InnerState,
    task_id: &str,
    task_item_id: Option<&str>,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    state.db_writer.insert_event(
        task_id,
        task_item_id,
        event_type,
        &serde_json::to_string(&payload)?,
    )
}

/// Parsed step event from the events table for display in watch/follow.
#[derive(Debug)]
pub struct StepEvent {
    pub event_type: String,
    pub step: Option<String>,
    pub agent_id: Option<String>,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub stdout_bytes: Option<u64>,
    pub pid: Option<u32>,
    pub pid_alive: Option<bool>,
    pub created_at: String,
}

/// Query the latest step's log file paths for real-time tailing.
/// Returns (phase, stdout_path, stderr_path) from the most recent step_spawned event.
pub fn query_latest_step_log_paths(
    db_path: &Path,
    task_id: &str,
) -> Result<Option<(String, String, String)>> {
    let conn = open_conn(db_path)?;
    let result: Option<(String,)> = conn
        .query_row(
            "SELECT payload_json FROM events
             WHERE task_id = ?1 AND event_type IN ('step_spawned', 'step_started')
             ORDER BY id DESC LIMIT 1",
            params![task_id],
            |row| Ok((row.get::<_, String>(0)?,)),
        )
        .ok();

    match result {
        Some((payload_json,)) => {
            let v: Value = serde_json::from_str(&payload_json).unwrap_or_default();
            let phase = v["phase"]
                .as_str()
                .or_else(|| v["step"].as_str())
                .unwrap_or("")
                .to_string();
            let stdout = v["stdout_path"].as_str().unwrap_or("").to_string();
            let stderr = v["stderr_path"].as_str().unwrap_or("").to_string();
            if phase.is_empty() || stdout.is_empty() {
                Ok(None)
            } else {
                Ok(Some((phase, stdout, stderr)))
            }
        }
        None => Ok(None),
    }
}

/// Query all step-related events for a task, parsed into StepEvent structs.
pub fn query_step_events(db_path: &Path, task_id: &str) -> Result<Vec<StepEvent>> {
    let conn = open_conn(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT event_type, payload_json, created_at FROM events
         WHERE task_id = ?1
           AND event_type IN ('step_started', 'step_finished', 'step_skipped', 'step_heartbeat', 'step_spawned', 'step_timeout', 'cycle_started')
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        let event_type: String = row.get(0)?;
        let payload_json: String = row.get(1)?;
        let created_at: String = row.get(2)?;
        Ok((event_type, payload_json, created_at))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (event_type, payload_json, created_at) = row?;
        let v: Value = serde_json::from_str(&payload_json).unwrap_or_default();
        events.push(StepEvent {
            event_type,
            step: v["step"].as_str().or_else(|| v["phase"].as_str()).map(String::from),
            agent_id: v["agent_id"].as_str().map(String::from),
            success: v["success"].as_bool(),
            duration_ms: v["duration_ms"].as_u64(),
            confidence: v["confidence"].as_f64(),
            reason: v["reason"].as_str().map(String::from),
            stdout_bytes: v["stdout_bytes"].as_u64(),
            pid: v["pid"].as_u64().map(|p| p as u32),
            pid_alive: v["pid_alive"].as_bool(),
            created_at,
        });
    }
    Ok(events)
}
