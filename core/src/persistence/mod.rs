//! Persistence infrastructure layer.
//!
//! This module provides the foundational persistence building blocks:
//! connection setup (`sqlite`), schema bootstrap and migrations (`schema`,
//! `migration`, `migration_steps`), and domain-specific repository traits
//! with SQLite implementations (`repository`).
//!
//! **Not to be confused with:**
//! - `task_repository` — task-execution persistence (items, runs, events)
//! - `db` — admin facade (project queries, audit, metrics, reset)
//! - `async_database` — writer/reader connection pair for async I/O

/// Public schema migration model and execution helpers.
pub mod migration;
/// Individual migration step implementations.
pub mod migration_steps;
/// Persistence repository traits and SQLite implementations.
pub mod repository;
/// Persistence bootstrap entrypoints.
pub mod schema;
/// SQLite-specific connection helpers.
pub mod sqlite;
