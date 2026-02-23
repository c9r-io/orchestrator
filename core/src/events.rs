use crate::state::InnerState;
use anyhow::Result;
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
