use crate::collab::MessageBus;
use crate::config::ActiveConfig;
use crate::config_load::ConfigSelfHealReport;
use crate::events::{EventSink, TracingEventSink};
use crate::metrics::{AgentHealthState, AgentMetrics};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::error;

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
        let sink = clone_event_sink(self);
        sink.emit(task_id, task_item_id, event_type, payload);
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
    write_lock_fail_closed(&state.active_config, "active_config")
}

pub fn read_agent_health<'a>(
    state: &'a InnerState,
) -> RwLockReadGuard<'a, HashMap<String, AgentHealthState>> {
    recoverable_read_lock(
        &state.agent_health,
        "agent_health",
        HashMap::new,
        Some(state),
    )
}

pub fn write_active_config_error<'a>(
    state: &'a InnerState,
) -> Result<RwLockWriteGuard<'a, Option<String>>, anyhow::Error> {
    write_lock_fail_closed(&state.active_config_error, "active_config_error")
}

pub fn write_active_config_notice<'a>(
    state: &'a InnerState,
) -> Result<RwLockWriteGuard<'a, Option<ConfigSelfHealReport>>, anyhow::Error> {
    write_lock_fail_closed(&state.active_config_notice, "active_config_notice")
}

pub fn clear_active_config_status(state: &InnerState) -> Result<(), anyhow::Error> {
    *write_active_config_error(state)? = None;
    *write_active_config_notice(state)? = None;
    Ok(())
}

pub fn reset_active_config_to_default(state: &InnerState) -> Result<(), anyhow::Error> {
    *write_active_config(state)? = ActiveConfig {
        config: Default::default(),
        workspaces: Default::default(),
        projects: Default::default(),
    };
    clear_active_config_status(state)
}

pub fn clone_event_sink(state: &InnerState) -> Arc<dyn EventSink> {
    recoverable_read_lock(
        &state.event_sink,
        "event_sink",
        || Arc::new(TracingEventSink::new()),
        None,
    )
    .clone()
}

pub fn replace_event_sink(state: &InnerState, sink: Arc<dyn EventSink>) {
    *recoverable_write_lock(&state.event_sink, "event_sink", || {
        Arc::new(TracingEventSink::new())
    }, None) = sink;
}

pub fn read_active_config_error<'a>(
    state: &'a InnerState,
) -> Result<RwLockReadGuard<'a, Option<String>>, anyhow::Error> {
    read_lock_fail_closed(&state.active_config_error, "active_config_error")
}

pub fn read_loaded_config_guard<'a>(
    state: &'a InnerState,
) -> Result<RwLockReadGuard<'a, ActiveConfig>, anyhow::Error> {
    read_lock_fail_closed(&state.active_config, "active_config")
}

pub fn read_active_config_notice<'a>(
    state: &'a InnerState,
) -> Result<RwLockReadGuard<'a, Option<ConfigSelfHealReport>>, anyhow::Error> {
    read_lock_fail_closed(&state.active_config_notice, "active_config_notice")
}

fn control_plane_lock_error(lock_name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{lock_name} lock poisoned; in-memory control-plane state corrupted; restart required"
    )
}

fn read_lock_fail_closed<'a, T>(
    lock: &'a RwLock<T>,
    lock_name: &str,
) -> Result<RwLockReadGuard<'a, T>, anyhow::Error> {
    lock.read()
        .map_err(|_| control_plane_lock_error(lock_name))
}

fn write_lock_fail_closed<'a, T>(
    lock: &'a RwLock<T>,
    lock_name: &str,
) -> Result<RwLockWriteGuard<'a, T>, anyhow::Error> {
    lock.write()
        .map_err(|_| control_plane_lock_error(lock_name))
}

fn recoverable_read_lock<'a, T>(
    lock: &'a RwLock<T>,
    lock_name: &str,
    reset: impl FnOnce() -> T,
    state: Option<&InnerState>,
) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(err) => {
            drop(err.into_inner());
            drop(recoverable_write_lock(lock, lock_name, reset, state));
            lock.read().unwrap_or_else(|err| err.into_inner())
        }
    }
}

pub fn write_agent_health<'a>(
    state: &'a InnerState,
) -> RwLockWriteGuard<'a, HashMap<String, AgentHealthState>> {
    recoverable_write_lock(&state.agent_health, "agent_health", HashMap::new, Some(state))
}

pub fn read_agent_metrics<'a>(
    state: &'a InnerState,
) -> RwLockReadGuard<'a, HashMap<String, AgentMetrics>> {
    recoverable_read_lock(
        &state.agent_metrics,
        "agent_metrics",
        HashMap::new,
        Some(state),
    )
}

pub fn write_agent_metrics<'a>(
    state: &'a InnerState,
) -> RwLockWriteGuard<'a, HashMap<String, AgentMetrics>> {
    recoverable_write_lock(
        &state.agent_metrics,
        "agent_metrics",
        HashMap::new,
        Some(state),
    )
}

fn recoverable_write_lock<'a, T>(
    lock: &'a RwLock<T>,
    lock_name: &str,
    reset: impl FnOnce() -> T,
    state: Option<&InnerState>,
) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(err) => {
            error!(
                lock_name,
                policy = "reset_and_continue",
                "lock poisoned; resetting in-memory state"
            );
            let mut guard = err.into_inner();
            *guard = reset();
            lock.clear_poison();
            if let Some(state) = state {
                state.emit_event(
                    "",
                    None,
                    "lock_poison_recovered",
                    serde_json::json!({
                        "lock_name": lock_name,
                        "policy": "reset_and_continue",
                        "state_dropped": true
                    }),
                );
            }
            guard
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::{read_active_config, read_loaded_config};
    use crate::test_utils::TestState;
    use std::sync::atomic::Ordering;
    use std::thread;

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
        guard
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_insert_with(|| crate::config::ResolvedProject {
                workspaces: HashMap::new(),
                agents: HashMap::new(),
                workflows: HashMap::new(),
                step_templates: HashMap::new(),
                env_stores: HashMap::new(),
                execution_profiles: HashMap::new(),
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

    #[test]
    fn poisoned_active_config_read_fails_closed() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let result = thread::spawn({
            let state = state.clone();
            move || {
            let _guard = state.active_config.write().expect("lock active config");
                panic!("poison active config");
            }
        });
        assert!(result.join().is_err());

        let error = read_loaded_config(&state).expect_err("poisoned config should fail");
        assert!(error.to_string().contains("active_config lock poisoned"));
        assert!(error.to_string().contains("restart required"));
    }

    #[test]
    fn poisoned_active_config_error_fails_closed() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let result = thread::spawn({
            let state = state.clone();
            move || {
                let _guard = state
                    .active_config_error
                    .write()
                    .expect("lock active_config_error");
                panic!("poison active_config_error");
            }
        });
        assert!(result.join().is_err());

        let error = read_active_config(&state).expect_err("poisoned config error lock should fail");
        assert!(error
            .to_string()
            .contains("active_config_error lock poisoned"));
        assert!(error.to_string().contains("restart required"));
    }

    #[test]
    fn poisoned_agent_health_resets_and_clears_poison() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        write_agent_health(&state).insert("echo".to_string(), AgentHealthState::default());
        let result = thread::spawn({
            let state = state.clone();
            move || {
                let _guard = state.agent_health.write().expect("lock agent_health");
                panic!("poison agent_health");
            }
        });
        assert!(result.join().is_err());

        let health = write_agent_health(&state);
        assert!(health.is_empty());
        drop(health);
        assert!(state.agent_health.read().is_ok());
    }

    #[test]
    fn poisoned_agent_metrics_resets_and_clears_poison() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        write_agent_metrics(&state).insert("echo".to_string(), AgentMetrics::default());
        let result = thread::spawn({
            let state = state.clone();
            move || {
                let _guard = state.agent_metrics.write().expect("lock agent_metrics");
                panic!("poison agent_metrics");
            }
        });
        assert!(result.join().is_err());

        let metrics = write_agent_metrics(&state);
        assert!(metrics.is_empty());
        drop(metrics);
        assert!(state.agent_metrics.read().is_ok());
    }

    #[test]
    fn poisoned_event_sink_recovers_with_tracing_sink() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let result = thread::spawn({
            let state = state.clone();
            move || {
                let _guard = state.event_sink.write().expect("lock event_sink");
                panic!("poison event_sink");
            }
        });
        assert!(result.join().is_err());

        state.emit_event(
            "task-1",
            Some("item-1"),
            "heartbeat",
            serde_json::json!({"ok": true}),
        );

        assert!(state.event_sink.read().is_ok());
    }
}
