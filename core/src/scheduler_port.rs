//! Scheduler port for cross-crate task enqueue dispatch.
//!
//! Core modules (e.g. `trigger_engine`) need to enqueue tasks but cannot
//! depend on the `orchestrator-scheduler` crate (that would create a cycle).
//! This module defines the [`TaskEnqueuer`] trait so core can call into the
//! scheduler through dynamic dispatch; the concrete implementation lives in
//! `orchestrator-scheduler` and is wired by the daemon at startup.

use crate::state::InnerState;
use std::sync::Arc;

/// Port for enqueuing tasks from core modules that cannot depend on the
/// scheduler crate directly.
///
/// The scheduler crate provides the concrete [`TaskEnqueuer`] implementation;
/// the daemon registers it in [`InnerState`] at startup.  Test contexts use
/// the [`NoopTaskEnqueuer`] default.
#[async_trait::async_trait]
pub trait TaskEnqueuer: Send + Sync {
    /// Marks *task_id* as pending and wakes the background worker.
    async fn enqueue_task(&self, state: &InnerState, task_id: &str) -> crate::error::Result<()>;
}

/// No-op implementation used in test contexts and CLI-only paths where no
/// background scheduler is running.
pub struct NoopTaskEnqueuer;

#[async_trait::async_trait]
impl TaskEnqueuer for NoopTaskEnqueuer {
    async fn enqueue_task(&self, _state: &InnerState, _task_id: &str) -> crate::error::Result<()> {
        Ok(())
    }
}

/// Convenience constructor for a type-erased no-op enqueuer.
pub fn noop_task_enqueuer() -> Arc<dyn TaskEnqueuer> {
    Arc::new(NoopTaskEnqueuer)
}
