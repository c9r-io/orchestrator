use crate::database::Database;
use crate::db::open_conn;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedStepScope {
    Task,
    Item,
}

pub fn observed_step_scope_from_payload(payload: &Value) -> Option<ObservedStepScope> {
    match payload["step_scope"].as_str() {
        Some("task") => Some(ObservedStepScope::Task),
        Some("item") => Some(ObservedStepScope::Item),
        _ => None,
    }
}

pub fn observed_step_scope_label(scope: Option<ObservedStepScope>) -> &'static str {
    match scope {
        Some(ObservedStepScope::Task) => "task",
        Some(ObservedStepScope::Item) => "item",
        None => "legacy",
    }
}

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

pub struct TracingEventSink;

impl TracingEventSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TracingEventSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for TracingEventSink {
    fn emit(&self, task_id: &str, task_item_id: Option<&str>, event_type: &str, payload: Value) {
        let payload_text = payload.to_string();
        match event_type {
            "task_failed" => error!(
                task_id,
                task_item_id,
                event_type,
                payload = %payload_text,
                "workflow event"
            ),
            "step_timeout" | "auto_rollback_failed" => warn!(
                task_id,
                task_item_id,
                event_type,
                payload = %payload_text,
                "workflow event"
            ),
            "step_started" | "step_finished" | "task_completed" | "task_paused" => info!(
                task_id,
                task_item_id,
                event_type,
                payload = %payload_text,
                "workflow event"
            ),
            _ => debug!(
                task_id,
                task_item_id,
                event_type,
                payload = %payload_text,
                "workflow event"
            ),
        }
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
    pub step_scope: Option<ObservedStepScope>,
    pub task_item_id: Option<String>,
    pub agent_id: Option<String>,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub elapsed_secs: Option<u64>,
    pub stdout_bytes: Option<u64>,
    pub stderr_bytes: Option<u64>,
    pub stdout_delta_bytes: Option<u64>,
    pub stderr_delta_bytes: Option<u64>,
    pub stagnant_heartbeats: Option<u32>,
    pub pid: Option<u32>,
    pub pid_alive: Option<bool>,
    pub output_state: Option<String>,
    pub created_at: String,
}

/// Query the latest step's log file paths for real-time tailing.
/// Returns (phase, stdout_path, stderr_path) from the most recent step_spawned event.
pub fn query_latest_step_log_paths(
    db_path: &Path,
    task_id: &str,
) -> Result<Option<(String, String, String)>> {
    let conn = open_conn(db_path)?;
    query_latest_step_log_paths_with_conn(&conn, task_id)
}

pub fn query_latest_step_log_paths_db(
    database: &Database,
    task_id: &str,
) -> Result<Option<(String, String, String)>> {
    let conn = database.connection()?;
    query_latest_step_log_paths_with_conn(&conn, task_id)
}

fn query_latest_step_log_paths_with_conn(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<(String, String, String)>> {
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
    query_step_events_with_conn(&conn, task_id)
}

pub fn query_step_events_db(database: &Database, task_id: &str) -> Result<Vec<StepEvent>> {
    let conn = database.connection()?;
    query_step_events_with_conn(&conn, task_id)
}

fn query_step_events_with_conn(conn: &Connection, task_id: &str) -> Result<Vec<StepEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_type, payload_json, created_at, task_item_id FROM events
         WHERE task_id = ?1
           AND event_type IN ('step_started', 'step_finished', 'step_skipped', 'step_heartbeat', 'step_spawned', 'step_timeout', 'cycle_started')
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        let event_type: String = row.get(0)?;
        let payload_json: String = row.get(1)?;
        let created_at: String = row.get(2)?;
        let task_item_id: Option<String> = row.get(3)?;
        Ok((event_type, payload_json, created_at, task_item_id))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (event_type, payload_json, created_at, task_item_id) = row?;
        let v: Value = serde_json::from_str(&payload_json).unwrap_or_default();
        events.push(StepEvent {
            event_type,
            step: v["step"]
                .as_str()
                .or_else(|| v["phase"].as_str())
                .map(String::from),
            step_scope: observed_step_scope_from_payload(&v),
            task_item_id,
            agent_id: v["agent_id"].as_str().map(String::from),
            success: v["success"].as_bool(),
            duration_ms: v["duration_ms"].as_u64(),
            confidence: v["confidence"].as_f64(),
            reason: v["reason"].as_str().map(String::from),
            elapsed_secs: v["elapsed_secs"].as_u64(),
            stdout_bytes: v["stdout_bytes"].as_u64(),
            stderr_bytes: v["stderr_bytes"].as_u64(),
            stdout_delta_bytes: v["stdout_delta_bytes"].as_u64(),
            stderr_delta_bytes: v["stderr_delta_bytes"].as_u64(),
            stagnant_heartbeats: v["stagnant_heartbeats"].as_u64().map(|v| v as u32),
            pid: v["pid"].as_u64().map(|p| p as u32),
            pid_alive: v["pid_alive"].as_bool(),
            output_state: v["output_state"].as_str().map(String::from),
            created_at,
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_sink_does_not_panic() {
        let sink = NoopSink;
        sink.emit(
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({}),
        );
        sink.emit(
            "task1",
            None,
            "task_completed",
            serde_json::json!({"status": "ok"}),
        );
    }

    #[test]
    fn insert_event_and_query_roundtrip() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Insert events
        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"step": "qa", "agent_id": "qa_agent"}),
        )
        .expect("insert step_started event");

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_finished",
            serde_json::json!({"step": "qa", "success": true, "duration_ms": 1500}),
        )
        .expect("insert step_finished event");

        insert_event(
            &state,
            "task1",
            None,
            "cycle_started",
            serde_json::json!({"cycle": 1}),
        )
        .expect("insert cycle_started event");

        // Query events back
        let events = query_step_events(&state.db_path, "task1").expect("query roundtrip events");
        assert_eq!(events.len(), 3);

        assert_eq!(events[0].event_type, "step_started");
        assert_eq!(events[0].step.as_deref(), Some("qa"));
        assert_eq!(events[0].task_item_id.as_deref(), Some("item1"));
        assert_eq!(events[0].agent_id.as_deref(), Some("qa_agent"));

        assert_eq!(events[1].event_type, "step_finished");
        assert_eq!(events[1].success, Some(true));
        assert_eq!(events[1].duration_ms, Some(1500));

        assert_eq!(events[2].event_type, "cycle_started");
    }

    #[test]
    fn query_step_events_empty_for_unknown_task() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        let events =
            query_step_events(&state.db_path, "nonexistent_task").expect("query empty events");
        assert!(events.is_empty());
    }

    #[test]
    fn observed_step_scope_parses_known_values() {
        assert_eq!(
            observed_step_scope_from_payload(&serde_json::json!({"step_scope": "task"})),
            Some(ObservedStepScope::Task)
        );
        assert_eq!(
            observed_step_scope_from_payload(&serde_json::json!({"step_scope": "item"})),
            Some(ObservedStepScope::Item)
        );
        assert_eq!(
            observed_step_scope_from_payload(&serde_json::json!({})),
            None
        );
    }

    #[test]
    fn observed_step_scope_label_returns_legacy_for_none() {
        assert_eq!(observed_step_scope_label(None), "legacy");
        assert_eq!(
            observed_step_scope_label(Some(ObservedStepScope::Task)),
            "task"
        );
        assert_eq!(
            observed_step_scope_label(Some(ObservedStepScope::Item)),
            "item"
        );
    }

    #[test]
    fn query_latest_step_log_paths_returns_none_when_empty() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        let result =
            query_latest_step_log_paths(&state.db_path, "task1").expect("query latest log paths");
        assert!(result.is_none());
    }

    #[test]
    fn query_latest_step_log_paths_returns_paths() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_spawned",
            serde_json::json!({
                "phase": "qa",
                "stdout_path": "/tmp/stdout.log",
                "stderr_path": "/tmp/stderr.log"
            }),
        )
        .expect("insert step_spawned event");

        let result = query_latest_step_log_paths(&state.db_path, "task1")
            .expect("query latest spawned log paths");
        assert!(result.is_some());
        let (phase, stdout, stderr) = result.expect("spawned log paths should exist");
        assert_eq!(phase, "qa");
        assert_eq!(stdout, "/tmp/stdout.log");
        assert_eq!(stderr, "/tmp/stderr.log");
    }

    #[test]
    fn query_latest_step_log_paths_empty_phase_returns_none() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"stdout_path": "/tmp/out.log"}),
        )
        .expect("insert step_started log event");

        let result = query_latest_step_log_paths(&state.db_path, "task1")
            .expect("query empty phase log paths");
        assert!(result.is_none());
    }

    #[test]
    fn query_step_events_parses_step_scope_and_task_item_id() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"step": "qa", "step_scope": "item"}),
        )
        .expect("insert scoped step_started event");

        let events = query_step_events(&state.db_path, "task1").expect("query scoped events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].step_scope, Some(ObservedStepScope::Item));
        assert_eq!(events[0].task_item_id.as_deref(), Some("item1"));
    }

    #[test]
    fn step_event_parses_all_optional_fields() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            None,
            "step_heartbeat",
            serde_json::json!({
                "step": "implement",
                "step_scope": "task",
                "elapsed_secs": 120,
                "stdout_bytes": 4096,
                "stderr_bytes": 256,
                "stdout_delta_bytes": 0,
                "stderr_delta_bytes": 4,
                "stagnant_heartbeats": 3,
                "pid": 12345,
                "pid_alive": true,
                "output_state": "low_output"
            }),
        )
        .expect("insert step_heartbeat event");

        let events = query_step_events(&state.db_path, "task1").expect("query heartbeat events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].step_scope, Some(ObservedStepScope::Task));
        assert_eq!(events[0].elapsed_secs, Some(120));
        assert_eq!(events[0].stdout_bytes, Some(4096));
        assert_eq!(events[0].stderr_bytes, Some(256));
        assert_eq!(events[0].stdout_delta_bytes, Some(0));
        assert_eq!(events[0].stderr_delta_bytes, Some(4));
        assert_eq!(events[0].stagnant_heartbeats, Some(3));
        assert_eq!(events[0].pid, Some(12345));
        assert_eq!(events[0].pid_alive, Some(true));
        assert_eq!(events[0].output_state.as_deref(), Some("low_output"));
    }
}
