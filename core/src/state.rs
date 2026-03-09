use crate::collab::MessageBus;
use crate::config::ActiveConfig;
use crate::config_load::ConfigSelfHealReport;
use crate::events::EventSink;
use crate::metrics::{AgentHealthState, AgentMetrics};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::process::Child;
use tokio::sync::Mutex;

pub const MAX_CONCURRENT_TASKS: usize = 10;

static TASK_SEMAPHORE: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

pub fn task_semaphore() -> &'static Arc<tokio::sync::Semaphore> {
    TASK_SEMAPHORE.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TASKS)))
}

#[derive(Clone)]
pub struct ManagedState {
    pub inner: Arc<InnerState>,
}

pub struct InnerState {
    pub app_root: PathBuf,
    pub db_path: PathBuf,
    pub unsafe_mode: bool,
    pub async_database: Arc<crate::async_database::AsyncDatabase>,
    pub logs_dir: PathBuf,
    pub active_config: RwLock<ActiveConfig>,
    pub active_config_error: RwLock<Option<String>>,
    pub active_config_notice: RwLock<Option<ConfigSelfHealReport>>,
    pub running: Mutex<HashMap<String, RunningTask>>,
    pub agent_health: std::sync::RwLock<HashMap<String, crate::metrics::AgentHealthState>>,
    pub agent_metrics: std::sync::RwLock<HashMap<String, crate::metrics::AgentMetrics>>,
    pub message_bus: Arc<MessageBus>,
    pub event_sink: std::sync::RwLock<Arc<dyn EventSink>>,
    pub db_writer: Arc<crate::db_write::DbWriteCoordinator>,
    pub session_store: Arc<crate::session_store::AsyncSessionStore>,
    pub task_repo: Arc<crate::task_repository::AsyncSqliteTaskRepository>,
    pub store_manager: crate::store::StoreManager,
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

    /// Create a sibling that shares `stop_flag` but has its own `child` slot.
    /// Used for parallel item execution — stopping the task sets the shared flag,
    /// which all forked items observe.
    pub fn fork(&self) -> Self {
        Self {
            stop_flag: Arc::clone(&self.stop_flag),
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

pub fn read_agent_health<'a>(
    state: &'a InnerState,
) -> RwLockReadGuard<'a, HashMap<String, AgentHealthState>> {
    match state.agent_health.read() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

pub fn write_agent_health<'a>(
    state: &'a InnerState,
) -> RwLockWriteGuard<'a, HashMap<String, AgentHealthState>> {
    match state.agent_health.write() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

pub fn read_agent_metrics<'a>(
    state: &'a InnerState,
) -> RwLockReadGuard<'a, HashMap<String, AgentMetrics>> {
    match state.agent_metrics.read() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

pub fn write_agent_metrics<'a>(
    state: &'a InnerState,
) -> RwLockWriteGuard<'a, HashMap<String, AgentMetrics>> {
    match state.agent_metrics.write() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;
    use std::sync::atomic::Ordering;

    #[test]
    fn running_task_starts_with_defaults() {
        let runtime = RunningTask::new();
        assert!(!runtime.stop_flag.load(Ordering::SeqCst));
        assert!(runtime.child.try_lock().expect("lock child").is_none());
    }

    #[test]
    fn state_accessors_round_trip_agent_maps() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        write_agent_health(&state).insert(
            "echo".to_string(),
            AgentHealthState {
                consecutive_errors: 1,
                diseased_until: None,
                total_lifetime_errors: 1,
                capability_health: HashMap::new(),
            },
        );
        write_agent_metrics(&state).insert("echo".to_string(), AgentMetrics::default());

        assert!(read_agent_health(&state).contains_key("echo"));
        assert!(read_agent_metrics(&state).contains_key("echo"));
    }

    #[test]
    fn emit_event_is_safe_with_noop_sink() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        state.emit_event(
            "task-1",
            Some("item-1"),
            "heartbeat",
            serde_json::json!({"ok": true}),
        );
    }

    #[test]
    fn write_active_config_returns_mutable_guard() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let original = state
            .active_config
            .read()
            .expect("read active config")
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|p| p.workflows.keys().next())
            .cloned()
            .unwrap_or_default();
        let mut guard = write_active_config(&state).expect("lock active config");
        let workflow_clone = guard
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|p| p.workflows.get(&original))
            .cloned()
            .expect("default workflow should exist");
        guard.projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_insert_with(|| crate::config::ResolvedProject {
                workspaces: HashMap::new(),
                agents: HashMap::new(),
                workflows: HashMap::new(),
                step_templates: HashMap::new(),
                env_stores: HashMap::new(),
            })
            .workflows
            .insert(format!("{}-updated", original), workflow_clone);
        drop(guard);

        let updated_exists = state
            .active_config
            .read()
            .expect("re-read active config")
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .map(|p| p.workflows.contains_key(&format!("{}-updated", original)))
            .unwrap_or(false);
        assert!(updated_exists);
    }
}
