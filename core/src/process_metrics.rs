//! Privacy-safe, project-scoped operational metrics for the Process Console.
//!
//! The implementation moved to `orchestrator-persistence` (FR-141 B4) and is
//! re-exported here so every existing `crate::process_metrics::*` path keeps
//! resolving. The tests stay in core: they bootstrap a schema through
//! `crate::db`, the admin facade, and moving them would move that dependency
//! rather than remove it.

pub use orchestrator_persistence::process_metrics_store::*;
