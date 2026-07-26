//! SQLite persistence layer for the Agent Orchestrator.
//!
//! This crate owns `agent_orchestrator.db`: the connection helpers that open it,
//! the migration chain that shapes it, and the repository implementations that
//! read and write it. Everything above this layer — `agent-orchestrator`,
//! `orchestrator-scheduler`, `orchestratord` — reaches the database through
//! these types rather than through the driver, which is the chokepoint
//! DD-147 chose and this crate makes structural.
//!
//! Its only workspace edges are `orchestrator-config` and
//! `orchestrator-collab`, both leaf data crates, and both reached from exactly
//! two fields of two [`dto`] structs — `ConfigOverview::config` and
//! `RunResult::output`. Nothing in the migration chain, the connection
//! helpers or the repositories names them. That distinction is the point: the
//! chain's output is frozen byte-for-byte in
//! `config/governance/schema-snapshot.sql`, and an edge from the chain to a
//! crate that can change domain types is an edge along which that schema can
//! move without the diff saying so.
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]

/// Writer/reader connection pair for async database I/O.
pub mod async_database;
/// The `control_action_audit` table and the statements over it.
pub mod control_action_audit;
/// Admin facade: project queries, audit, metrics and reset.
pub mod db;
/// Database maintenance utilities: VACUUM and size reporting.
pub mod db_maintenance;
/// Async facade for persistence writes that need serialized database access.
pub mod db_write;
/// Row and read-model shapes the repositories produce and consume.
pub mod dto;
/// Retention queries over the `events` table: age, volume and rows.
pub mod event_retention;
/// Row access for the `events` table.
pub mod events;
/// The handoff and resume tables and the statements over them.
pub mod handoff_store;
/// Public schema migration model and execution helpers.
pub mod migration;
/// Individual migration step implementations.
pub mod migration_steps;
/// Domain-specific repository traits and their SQLite implementations.
pub mod repository;
/// Persistence bootstrap entrypoints.
pub mod schema;
/// Agent session rows: creation, state transitions, reader/writer attachment.
pub mod session_store;
/// The source-automation route tables and the statements over them.
pub mod source_automation_routes;
/// The SourceConnection tables and the statements over them.
pub mod source_connections;
/// The source-ingestion tables and the statements over them.
pub mod source_events;
/// SQLite-specific connection helpers.
pub mod sqlite;
/// Task-execution persistence: tasks, items, command runs and events.
pub mod task_repository;

/// Returns the current UTC timestamp encoded as RFC 3339.
///
/// It lives in this crate because the format is a database contract: every
/// caller in this repository is writing a `created_at` or `updated_at` column,
/// and a second definition beside this one is how two rows in one table come to
/// disagree about what a timestamp looks like. `core` re-exports it as
/// `config_load::now_ts`, which is where most callers still name it.
pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}
