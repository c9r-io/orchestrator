use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::Instant;

const STATE_SERVING: u8 = 0;
const STATE_DRAINING: u8 = 1;
const STATE_STOPPED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonLifecycleState {
    Serving,
    Draining,
    Stopped,
}

impl DaemonLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Serving => "serving",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Serving => STATE_SERVING,
            Self::Draining => STATE_DRAINING,
            Self::Stopped => STATE_STOPPED,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            STATE_DRAINING => Self::Draining,
            STATE_STOPPED => Self::Stopped,
            _ => Self::Serving,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DaemonRuntimeSnapshot {
    pub uptime_secs: u64,
    pub lifecycle_state: DaemonLifecycleState,
    pub shutdown_requested: bool,
    pub configured_workers: u64,
    pub live_workers: u64,
    pub idle_workers: u64,
    pub active_workers: u64,
    pub running_tasks: u64,
}

pub struct DaemonRuntimeState {
    started_at: Instant,
    lifecycle_state: AtomicU8,
    shutdown_requested: AtomicBool,
    configured_workers: AtomicU64,
    live_workers: AtomicU64,
    idle_workers: AtomicU64,
    active_workers: AtomicU64,
    running_tasks: AtomicU64,
}

impl Default for DaemonRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonRuntimeState {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            lifecycle_state: AtomicU8::new(STATE_SERVING),
            shutdown_requested: AtomicBool::new(false),
            configured_workers: AtomicU64::new(0),
            live_workers: AtomicU64::new(0),
            idle_workers: AtomicU64::new(0),
            active_workers: AtomicU64::new(0),
            running_tasks: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> DaemonRuntimeSnapshot {
        DaemonRuntimeSnapshot {
            uptime_secs: self.started_at.elapsed().as_secs(),
            lifecycle_state: DaemonLifecycleState::from_u8(
                self.lifecycle_state.load(Ordering::SeqCst),
            ),
            shutdown_requested: self.shutdown_requested.load(Ordering::SeqCst),
            configured_workers: self.configured_workers.load(Ordering::SeqCst),
            live_workers: self.live_workers.load(Ordering::SeqCst),
            idle_workers: self.idle_workers.load(Ordering::SeqCst),
            active_workers: self.active_workers.load(Ordering::SeqCst),
            running_tasks: self.running_tasks.load(Ordering::SeqCst),
        }
    }

    pub fn set_configured_workers(&self, count: usize) {
        self.configured_workers
            .store(count as u64, Ordering::SeqCst);
    }

    pub fn request_shutdown(&self) -> bool {
        let first = !self.shutdown_requested.swap(true, Ordering::SeqCst);
        self.lifecycle_state
            .store(DaemonLifecycleState::Draining.as_u8(), Ordering::SeqCst);
        first
    }

    pub fn mark_stopped(&self) {
        self.lifecycle_state
            .store(DaemonLifecycleState::Stopped.as_u8(), Ordering::SeqCst);
    }

    pub fn worker_started(&self) {
        self.live_workers.fetch_add(1, Ordering::SeqCst);
        self.idle_workers.fetch_add(1, Ordering::SeqCst);
    }

    pub fn worker_stopped(&self, was_busy: bool) {
        self.live_workers.fetch_sub(1, Ordering::SeqCst);
        if was_busy {
            self.active_workers.fetch_sub(1, Ordering::SeqCst);
        } else {
            self.idle_workers.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn worker_became_busy(&self) {
        self.idle_workers.fetch_sub(1, Ordering::SeqCst);
        self.active_workers.fetch_add(1, Ordering::SeqCst);
    }

    pub fn worker_became_idle(&self) {
        self.active_workers.fetch_sub(1, Ordering::SeqCst);
        self.idle_workers.fetch_add(1, Ordering::SeqCst);
    }

    pub fn running_task_started(&self) {
        self.running_tasks.fetch_add(1, Ordering::SeqCst);
    }

    pub fn running_task_finished(&self) {
        self.running_tasks.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reflects_state_transitions() {
        let runtime = DaemonRuntimeState::new();
        runtime.set_configured_workers(2);
        runtime.worker_started();
        runtime.worker_started();
        runtime.worker_became_busy();
        runtime.running_task_started();

        let serving = runtime.snapshot();
        assert_eq!(serving.lifecycle_state, DaemonLifecycleState::Serving);
        assert_eq!(serving.configured_workers, 2);
        assert_eq!(serving.live_workers, 2);
        assert_eq!(serving.idle_workers, 1);
        assert_eq!(serving.active_workers, 1);
        assert_eq!(serving.running_tasks, 1);
        assert!(!serving.shutdown_requested);

        assert!(runtime.request_shutdown());
        let draining = runtime.snapshot();
        assert_eq!(draining.lifecycle_state, DaemonLifecycleState::Draining);
        assert!(draining.shutdown_requested);

        runtime.running_task_finished();
        runtime.worker_became_idle();
        runtime.worker_stopped(false);
        runtime.worker_stopped(false);
        runtime.mark_stopped();
        let stopped = runtime.snapshot();
        assert_eq!(stopped.lifecycle_state, DaemonLifecycleState::Stopped);
        assert_eq!(stopped.live_workers, 0);
        assert_eq!(stopped.idle_workers, 0);
        assert_eq!(stopped.active_workers, 0);
        assert_eq!(stopped.running_tasks, 0);
    }
}
