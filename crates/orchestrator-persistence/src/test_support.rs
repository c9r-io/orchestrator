//! The one way to obtain a driver connection from outside this crate, and it
//! exists only in a test build.
//!
//! FR-141 closed the layer's public API over the SQLite connection type. Most
//! callers did not need one: their SQL moved into this crate and they now call
//! a named operation. Three groups could not follow.
//!
//! 1. **Core's own tests.** They build a fixture with `TestState` and
//!    `create_task_impl` — domain machinery that sits *above* this layer — run
//!    core logic against it, and then open the database to assert on the rows
//!    that logic wrote. They cannot sink into this crate without inverting the
//!    dependency, and rewriting them to read back through the repository they
//!    are testing would make them assert against the same code path they are
//!    evidence for.
//! 2. **This crate's own `tests/round_trip.rs`.** Rust compiles an integration
//!    test as a separate crate, so it is outside this crate's privacy boundary
//!    by the language's design, not by anyone's choice.
//! 3. **`crates/integration-tests`.** The dependency ledger already blesses it
//!    as `test-only`; it asserts against the database directly because that is
//!    what makes it an end-to-end check rather than a restatement.
//!
//! What makes a conditional hole different from the one FR-141 closed is
//! whether anything stops it being opened in production. Two things do:
//!
//! - The module exists only under the `test-support` feature. `cargo build`
//!   does not enable it, so a production call to any function here is a
//!   compile error in the shipped artifact, not a lint.
//! - Every consumer enables the feature from `[dev-dependencies]`. Under
//!   resolver 2 a dev-dependency's features are not unified into a normal
//!   build, and `scripts/qa/persistence-api-boundary.rb` fails if any crate
//!   enables it from `[dependencies]`. The gate does not merely skip this
//!   module — skipping it would certify an exemption it cannot observe. It
//!   counts what the module yields, records that separately in the ledger, and
//!   asserts the condition that keeps the count harmless.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

use crate::async_database::AsyncDatabase;
use crate::db::TaskReference;
use crate::dto::{TaskGraphDebugBundle, TaskItemRow, TaskSummary};
use crate::events::StepEventRow;
use crate::migration::{AppliedMigrationSummary, Migration};
use crate::session_store::{NewSession, SessionRow};
use crate::task_repository::TaskDetailRows;
use crate::task_repository::types::{NewTaskGraphRun, NewTaskGraphSnapshot, TaskLogRunRow};

/// Opens a configured connection to the database at `db_path`.
/// Opens a SQLite connection using the orchestrator persistence defaults.
pub fn open_conn(db_path: &Path) -> Result<Connection> {
    crate::db::open_conn(db_path)
}

/// Applies the layer's standard pragmas to an already-open connection.
/// Applies the standard busy timeout and pragma configuration to a connection.
pub fn configure_conn(conn: &Connection) -> Result<()> {
    crate::db::configure_conn(conn)
}

/// The serialized writer connection behind an [`AsyncDatabase`].
///
/// A free function rather than a method: the call site then reads
/// `test_support::writer(&db)` and says at a glance that the test reached
/// through the door, which `db.writer()` did not.
/// Delegates to the layer's `writer`.
pub fn writer(db: &AsyncDatabase) -> &tokio_rusqlite::Connection {
    db.writer()
}

/// The reader connection behind an [`AsyncDatabase`].
/// Delegates to the layer's `reader`.
pub fn reader(db: &AsyncDatabase) -> &tokio_rusqlite::Connection {
    db.reader()
}

// ── Statements a test reaches with a connection in hand ──────────────────
//
// Holding a connection and running one of the layer's statements against it is
// one capability, not two, so both halves of it live behind the same feature.
//
// Thin delegations rather than a second implementation: each forwards to the
// statement production runs, so a test asserts against the real one and cannot
// be kept green by a copy that drifted from it. They are wrappers rather than
// `pub use` only because Rust will not re-export a `pub(crate)` item as `pub`,
// and `pub(crate)` is exactly what the demotion is for.
/// Counts running or pending tasks for one project workflow pair.
pub fn count_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
) -> Result<i64> {
    crate::db::count_non_terminal_tasks_by_workflow(conn, project_id, workflow_id)
}

/// Counts running or pending tasks for one project workspace pair.
pub fn count_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
) -> Result<i64> {
    crate::db::count_non_terminal_tasks_by_workspace(conn, project_id, workspace_id)
}

/// Inserts a new interactive session row.
pub fn insert_session(conn: &Connection, s: &NewSession<'_>) -> Result<()> {
    crate::session_store::insert_session(conn, s)
}

/// Inserts one task-graph planning run.
pub fn insert_task_graph_run(conn: &Connection, run: &NewTaskGraphRun) -> Result<()> {
    crate::task_repository::queries::insert_task_graph_run(conn, run)
}

/// Inserts one task-graph snapshot belonging to a planning run.
pub fn insert_task_graph_snapshot(
    conn: &Connection,
    snapshot: &NewTaskGraphSnapshot,
) -> Result<()> {
    crate::task_repository::queries::insert_task_graph_snapshot(conn, snapshot)
}

/// Lists the oldest non-terminal tasks for one project workflow pair.
pub fn list_non_terminal_tasks_by_workflow(
    conn: &Connection,
    project_id: &str,
    workflow_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    crate::db::list_non_terminal_tasks_by_workflow(conn, project_id, workflow_id, limit)
}

/// Lists the oldest non-terminal tasks for one project workspace pair.
pub fn list_non_terminal_tasks_by_workspace(
    conn: &Connection,
    project_id: &str,
    workspace_id: &str,
    limit: usize,
) -> Result<Vec<TaskReference>> {
    crate::db::list_non_terminal_tasks_by_workspace(conn, project_id, workspace_id, limit)
}

/// Lists a task's items with the fields one execution cycle needs.
pub fn list_task_items_for_cycle(conn: &Connection, task_id: &str) -> Result<Vec<TaskItemRow>> {
    crate::task_repository::queries::list_task_items_for_cycle(conn, task_id)
}

/// Lists the most recent command runs of a task with their log paths, newest
/// first, capped at `limit`.
pub fn list_task_log_runs(
    conn: &Connection,
    task_id: &str,
    limit: usize,
) -> Result<Vec<TaskLogRunRow>> {
    crate::task_repository::queries::list_task_log_runs(conn, task_id, limit)
}

/// Lists ids of tasks in a terminal state whose `updated_at` is older than
/// `retention_days`, capped at `limit`.
///
/// `retention_days` and `limit` are interpolated rather than bound, because
/// SQLite will not accept a parameter inside `datetime('now', ?)`'s modifier or
/// after `LIMIT`. Both are `u32` from configuration, so there is no string to
/// inject; the types are the guard, and that is why the signature takes `u32`
/// rather than something more convenient.
pub fn list_terminal_tasks_older_than(
    conn: &Connection,
    retention_days: u32,
    limit: u32,
) -> Result<Vec<String>> {
    crate::task_repository::queries::list_terminal_tasks_older_than(conn, retention_days, limit)
}

/// Loads a session row by session identifier.
pub fn load_session(conn: &Connection, session_id: &str) -> Result<Option<SessionRow>> {
    crate::session_store::load_session(conn, session_id)
}

/// Loads a task's items, command runs and events in one pass.
pub fn load_task_detail_rows(conn: &Connection, task_id: &str) -> Result<TaskDetailRows> {
    crate::task_repository::queries::load_task_detail_rows(conn, task_id)
}

/// Loads every task-graph run of a task together with its snapshots, for the
/// graph debug surface.
pub fn load_task_graph_debug_bundles(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<TaskGraphDebugBundle>> {
    crate::task_repository::queries::load_task_graph_debug_bundles(conn, task_id)
}

/// Loads the summary row for one task.
pub fn load_task_summary(conn: &Connection, task_id: &str) -> Result<TaskSummary> {
    crate::task_repository::queries::load_task_summary(conn, task_id)
}

/// Blanket-pause all running tasks and reset their items to pending.
/// Used during daemon shutdown before exec() to prevent orphaned state
/// across process replacement.  Handles the race where a requesting worker
/// already removed its task from `state.running` before
/// `shutdown_running_tasks` could pause it.
/// Returns the number of items reset.
pub fn pause_all_running_tasks_and_items(conn: &Connection) -> Result<usize> {
    crate::task_repository::state::pause_all_running_tasks_and_items(conn)
}

/// Pause only tasks in `restart_pending` status and reset their running items.
/// Used before `exec()` to avoid disrupting unrelated tasks.
pub fn pause_restart_pending_tasks_and_items(conn: &Connection) -> Result<usize> {
    crate::task_repository::state::pause_restart_pending_tasks_and_items(conn)
}

/// Recover all orphaned running items across all tasks.
/// Resets running items to `pending` and their parent tasks to `restart_pending`.
/// Returns `Vec<(task_id, Vec<item_id>)>` for audit.
pub fn recover_orphaned_running_items(conn: &Connection) -> Result<Vec<(String, Vec<String>)>> {
    crate::task_repository::state::recover_orphaned_running_items(conn)
}

/// Recover orphaned running items for a single task.
/// Returns the list of recovered item IDs.
pub fn recover_orphaned_running_items_for_task(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<String>> {
    crate::task_repository::state::recover_orphaned_running_items_for_task(conn, task_id)
}

/// Recover stalled running items older than the given threshold.
///
/// `exclude_task_ids` contains tasks that currently have an active worker.
/// Items belonging to excluded tasks are skipped entirely — the active worker
/// is responsible for managing them (they may simply be slow, not stalled).
///
/// Returns `Vec<(task_id, Vec<item_id>)>` for audit (only non-excluded tasks).
pub fn recover_stalled_running_items(
    conn: &Connection,
    stall_threshold_secs: u64,
    exclude_task_ids: &std::collections::HashSet<String>,
) -> Result<Vec<(String, Vec<String>)>> {
    crate::task_repository::state::recover_stalled_running_items(
        conn,
        stall_threshold_secs,
        exclude_task_ids,
    )
}

/// Resets unresolved items back to pending and resets the cycle counter
/// when there are items to re-process, so the scheduler starts fresh.
pub fn reset_unresolved_items(conn: &Connection, task_id: &str) -> Result<()> {
    crate::task_repository::state::reset_unresolved_items(conn, task_id)
}

/// Applies every migration newer than the current schema version.
pub fn run_pending(conn: &Connection, migrations: &[Migration]) -> Result<AppliedMigrationSummary> {
    crate::migration::run_pending(conn, migrations)
}

/// Answers whether any stored `SecretStore` resource still names this key.
///
/// The needle is built here rather than passed in because how a key id appears
/// inside a persisted `spec_json` is this layer's encoding, not its caller's
/// question. The caller's question is only "is this revoked key still in use".
pub fn secret_store_resources_reference_key(conn: &Connection, key_id: &str) -> Result<bool> {
    crate::db::secret_store_resources_reference_key(conn, key_id)
}

/// Returns every row for a task whose `event_type` is in `event_types`, oldest first.
pub fn step_event_rows(
    conn: &Connection,
    task_id: &str,
    event_types: &[&str],
) -> Result<Vec<StepEventRow>> {
    crate::events::step_event_rows(conn, task_id, event_types)
}

/// Updates a task-graph run's status and bumps its `updated_at`.
pub fn update_task_graph_run_status(
    conn: &Connection,
    graph_run_id: &str,
    status: &str,
) -> Result<()> {
    crate::task_repository::queries::update_task_graph_run_status(conn, graph_run_id, status)
}
