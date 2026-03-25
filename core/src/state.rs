use crate::collab::MessageBus;
use crate::config::ActiveConfig;
use crate::config_load::ConfigSelfHealReport;
use crate::events::{EventSink, TracingEventSink};
use crate::metrics::{AgentHealthState, AgentMetrics, AgentRuntimeState};
use crate::runtime::DaemonRuntimeState;
use arc_swap::ArcSwap;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use tokio::process::Child;
use tokio::sync::{Mutex, Notify};

/// Maximum number of tasks that may run concurrently in-process.
pub const MAX_CONCURRENT_TASKS: usize = 10;

static TASK_SEMAPHORE: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

/// Returns the global task-execution semaphore.
pub fn task_semaphore() -> &'static Arc<tokio::sync::Semaphore> {
    TASK_SEMAPHORE.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_TASKS)))
}

/// Owned wrapper returned by bootstrap helpers.
#[derive(Clone)]
pub struct ManagedState {
    /// Shared inner runtime state.
    pub inner: Arc<InnerState>,
}

/// Snapshot of the currently active configuration and its status.
#[derive(Clone)]
pub struct ConfigRuntimeSnapshot {
    /// Active runtime configuration.
    pub active_config: Arc<ActiveConfig>,
    /// Optional validation or load error for the active config.
    pub active_config_error: Option<String>,
    /// Optional self-heal notice associated with the active config.
    pub active_config_notice: Option<ConfigSelfHealReport>,
}

impl ConfigRuntimeSnapshot {
    /// Creates a runtime snapshot from configuration and status values.
    pub fn new(
        active_config: ActiveConfig,
        active_config_error: Option<String>,
        active_config_notice: Option<ConfigSelfHealReport>,
    ) -> Self {
        Self {
            active_config: Arc::new(active_config),
            active_config_error,
            active_config_notice,
        }
    }
}

/// Shared daemon state referenced by services and scheduler code.
pub struct InnerState {
    /// Runtime data directory (`~/.orchestratord` by default).
    pub data_dir: PathBuf,
    /// SQLite database path.
    pub db_path: PathBuf,
    /// Whether unsafe mode is enabled.
    pub unsafe_mode: bool,
    /// Async database handle.
    pub async_database: Arc<crate::async_database::AsyncDatabase>,
    /// Directory containing task and command logs.
    pub logs_dir: PathBuf,
    /// Atomically swappable configuration snapshot.
    pub config_runtime: ArcSwap<ConfigRuntimeSnapshot>,
    /// Currently running tasks keyed by task ID.
    pub running: Mutex<HashMap<String, RunningTask>>,
    /// Runtime agent-health map.
    pub agent_health: tokio::sync::RwLock<HashMap<String, AgentHealthState>>,
    /// Runtime agent metrics map.
    pub agent_metrics: tokio::sync::RwLock<HashMap<String, AgentMetrics>>,
    /// Runtime agent lifecycle map.
    pub agent_lifecycle: tokio::sync::RwLock<HashMap<String, AgentRuntimeState>>,
    /// Collaboration message bus.
    pub message_bus: Arc<MessageBus>,
    // FR-016 sync exception: event emission must remain callable from sync and async
    // paths without making the EventSink interface async. This lock is an
    // observability boundary, not async main-path shared business state.
    /// Event sink used by synchronous and asynchronous execution paths.
    pub event_sink: std::sync::RwLock<Arc<dyn EventSink>>,
    /// Serialized database write coordinator.
    pub db_writer: Arc<crate::db_write::DbWriteCoordinator>,
    /// Interactive session store.
    pub session_store: Arc<crate::session_store::AsyncSessionStore>,
    /// Async task repository wrapper.
    pub task_repo: Arc<crate::task_repository::AsyncSqliteTaskRepository>,
    /// Workflow store manager.
    pub store_manager: crate::store::StoreManager,
    /// Runtime daemon lifecycle state.
    pub daemon_runtime: DaemonRuntimeState,
    /// In-process wakeup channel for idle workers.
    pub worker_notify: Arc<Notify>,
    /// Broadcast channel for trigger-relevant task events (task_completed / task_failed).
    pub trigger_event_tx:
        tokio::sync::broadcast::Sender<crate::trigger_engine::TriggerEventPayload>,
    /// Handle for notifying the trigger engine of config changes.
    pub trigger_engine_handle: std::sync::Mutex<Option<crate::trigger_engine::TriggerEngineHandle>>,
}

impl InnerState {
    /// Emits an event through the currently configured event sink.
    ///
    /// When the event is `task_completed` or `task_failed`, it is also broadcast
    /// on `trigger_event_tx` so the trigger engine can evaluate event triggers.
    pub fn emit_event(
        &self,
        task_id: &str,
        task_item_id: Option<&str>,
        event_type: &str,
        payload: Value,
    ) {
        let sink = clone_event_sink(self);
        sink.emit(task_id, task_item_id, event_type, payload);

        // Broadcast to trigger engine for event-driven triggers.
        if matches!(event_type, "task_completed" | "task_failed") {
            crate::trigger_engine::broadcast_task_event(
                self,
                crate::trigger_engine::TriggerEventPayload {
                    event_type: event_type.to_string(),
                    task_id: task_id.to_string(),
                    payload: None,
                },
            );
        }
    }
}

/// Mutable runtime handle for a task process.
#[derive(Clone)]
pub struct RunningTask {
    /// Shared stop flag observed by all forked execution branches.
    pub stop_flag: Arc<AtomicBool>,
    /// Handle to the currently running child process, if any.
    pub child: Arc<Mutex<Option<Child>>>,
}

impl Default for RunningTask {
    fn default() -> Self {
        Self::new()
    }
}

impl RunningTask {
    /// Creates an empty running-task handle.
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

/// Loads the current configuration snapshot.
pub fn config_runtime_snapshot(state: &InnerState) -> Arc<ConfigRuntimeSnapshot> {
    state.config_runtime.load_full()
}

/// Replaces the current configuration snapshot atomically.
pub fn set_config_runtime_snapshot(state: &InnerState, snapshot: ConfigRuntimeSnapshot) {
    state.config_runtime.store(Arc::new(snapshot));
}

/// Updates the configuration snapshot using a read-modify-write closure.
pub fn update_config_runtime<R>(
    state: &InnerState,
    f: impl FnOnce(&ConfigRuntimeSnapshot) -> (ConfigRuntimeSnapshot, R),
) -> R {
    let current = state.config_runtime.load_full();
    let (next, result) = f(current.as_ref());
    state.config_runtime.store(Arc::new(next));
    result
}

/// Replaces only the active config while preserving status fields.
pub fn replace_active_config(state: &InnerState, active_config: ActiveConfig) {
    update_config_runtime(state, |current| {
        (
            ConfigRuntimeSnapshot {
                active_config: Arc::new(active_config),
                active_config_error: current.active_config_error.clone(),
                active_config_notice: current.active_config_notice.clone(),
            },
            (),
        )
    });
}

/// Replaces the active config status fields while preserving the config itself.
pub fn replace_active_config_status(
    state: &InnerState,
    active_config_error: Option<String>,
    active_config_notice: Option<ConfigSelfHealReport>,
) {
    update_config_runtime(state, |current| {
        (
            ConfigRuntimeSnapshot {
                active_config: Arc::clone(&current.active_config),
                active_config_error,
                active_config_notice,
            },
            (),
        )
    });
}

/// Clears active-config error and notice state.
pub fn clear_active_config_status(state: &InnerState) {
    replace_active_config_status(state, None, None);
}

/// Resets the active config to an empty default snapshot.
pub fn reset_active_config_to_default(state: &InnerState) {
    set_config_runtime_snapshot(
        state,
        ConfigRuntimeSnapshot::new(
            ActiveConfig {
                config: Default::default(),
                workspaces: Default::default(),
                projects: Default::default(),
            },
            None,
            None,
        ),
    );
}

/// Clones the current event sink, recovering from poisoning if needed.
pub fn clone_event_sink(state: &InnerState) -> Arc<dyn EventSink> {
    match state.event_sink.read() {
        Ok(guard) => guard.clone(),
        Err(err) => {
            drop(err.into_inner());
            let mut guard = state
                .event_sink
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = Arc::new(TracingEventSink::new());
            state.event_sink.clear_poison();
            guard.clone()
        }
    }
}

/// Replaces the current event sink, recovering from poisoning if needed.
pub fn replace_event_sink(state: &InnerState, sink: Arc<dyn EventSink>) {
    match state.event_sink.write() {
        Ok(mut guard) => *guard = sink,
        Err(err) => {
            let mut guard = err.into_inner();
            *guard = sink;
            state.event_sink.clear_poison();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_load::{read_active_config, read_loaded_config};
    use crate::test_utils::TestState;
    use std::sync::atomic::Ordering;

    #[test]
    fn running_task_starts_with_defaults() {
        let runtime = RunningTask::new();
        assert!(!runtime.stop_flag.load(Ordering::SeqCst));
        assert!(runtime.child.try_lock().expect("lock child").is_none());
    }

    #[tokio::test]
    async fn state_accessors_round_trip_agent_maps() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        state.agent_health.write().await.insert(
            "echo".to_string(),
            AgentHealthState {
                consecutive_errors: 1,
                diseased_until: None,
                total_lifetime_errors: 1,
                capability_health: HashMap::new(),
            },
        );
        state
            .agent_metrics
            .write()
            .await
            .insert("echo".to_string(), AgentMetrics::default());

        assert!(state.agent_health.read().await.contains_key("echo"));
        assert!(state.agent_metrics.read().await.contains_key("echo"));
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
    fn update_config_runtime_replaces_snapshot_without_exposing_guards() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let original = read_loaded_config(&state)
            .expect("read loaded config")
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|p| p.workflows.keys().next())
            .cloned()
            .unwrap_or_default();
        update_config_runtime(&state, |current| {
            let mut next = current.clone();
            let workflow_clone = next
                .active_config
                .projects
                .get(crate::config::DEFAULT_PROJECT_ID)
                .and_then(|p| p.workflows.get(&original))
                .cloned()
                .expect("default workflow should exist");
            Arc::make_mut(&mut next.active_config)
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
            (next, ())
        });

        let updated_exists = read_loaded_config(&state)
            .expect("re-read active config")
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .map(|p| p.workflows.contains_key(&format!("{}-updated", original)))
            .unwrap_or(false);
        assert!(updated_exists);
    }

    #[test]
    fn read_active_config_rejects_non_runnable_snapshot() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        replace_active_config_status(
            &state,
            Some("active config is not runnable".to_string()),
            None,
        );

        let error = read_active_config(&state).expect_err("non-runnable config should fail");
        assert!(error.to_string().contains("not runnable"));
    }

    #[tokio::test]
    async fn agent_health_and_metrics_reset_explicitly() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        state
            .agent_health
            .write()
            .await
            .insert("echo".to_string(), AgentHealthState::default());
        state
            .agent_metrics
            .write()
            .await
            .insert("echo".to_string(), AgentMetrics::default());

        state.agent_health.write().await.clear();
        state.agent_metrics.write().await.clear();

        assert!(state.agent_health.read().await.is_empty());
        assert!(state.agent_metrics.read().await.is_empty());
    }

    #[test]
    fn poisoned_event_sink_recovers_with_tracing_sink() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let result = std::thread::spawn({
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
