use crate::collab::MessageBus;
use crate::config::ActiveConfig;
use crate::events::EventSink;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use tokio::process::Child;
use tokio::sync::Mutex;

pub const MAX_CONCURRENT_TASKS: usize = 10;

lazy_static::lazy_static! {
    pub static ref TASK_SEMAPHORE: Arc<tokio::sync::Semaphore> = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TASKS));
}

#[derive(Clone)]
pub struct ManagedState {
    pub inner: Arc<InnerState>,
}

pub struct InnerState {
    pub app_root: PathBuf,
    pub db_path: PathBuf,
    pub logs_dir: PathBuf,
    pub active_config: RwLock<ActiveConfig>,
    pub running: Mutex<HashMap<String, RunningTask>>,
    pub agent_health: std::sync::RwLock<HashMap<String, crate::metrics::AgentHealthState>>,
    pub agent_metrics: std::sync::RwLock<HashMap<String, crate::metrics::AgentMetrics>>,
    pub message_bus: Arc<MessageBus>,
    pub event_sink: std::sync::RwLock<Arc<dyn EventSink>>,
    pub db_writer: Arc<crate::db_write::DbWriteCoordinator>,
}

impl InnerState {
    pub fn emit_event(
        &self,
        task_id: &str,
        task_item_id: Option<&str>,
        event_type: &str,
        payload: Value,
    ) {
        if let Ok(sink) = self.event_sink.read() {
            sink.emit(task_id, task_item_id, event_type, payload);
        }
    }
}

#[derive(Clone)]
pub struct RunningTask {
    pub stop_flag: Arc<AtomicBool>,
    pub child: Arc<Mutex<Option<Child>>>,
}

impl Default for RunningTask {
    fn default() -> Self {
        Self::new()
    }
}

impl RunningTask {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
        }
    }
}

pub fn write_active_config<'a>(
    state: &'a InnerState,
) -> Result<std::sync::RwLockWriteGuard<'a, ActiveConfig>, anyhow::Error> {
    state
        .active_config
        .write()
        .map_err(|_| anyhow::anyhow!("active config lock is poisoned"))
}
