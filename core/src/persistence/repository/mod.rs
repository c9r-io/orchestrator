//! Persistence repository traits and SQLite implementations.
//!
//! All but `config` moved to `orchestrator-persistence` (FR-130 Phase A) and
//! are re-exported here, so every existing
//! `crate::persistence::repository::*` path keeps resolving.
//!
//! `config` stays. It is a domain repository that happens to live under
//! `persistence/`: it reads and writes `crate::crd` and `crate::resource`, and
//! `crate::crd::plugins` in turn calls into `crate::db`. Sinking both while
//! `crd` remains in core would close a `persistence -> crd -> persistence`
//! cycle. It moves in Phase B, alongside `crd`, rather than being forced down
//! here.

mod config;
/// Daemon metadata persistence (incarnation counter, etc.).
pub mod daemon_meta;
mod scheduler;
mod workflow_store;

pub use config::{ConfigRepository, HealLogEntry, SqliteConfigRepository};
pub use orchestrator_persistence::repository::{SessionRepository, SqliteSessionRepository};
pub use scheduler::{SchedulerRepository, SqliteSchedulerRepository};
pub use workflow_store::{
    SqliteWorkflowStoreRepository, WorkflowStoreEntryRow, WorkflowStoreRepository,
};
