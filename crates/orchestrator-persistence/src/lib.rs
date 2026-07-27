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
//! `orchestrator-collab`, both leaf data crates. They are reached from two
//! fields of two [`dto`] structs — `ConfigOverview::config` and
//! `RunResult::output` — and, since FR-141 B3, from
//! [`scheduler_state::create_dynamic_task_items`], which takes
//! `orchestrator_config::config::NewDynamicItem` rather than defining a second
//! copy of a shape that already exists. Nothing in the migration chain or the
//! connection helpers names them. That distinction is the point: the
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
/// The attention queue tables and the statements over them.
pub mod attention_store;
/// Backlinks from an audited row to the request that produced it.
pub mod audit_links;
/// The configuration and resource tables and the statements over them.
pub mod config_store;
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

#[cfg(test)]
mod migration_chain_tests;
/// Process Console metrics: samples, rollups and the queries over them.
pub mod process_metrics_store;
/// Domain-specific repository traits and their SQLite implementations.
pub mod repository;
/// The reads and writes the scheduler makes about task and item state.
pub mod scheduler_state;
/// Persistence bootstrap entrypoints.
pub mod schema;
/// The `session_control_actions` table: idempotency envelopes for session control.
pub mod session_control_audit;
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
/// The `trigger_state` table and the reads the trigger engine makes.
pub mod trigger_state;

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
