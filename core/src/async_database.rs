//! Writer/reader connection pair for async database I/O.
//!
//! The implementation moved to `orchestrator-persistence` (FR-130 Phase A) and
//! is re-exported here so every existing `crate::async_database::*` and
//! `agent_orchestrator::async_database::*` path keeps resolving.
//!
//! The tests moved into the layer with FR-141. Phase A left them here
//! because they bootstrap the schema through `crate::db`, "which is core-side
//! until Phase A's last commit" — `db` is in the layer now, so every path they
//! import resolves there and the reason has expired.

pub use orchestrator_persistence::async_database::*;
