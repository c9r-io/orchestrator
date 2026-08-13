#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

/// Command-run insert payload.
pub mod command_run;
/// Writing a new task, its items and its creation-time events as one commit.
pub mod creation;
mod items;
/// Connection-level read queries over tasks, items, runs and events.
///
/// The repository types below are the supported entry point; these functions
/// are public because callers that already hold a `Connection` — recovery
/// paths, maintenance jobs, tests asserting against a specific connection —
/// need them without going back through the pool.
pub mod queries;
/// What happens to rows referencing a task when the task is deleted.
pub mod references;
/// Connection-level task and item state transitions, including the recovery
/// passes that run at daemon start.
///
/// Public for the same reason as [`queries`].
pub mod state;
/// Repository traits implemented by the SQLite adapters.
pub mod trait_def;
/// Row and payload types exchanged with the task-execution tables.
pub mod types;
mod write_ops;

pub use command_run::NewCommandRun;
pub use creation::{NewTaskRow, insert_task_with_items, reset_task_item};
pub(crate) use items::delete_task_and_collect_log_paths;
pub use references::{Disposition, TaskDeleteBlocked, disposition_for, recorded_dispositions};
pub use trait_def::{
    CommandRunRepository, EventRepository, TaskGraphRepository, TaskItemMutRepository,
    TaskItemQueryRepository, TaskQueryRepository, TaskRepository, TaskStateRepository,
};
pub use types::{
    DbEventRecord, NewTaskGraphRun, NewTaskGraphSnapshot, TaskLogRunRow, TaskRepositorySource,
    TaskRuntimeRow,
};
pub use write_ops::{CompletedRunRecord, InflightRunRecord};

use crate::async_database::{AsyncDatabase, flatten_err};
use crate::dto::{CommandRunDto, EventDto, TaskGraphDebugBundle, TaskItemDto};
use anyhow::Result;
use std::sync::Arc;

/// Carries an `anyhow::Error` across the worker boundary without flattening it.
///
/// `tokio_rusqlite::Error::Other` holds a `Box<dyn Error>`, and converting an
/// `anyhow::Error` into that box **discards the concrete type**: the message
/// survives and `downcast` afterwards fails. Measured, not assumed — a boxed
/// `TaskDeleteBlocked` comes back out as something that still prints correctly
/// and no longer downcasts.
///
/// Every caller of the delete path decides what to do by asking *whether* the
/// refusal was a blocking reference: a retention sweep records a skip, an
/// operator command prints a diagnostic, anything else propagates. The only
/// alternative to keeping the type is matching on message text, which is the
/// failure this repository keeps finding in its own postmortems.
#[derive(Debug)]
struct CarriedError(anyhow::Error);

impl std::fmt::Display for CarriedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for CarriedError {}

/// Flattens a worker error, keeping anything sent through [`CarriedError`]
/// intact — including its downcast target and any context attached to it.
fn recover_carried(err: tokio_rusqlite::Error) -> anyhow::Error {
    match err {
        tokio_rusqlite::Error::Other(inner) => match inner.downcast::<CarriedError>() {
            Ok(carried) => carried.0,
            Err(inner) => flatten_err(tokio_rusqlite::Error::Other(inner)),
        },
        other => flatten_err(other),
    }
}

/// Tuple returned by detail queries: items, runs, events, and graph bundles.
pub type TaskDetailRows = (
    Vec<TaskItemDto>,
    Vec<CommandRunDto>,
    Vec<EventDto>,
    Vec<TaskGraphDebugBundle>,
);

/// Synchronous SQLite-backed implementation of [`TaskRepository`].
pub struct SqliteTaskRepository {
    source: types::TaskRepositorySource,
}

impl SqliteTaskRepository {
    /// Creates a repository backed by the given connection source.
    pub fn new<T>(source: T) -> Self
    where
        T: Into<types::TaskRepositorySource>,
    {
        Self {
            source: source.into(),
        }
    }

    fn connection(&self) -> Result<types::TaskRepositoryConn> {
        self.source.connection()
    }
}

// ── TaskQueryRepository ─────────────────────────────────────────────

impl TaskQueryRepository for SqliteTaskRepository {
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String> {
        let conn = self.connection()?;
        queries::resolve_task_id(&conn, task_id_or_prefix)
    }

    fn load_task_summary(&self, task_id: &str) -> Result<crate::dto::TaskSummary> {
        let conn = self.connection()?;
        queries::load_task_summary(&conn, task_id)
    }

    fn load_task_detail_rows(&self, task_id: &str) -> Result<TaskDetailRows> {
        let conn = self.connection()?;
        queries::load_task_detail_rows(&conn, task_id)
    }

    fn load_task_timeline_source(
        &self,
        task_id: &str,
        max_event_id: Option<i64>,
    ) -> Result<crate::dto::TaskTimelineSource> {
        let conn = self.connection()?;
        queries::load_task_timeline_source(&conn, task_id, max_event_id)
    }

    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)> {
        let conn = self.connection()?;
        queries::load_task_item_counts(&conn, task_id)
    }

    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>> {
        let conn = self.connection()?;
        queries::list_task_ids_ordered_by_created_desc(&conn)
    }

    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::find_latest_resumable_task_id(&conn, include_pending)
    }

    fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow> {
        let conn = self.connection()?;
        queries::load_task_runtime_row(&conn, task_id)
    }

    fn load_task_status(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::load_task_status(&conn, task_id)
    }

    fn load_task_name(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::load_task_name(&conn, task_id)
    }
}

// ── TaskItemQueryRepository ──���──────────────────────────────────────

impl TaskItemQueryRepository for SqliteTaskRepository {
    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::first_task_item_id(&conn, task_id)
    }

    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<crate::dto::TaskItemRow>> {
        let conn = self.connection()?;
        queries::list_task_items_for_cycle(&conn, task_id)
    }

    fn count_unresolved_items(&self, task_id: &str) -> Result<i64> {
        let conn = self.connection()?;
        queries::count_unresolved_items(&conn, task_id)
    }

    fn count_stale_pending_items(&self, task_id: &str) -> Result<i64> {
        let conn = self.connection()?;
        queries::count_stale_pending_items(&conn, task_id)
    }

    fn count_recent_heartbeats_for_items(
        &self,
        task_id: &str,
        item_ids: &[String],
        cutoff_ts: &str,
    ) -> Result<i64> {
        let conn = self.connection()?;
        write_ops::count_recent_heartbeats_for_items(&conn, task_id, item_ids, cutoff_ts)
    }
}

// ── TaskStateRepository ──────────────────────────────────��──────────

impl TaskStateRepository for SqliteTaskRepository {
    fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()> {
        let conn = self.connection()?;
        state::set_task_status(&conn, task_id, status, set_completed)
    }

    fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()> {
        let conn = self.connection()?;
        state::prepare_task_for_start_batch(&conn, task_id)
    }

    fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()> {
        let conn = self.connection()?;
        state::update_task_cycle_state(&conn, task_id, current_cycle, init_done)
    }

    fn update_task_pipeline_vars(&self, task_id: &str, pipeline_vars_json: &str) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_task_pipeline_vars(&conn, task_id, pipeline_vars_json)
    }

    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let conn = self.connection()?;
        items::delete_task_and_collect_log_paths(&conn, task_id)
    }
}

// ── TaskItemMutRepository ───────────────────────────────────────────

impl TaskItemMutRepository for SqliteTaskRepository {
    fn mark_task_item_running(&self, task_item_id: &str) -> Result<()> {
        let conn = self.connection()?;
        items::mark_task_item_running(&conn, task_item_id)
    }

    fn set_task_item_terminal_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        let conn = self.connection()?;
        items::set_task_item_terminal_status(&conn, task_item_id, status)
    }

    fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        let conn = self.connection()?;
        items::update_task_item_status(&conn, task_item_id, status)
    }

    fn update_task_item_pipeline_vars(
        &self,
        task_item_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        let conn = self.connection()?;
        items::update_task_item_pipeline_vars(&conn, task_item_id, pipeline_vars_json)
    }

    fn update_task_item_tickets(
        &self,
        task_item_id: &str,
        ticket_files_json: &str,
        ticket_content_json: &str,
    ) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_task_item_tickets(
            &conn,
            task_item_id,
            ticket_files_json,
            ticket_content_json,
        )
    }
}

// ─�� CommandRunRepository ────────────────────────────────────────────

impl CommandRunRepository for SqliteTaskRepository {
    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = self.connection()?;
        items::insert_command_run(&conn, run)
    }

    fn update_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_command_run(&conn, run)
    }

    fn update_command_run_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_command_run_with_events(&conn, run, events)
    }

    fn persist_phase_result_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        let conn = self.connection()?;
        write_ops::persist_phase_result_with_events(&conn, run, events)
    }

    fn update_command_run_pid(&self, run_id: &str, pid: i64) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_command_run_pid(&conn, run_id, pid)
    }

    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>> {
        let conn = self.connection()?;
        queries::list_task_log_runs(&conn, task_id, limit)
    }

    fn find_active_child_pids(&self, task_id: &str) -> Result<Vec<i64>> {
        let conn = self.connection()?;
        write_ops::find_active_child_pids(&conn, task_id)
    }

    fn find_inflight_command_runs_for_task(&self, task_id: &str) -> Result<Vec<InflightRunRecord>> {
        let conn = self.connection()?;
        write_ops::find_inflight_command_runs_for_task(&conn, task_id)
    }

    fn find_completed_runs_for_pending_items(
        &self,
        task_id: &str,
    ) -> Result<Vec<write_ops::CompletedRunRecord>> {
        let conn = self.connection()?;
        write_ops::find_completed_runs_for_pending_items(&conn, task_id)
    }
}

// ── EventRepository ─────────────────────────────────────��───────────

impl EventRepository for SqliteTaskRepository {
    fn insert_event(&self, event: &DbEventRecord) -> Result<()> {
        let conn = self.connection()?;
        write_ops::insert_event(&conn, event)
    }
}

// ── TaskGraphRepository ─────��───────────────────────────────────────

impl TaskGraphRepository for SqliteTaskRepository {
    fn insert_task_graph_run(&self, run: &NewTaskGraphRun) -> Result<()> {
        let conn = self.connection()?;
        queries::insert_task_graph_run(&conn, run)
    }

    fn update_task_graph_run_status(&self, graph_run_id: &str, status: &str) -> Result<()> {
        let conn = self.connection()?;
        queries::update_task_graph_run_status(&conn, graph_run_id, status)
    }

    fn insert_task_graph_snapshot(&self, snapshot: &NewTaskGraphSnapshot) -> Result<()> {
        let conn = self.connection()?;
        queries::insert_task_graph_snapshot(&conn, snapshot)
    }

    fn load_task_graph_debug_bundles(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::dto::TaskGraphDebugBundle>> {
        let conn = self.connection()?;
        queries::load_task_graph_debug_bundles(&conn, task_id)
    }
}

/// Async wrapper around [`SqliteTaskRepository`] built on [`AsyncDatabase`].
pub struct AsyncSqliteTaskRepository {
    async_db: Arc<AsyncDatabase>,
}

impl AsyncSqliteTaskRepository {
    /// Creates a new async repository wrapper.
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }

    // ── Read operations (use reader) ──

    /// Resolves a full task identifier from an ID prefix.
    pub async fn resolve_task_id(&self, prefix: &str) -> Result<String> {
        let prefix = prefix.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::resolve_task_id(conn, &prefix)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads a summary row for a task.
    pub async fn load_task_summary(&self, task_id: &str) -> Result<crate::dto::TaskSummary> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_summary(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads the full detail bundle for a task.
    pub async fn load_task_detail_rows(&self, task_id: &str) -> Result<TaskDetailRows> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_detail_rows(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads an uncapped timeline source snapshot at an optional event watermark.
    pub async fn load_task_timeline_source(
        &self,
        task_id: &str,
        max_event_id: Option<i64>,
    ) -> Result<crate::dto::TaskTimelineSource> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_timeline_source(conn, &task_id, max_event_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads `(total, resolved, unresolved)` item counts for a task.
    pub async fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_item_counts(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists task identifiers ordered by creation time descending.
    pub async fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>> {
        self.async_db
            .reader()
            .call(move |conn| {
                queries::list_task_ids_ordered_by_created_desc(conn)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns the latest resumable task, optionally including pending tasks.
    pub async fn find_latest_resumable_task_id(
        &self,
        include_pending: bool,
    ) -> Result<Option<String>> {
        self.async_db
            .reader()
            .call(move |conn| {
                queries::find_latest_resumable_task_id(conn, include_pending)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads execution state required to resume a task.
    pub async fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_runtime_row(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns the first task-item identifier for a task, if present.
    pub async fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::first_task_item_id(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Counts unresolved task items.
    pub async fn count_unresolved_items(&self, task_id: &str) -> Result<i64> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::count_unresolved_items(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists task items participating in the current cycle.
    pub async fn list_task_items_for_cycle(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::dto::TaskItemRow>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::list_task_items_for_cycle(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads the current task status string.
    pub async fn load_task_status(&self, task_id: &str) -> Result<Option<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_status(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads the human-readable task name.
    pub async fn load_task_name(&self, task_id: &str) -> Result<Option<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_name(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists recent command runs used for log inspection.
    pub async fn list_task_log_runs(
        &self,
        task_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLogRunRow>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::list_task_log_runs(conn, &task_id, limit)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Loads graph-planning debug bundles for a task.
    pub async fn load_task_graph_debug_bundles(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::dto::TaskGraphDebugBundle>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::load_task_graph_debug_bundles(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    // ── Write operations (use writer) ──

    /// Updates a task status and optionally marks completion.
    pub async fn set_task_status(
        &self,
        task_id: &str,
        status: &str,
        set_completed: bool,
    ) -> Result<()> {
        let task_id = task_id.to_owned();
        let status = status.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                state::set_task_status(conn, &task_id, &status, set_completed)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Resets unresolved items back to pending without changing the task status.
    /// Called before enqueuing a task so the worker can re-process them.
    pub async fn reset_unresolved_items(&self, task_id: &str) -> Result<()> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                state::reset_unresolved_items(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Resets a task into a fresh batch-start state.
    pub async fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                state::prepare_task_for_start_batch(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists cycle counters and `init_once` state.
    pub async fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                state::update_task_cycle_state(conn, &task_id, current_cycle, init_done)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Marks a task item as running.
    pub async fn mark_task_item_running(&self, task_item_id: &str) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::mark_task_item_running(conn, &task_item_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Sets a terminal status for a task item.
    pub async fn set_task_item_terminal_status(
        &self,
        task_item_id: &str,
        status: &str,
    ) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        let status = status.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::set_task_item_terminal_status(conn, &task_item_id, &status)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Updates a task item to an arbitrary status.
    pub async fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        let status = status.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::update_task_item_status(conn, &task_item_id, &status)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists accumulated pipeline variables back to the task item's dynamic_vars column.
    pub async fn update_task_item_pipeline_vars(
        &self,
        task_item_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        let pipeline_vars_json = pipeline_vars_json.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::update_task_item_pipeline_vars(conn, &task_item_id, &pipeline_vars_json)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Lists ids of terminal tasks older than `retention_days`, capped at `limit`.
    pub async fn list_terminal_tasks_older_than(
        &self,
        retention_days: u32,
        limit: u32,
    ) -> Result<Vec<String>> {
        self.async_db
            .reader()
            .call(move |conn| {
                queries::list_terminal_tasks_older_than(conn, retention_days, limit)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// References with no recorded disposition that would refuse a delete of
    /// `task_id`, in schema order. Empty means the delete would not be refused
    /// for that reason.
    ///
    /// Read-only, and offered so a caller can find out *before* taking an
    /// action it cannot undo. `delete_task` stops the task's runtime first, and
    /// a delete that is then refused would leave the task stopped for nothing.
    /// This does not make the delete itself conditional on an earlier read —
    /// the delete re-checks under its own transaction and is the authority.
    pub async fn references_blocking_delete(&self, task_id: &str) -> Result<Vec<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                (|| -> Result<Vec<String>> {
                    let refs = references::blocking_references(conn)?;
                    let holding = references::references_holding(conn, &refs, &task_id)?;
                    Ok(holding
                        .into_iter()
                        .filter(|held| {
                            let (table, column) = held.split_once('.').unwrap_or((held, ""));
                            references::disposition_for(table, column)
                                == references::Disposition::BlockAndReport
                        })
                        .collect())
                })()
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Deletes a task and returns log paths that should be removed.
    pub async fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::delete_task_and_collect_log_paths(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(Box::new(CarriedError(e))))
            })
            .await
            .map_err(recover_carried)
    }

    /// Inserts a command-run record.
    pub async fn insert_command_run(&self, run: NewCommandRun) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                items::insert_command_run(conn, &run)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Inserts an event record.
    pub async fn insert_event(&self, event: DbEventRecord) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::insert_event(conn, &event)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Updates an existing command-run record.
    pub async fn update_command_run(&self, run: NewCommandRun) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::update_command_run(conn, &run)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Updates a command run and appends emitted events.
    pub async fn update_command_run_with_events(
        &self,
        run: NewCommandRun,
        events: Vec<DbEventRecord>,
    ) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::update_command_run_with_events(conn, &run, &events)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists a completed phase result together with emitted events.
    pub async fn persist_phase_result_with_events(
        &self,
        run: NewCommandRun,
        events: Vec<DbEventRecord>,
    ) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::persist_phase_result_with_events(conn, &run, &events)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Updates the PID associated with a running command.
    pub async fn update_command_run_pid(&self, run_id: &str, pid: i64) -> Result<()> {
        let run_id = run_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::update_command_run_pid(conn, &run_id, pid)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns active child PIDs for a task.
    pub async fn find_active_child_pids(&self, task_id: &str) -> Result<Vec<i64>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                write_ops::find_active_child_pids(conn, &task_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns in-flight command runs for a task (FR-038).
    pub async fn find_inflight_command_runs_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<InflightRunRecord>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                write_ops::find_inflight_command_runs_for_task(conn, &task_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Returns completed runs whose parent items are still `pending` (FR-038).
    pub async fn find_completed_runs_for_pending_items(
        &self,
        task_id: &str,
    ) -> Result<Vec<write_ops::CompletedRunRecord>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                write_ops::find_completed_runs_for_pending_items(conn, &task_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Counts stale pending items (FR-038).
    pub async fn count_stale_pending_items(&self, task_id: &str) -> Result<i64> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                queries::count_stale_pending_items(conn, &task_id)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// FR-052: Counts recent heartbeat events for specified item IDs since cutoff.
    pub async fn count_recent_heartbeats_for_items(
        &self,
        task_id: &str,
        item_ids: &[String],
        cutoff_ts: &str,
    ) -> Result<i64> {
        let task_id = task_id.to_owned();
        let item_ids = item_ids.to_vec();
        let cutoff_ts = cutoff_ts.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                write_ops::count_recent_heartbeats_for_items(conn, &task_id, &item_ids, &cutoff_ts)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists the serialized pipeline-variable map for a task.
    pub async fn update_task_pipeline_vars(
        &self,
        task_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        let task_id = task_id.to_owned();
        let pipeline_vars_json = pipeline_vars_json.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::update_task_pipeline_vars(conn, &task_id, &pipeline_vars_json)
                    .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists active ticket paths and preview content for a task item.
    pub async fn update_task_item_tickets(
        &self,
        task_item_id: &str,
        ticket_files_json: &str,
        ticket_content_json: &str,
    ) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        let ticket_files_json = ticket_files_json.to_owned();
        let ticket_content_json = ticket_content_json.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                write_ops::update_task_item_tickets(
                    conn,
                    &task_item_id,
                    &ticket_files_json,
                    &ticket_content_json,
                )
                .map_err(|err| tokio_rusqlite::Error::Other(err.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Inserts a task-graph planning run.
    pub async fn insert_task_graph_run(&self, run: NewTaskGraphRun) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                queries::insert_task_graph_run(conn, &run)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Updates the status of a task-graph planning run.
    pub async fn update_task_graph_run_status(
        &self,
        graph_run_id: &str,
        status: &str,
    ) -> Result<()> {
        let graph_run_id = graph_run_id.to_owned();
        let status = status.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                queries::update_task_graph_run_status(conn, &graph_run_id, &status)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Blanket-pause all running tasks and reset their items to pending.
    /// Used during daemon shutdown before exec() to prevent orphaned state.
    pub async fn pause_all_running_tasks_and_items(&self) -> Result<usize> {
        self.async_db
            .writer()
            .call(move |conn| {
                state::pause_all_running_tasks_and_items(conn)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Pauses only tasks in `restart_pending` status and resets their running items.
    pub async fn pause_restart_pending_tasks_and_items(&self) -> Result<usize> {
        self.async_db
            .writer()
            .call(move |conn| {
                state::pause_restart_pending_tasks_and_items(conn)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Recovers all orphaned running items across all tasks.
    pub async fn recover_orphaned_running_items(&self) -> Result<Vec<(String, Vec<String>)>> {
        self.async_db
            .writer()
            .call(move |conn| {
                state::recover_orphaned_running_items(conn)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Recovers orphaned running items for a single task.
    pub async fn recover_orphaned_running_items_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                state::recover_orphaned_running_items_for_task(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Recovers stalled running items older than the given threshold.
    ///
    /// Tasks in `exclude_task_ids` are skipped (they have active workers).
    pub async fn recover_stalled_running_items(
        &self,
        stall_threshold_secs: u64,
        exclude_task_ids: std::collections::HashSet<String>,
    ) -> Result<Vec<(String, Vec<String>)>> {
        self.async_db
            .writer()
            .call(move |conn| {
                state::recover_stalled_running_items(conn, stall_threshold_secs, &exclude_task_ids)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }

    /// Persists one task-graph snapshot payload.
    pub async fn insert_task_graph_snapshot(&self, snapshot: NewTaskGraphSnapshot) -> Result<()> {
        self.async_db
            .writer()
            .call(move |conn| {
                queries::insert_task_graph_snapshot(conn, &snapshot)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
    }
}
