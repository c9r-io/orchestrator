//! SQLite persistence layer for the Agent Orchestrator.
//!
//! This crate owns `agent_orchestrator.db`: the connection helpers that open it,
//! the migration chain that shapes it, and the repository implementations that
//! read and write it. Everything above this layer — `agent-orchestrator`,
//! `orchestrator-scheduler`, `orchestratord` — reaches the database through
//! these types rather than through the driver, which is the chokepoint
//! DD-147 chose and this crate makes structural.
//!
//! It deliberately depends on nothing else in the workspace. The migration
//! chain is the one artifact whose output is frozen byte-for-byte in
//! `config/governance/schema-snapshot.sql`, and a dependency edge from here to
//! a crate that can change domain types is an edge along which that schema can
//! move without the diff saying so.
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]

/// Public schema migration model and execution helpers.
pub mod migration;
/// Individual migration step implementations.
pub mod migration_steps;
/// Persistence bootstrap entrypoints.
pub mod schema;
/// SQLite-specific connection helpers.
pub mod sqlite;
