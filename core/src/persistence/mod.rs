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
