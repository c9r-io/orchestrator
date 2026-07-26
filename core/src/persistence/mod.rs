//! Persistence infrastructure layer.
//!
//! The connection setup (`sqlite`), schema bootstrap and migrations (`schema`,
//! `migration`, `migration_steps`) now live in the `orchestrator-persistence`
//! crate and are re-exported here, so every existing
//! `agent_orchestrator::persistence::*` path keeps resolving. What remains in
//! core is `repository`, whose domain-coupled members cannot sink below the
//! types they depend on.
//!
//! **Not to be confused with:**
//! - `task_repository` — task-execution persistence (items, runs, events)
//! - `db` — admin facade (project queries, audit, metrics, reset)
//! - `async_database` — writer/reader connection pair for async I/O

pub use orchestrator_persistence::{migration, migration_steps, schema, sqlite};

/// Persistence repository traits and SQLite implementations.
pub mod repository;
/// Reviewed schema baseline for the registered migration chain (test-only).
///
/// It stays in core, unchanged, although the chain it exercises has moved.
/// The comparison it performs is FR-130's only behavioural evidence that the
/// extraction preserved the schema, and a test that moves with the code it
/// tests is not the same test — `cargo test -p agent-orchestrator
/// schema_snapshot` has to mean in the "after" what it meant in the "before".
/// It follows the chain once core no longer re-exports it (FR-130 Phase C).
mod schema_snapshot;
