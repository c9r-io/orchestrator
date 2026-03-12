use crate::async_database::flatten_err;
use crate::db::open_conn;
use crate::state::InnerState;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedStepScope {
    /// Event belongs to a task-scoped step.
    Task,
    /// Event belongs to an item-scoped step.
    Item,
}

/// Extracts the observed step scope from an event payload.
pub fn observed_step_scope_from_payload(payload: &Value) -> Option<ObservedStepScope> {
    match payload["step_scope"].as_str() {
        Some("task") => Some(ObservedStepScope::Task),
        Some("item") => Some(ObservedStepScope::Item),
        _ => None,
    }
}

/// Returns the stable label used for a step scope in logs and APIs.
pub fn observed_step_scope_label(scope: Option<ObservedStepScope>) -> &'static str {
    match scope {
        Some(ObservedStepScope::Task) => "task",
        Some(ObservedStepScope::Item) => "item",
        None => "unspecified",
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

/// Event sink that forwards workflow events into structured tracing logs.
pub struct TracingEventSink;

impl TracingEventSink {
    /// Creates a tracing-backed event sink.
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

/// Persists one workflow event using the shared async database writer.
pub async fn insert_event(
    state: &InnerState,
    task_id: &str,
    task_item_id: Option<&str>,
    event_type: &str,
    payload: Value,
) -> Result<()> {
    state
        .db_writer
        .insert_event(
            task_id,
            task_item_id,
            event_type,
            &serde_json::to_string(&payload)?,
        )
        .await
}

/// Parsed step event from the events table for display in watch/follow.
#[derive(Debug)]
pub struct StepEvent {
    /// Event type label.
    pub event_type: String,
    /// Step identifier or phase name associated with the event.
    pub step: Option<String>,
    /// Scope inferred from promoted columns or payload JSON.
    pub step_scope: Option<ObservedStepScope>,
    /// Task-item identifier for item-scoped events.
    pub task_item_id: Option<String>,
    /// Agent identifier when an agent executed the step.
    pub agent_id: Option<String>,
    /// Success flag captured from the payload.
    pub success: Option<bool>,
    /// Step duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Confidence score reported by the agent.
    pub confidence: Option<f64>,
    /// Human-readable reason or message from the payload.
    pub reason: Option<String>,
    /// Elapsed seconds reported by heartbeat events.
    pub elapsed_secs: Option<u64>,
    /// Total stdout bytes written so far.
    pub stdout_bytes: Option<u64>,
    /// Total stderr bytes written so far.
    pub stderr_bytes: Option<u64>,
    /// Stdout growth since the previous heartbeat.
    pub stdout_delta_bytes: Option<u64>,
    /// Stderr growth since the previous heartbeat.
    pub stderr_delta_bytes: Option<u64>,
    /// Number of consecutive stagnant heartbeat samples.
    pub stagnant_heartbeats: Option<u32>,
    /// Child process identifier when tracked.
    pub pid: Option<u32>,
    /// Whether the child process was still alive at sample time.
    pub pid_alive: Option<bool>,
    /// Output-state classification attached to the event.
    pub output_state: Option<String>,
    /// Timestamp when the event row was created.
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

fn query_step_events_with_conn(conn: &Connection, task_id: &str) -> Result<Vec<StepEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_type, payload_json, created_at, task_item_id, step, step_scope FROM events
         WHERE task_id = ?1
           AND event_type IN ('step_started', 'step_finished', 'step_skipped', 'step_heartbeat', 'step_spawned', 'step_timeout', 'cycle_started', 'sandbox_denied', 'sandbox_resource_exceeded', 'sandbox_network_blocked')
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        let event_type: String = row.get(0)?;
        let payload_json: String = row.get(1)?;
        let created_at: String = row.get(2)?;
        let task_item_id: Option<String> = row.get(3)?;
        let col_step: Option<String> = row.get(4)?;
        let col_step_scope: Option<String> = row.get(5)?;
        Ok((
            event_type,
            payload_json,
            created_at,
            task_item_id,
            col_step,
            col_step_scope,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (event_type, payload_json, created_at, task_item_id, col_step, col_step_scope) = row?;
        let v: Value = serde_json::from_str(&payload_json).unwrap_or_default();

        // Use promoted column values first, fall back to JSON parsing
        let step = col_step.or_else(|| {
            v["step"]
                .as_str()
                .or_else(|| v["phase"].as_str())
                .map(String::from)
        });
        let step_scope = if let Some(ref scope_str) = col_step_scope {
            match scope_str.as_str() {
                "task" => Some(ObservedStepScope::Task),
                "item" => Some(ObservedStepScope::Item),
                _ => None,
            }
        } else {
            observed_step_scope_from_payload(&v)
        };

        events.push(StepEvent {
            event_type,
            step,
            step_scope,
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

/// Async variant of [`query_latest_step_log_paths`] backed by the shared async reader.
pub async fn query_latest_step_log_paths_async(
    state: &InnerState,
    task_id: &str,
) -> Result<Option<(String, String, String)>> {
    let task_id = task_id.to_owned();
    state
        .async_database
        .reader()
        .call(move |conn| {
            query_latest_step_log_paths_with_conn(conn, &task_id)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(flatten_err)
}

/// Async variant of [`query_step_events`] backed by the shared async reader.
pub async fn query_step_events_async(state: &InnerState, task_id: &str) -> Result<Vec<StepEvent>> {
    let task_id = task_id.to_owned();
    state
        .async_database
        .reader()
        .call(move |conn| {
            query_step_events_with_conn(conn, &task_id)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(flatten_err)
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

    #[tokio::test]
    async fn insert_event_and_query_roundtrip() {
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
        .await
        .expect("insert step_started event");

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_finished",
            serde_json::json!({"step": "qa", "success": true, "duration_ms": 1500}),
        )
        .await
        .expect("insert step_finished event");

        insert_event(
            &state,
            "task1",
            None,
            "cycle_started",
            serde_json::json!({"cycle": 1}),
        )
        .await
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
    fn observed_step_scope_label_returns_unspecified_for_none() {
        assert_eq!(observed_step_scope_label(None), "unspecified");
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

    #[tokio::test]
    async fn query_latest_step_log_paths_returns_paths() {
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
        .await
        .expect("insert step_spawned event");

        let result = query_latest_step_log_paths(&state.db_path, "task1")
            .expect("query latest spawned log paths");
        assert!(result.is_some());
        let (phase, stdout, stderr) = result.expect("spawned log paths should exist");
        assert_eq!(phase, "qa");
        assert_eq!(stdout, "/tmp/stdout.log");
        assert_eq!(stderr, "/tmp/stderr.log");
    }

    #[tokio::test]
    async fn query_latest_step_log_paths_empty_phase_returns_none() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"stdout_path": "/tmp/out.log"}),
        )
        .await
        .expect("insert step_started log event");

        let result = query_latest_step_log_paths(&state.db_path, "task1")
            .expect("query empty phase log paths");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn query_step_events_parses_step_scope_and_task_item_id() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"step": "qa", "step_scope": "item"}),
        )
        .await
        .expect("insert scoped step_started event");

        let events = query_step_events(&state.db_path, "task1").expect("query scoped events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].step_scope, Some(ObservedStepScope::Item));
        assert_eq!(events[0].task_item_id.as_deref(), Some("item1"));
    }

    #[test]
    fn tracing_event_sink_does_not_panic_on_all_event_types() {
        let sink = TracingEventSink::new();
        // Error level
        sink.emit(
            "t1",
            None,
            "task_failed",
            serde_json::json!({"error": "boom"}),
        );
        // Warning level
        sink.emit("t1", None, "step_timeout", serde_json::json!({"secs": 60}));
        sink.emit("t1", None, "auto_rollback_failed", serde_json::json!({}));
        // Info level
        sink.emit("t1", Some("i1"), "step_started", serde_json::json!({}));
        sink.emit("t1", None, "step_finished", serde_json::json!({}));
        sink.emit("t1", None, "task_completed", serde_json::json!({}));
        sink.emit("t1", None, "task_paused", serde_json::json!({}));
        // Debug level (fallthrough)
        sink.emit("t1", None, "step_heartbeat", serde_json::json!({}));
        sink.emit("t1", None, "custom_event", serde_json::json!({}));
    }

    #[test]
    fn tracing_event_sink_default_impl() {
        let sink = TracingEventSink;
        sink.emit("t1", None, "task_completed", serde_json::json!({}));
    }

    #[test]
    fn observed_step_scope_from_payload_unknown_value() {
        assert_eq!(
            observed_step_scope_from_payload(&serde_json::json!({"step_scope": "unknown"})),
            None
        );
    }

    #[tokio::test]
    async fn query_latest_step_log_paths_prefers_step_key_over_phase() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({
                "step": "implement",
                "stdout_path": "/tmp/out.log",
                "stderr_path": "/tmp/err.log"
            }),
        )
        .await
        .expect("insert step_started event");

        let result = query_latest_step_log_paths(&state.db_path, "task1")
            .expect("query log paths with step key");
        assert!(result.is_some());
        let (phase, stdout, _) = result.expect("log paths should exist");
        assert_eq!(phase, "implement");
        assert_eq!(stdout, "/tmp/out.log");
    }

    #[tokio::test]
    async fn query_step_events_uses_promoted_column_scope() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Insert an event where the promoted step_scope column differs from JSON payload
        insert_event(
            &state,
            "task1",
            Some("item1"),
            "step_started",
            serde_json::json!({"step": "qa", "step_scope": "task"}),
        )
        .await
        .expect("insert step_started event");

        let events = query_step_events(&state.db_path, "task1").expect("query events");
        assert_eq!(events.len(), 1);
        // The promoted column should take precedence
        assert_eq!(events[0].step_scope, Some(ObservedStepScope::Task));
    }

    #[tokio::test]
    async fn step_event_parses_all_optional_fields() {
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
        .await
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
