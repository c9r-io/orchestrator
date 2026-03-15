#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

mod command_run;
mod items;
mod queries;
mod state;
mod trait_def;
mod types;
mod write_ops;

#[cfg(test)]
mod tests;

pub use command_run::NewCommandRun;
pub use trait_def::TaskRepository;
pub use types::{
    DbEventRecord, NewTaskGraphRun, NewTaskGraphSnapshot, TaskLogRunRow, TaskRepositoryConn,
    TaskRepositorySource, TaskRuntimeRow,
};
pub use write_ops::{CompletedRunRecord, InflightRunRecord};

use crate::async_database::{flatten_err, AsyncDatabase};
use crate::dto::{CommandRunDto, EventDto, TaskGraphDebugBundle, TaskItemDto};
use anyhow::Result;
use std::sync::Arc;

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

impl TaskRepository for SqliteTaskRepository {
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

    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::first_task_item_id(&conn, task_id)
    }

    fn count_unresolved_items(&self, task_id: &str) -> Result<i64> {
        let conn = self.connection()?;
        queries::count_unresolved_items(&conn, task_id)
    }

    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<crate::dto::TaskItemRow>> {
        let conn = self.connection()?;
        queries::list_task_items_for_cycle(&conn, task_id)
    }

    fn load_task_status(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::load_task_status(&conn, task_id)
    }

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

    fn load_task_name(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.connection()?;
        queries::load_task_name(&conn, task_id)
    }

    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>> {
        let conn = self.connection()?;
        queries::list_task_log_runs(&conn, task_id, limit)
    }

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

    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let conn = self.connection()?;
        items::delete_task_and_collect_log_paths(&conn, task_id)
    }

    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = self.connection()?;
        items::insert_command_run(&conn, run)
    }

    fn insert_event(&self, event: &DbEventRecord) -> Result<()> {
        let conn = self.connection()?;
        write_ops::insert_event(&conn, event)
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

    fn update_task_pipeline_vars(&self, task_id: &str, pipeline_vars_json: &str) -> Result<()> {
        let conn = self.connection()?;
        write_ops::update_task_pipeline_vars(&conn, task_id, pipeline_vars_json)
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

    /// Deletes a task and returns log paths that should be removed.
    pub async fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let task_id = task_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                items::delete_task_and_collect_log_paths(conn, &task_id)
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
            })
            .await
            .map_err(flatten_err)
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
    pub async fn recover_stalled_running_items(
        &self,
        stall_threshold_secs: u64,
    ) -> Result<Vec<(String, Vec<String>)>> {
        self.async_db
            .writer()
            .call(move |conn| {
                state::recover_stalled_running_items(conn, stall_threshold_secs)
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

#[cfg(test)]
mod async_wrapper_tests {
    use super::*;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/async-repo.md");
        std::fs::write(&qa_file, "# async repo\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("async-repo".to_string()),
                goal: Some("async-repo-goal".to_string()),
                ..CreateTaskPayload::default()
            },
        )
        .expect("create task");
        (state, created.id)
    }

    fn first_item_id(state: &crate::state::InnerState, task_id: &str) -> String {
        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        conn.query_row(
            "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("load item id")
    }

    #[tokio::test]
    async fn async_repository_read_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;

        let resolved = repo
            .resolve_task_id(&task_id[..8])
            .await
            .expect("resolve task id");
        assert_eq!(resolved, task_id);

        let summary = repo
            .load_task_summary(&task_id)
            .await
            .expect("load summary");
        assert_eq!(summary.name, "async-repo");

        let detail = repo
            .load_task_detail_rows(&task_id)
            .await
            .expect("load detail rows");
        assert!(!detail.0.is_empty());

        let counts = repo
            .load_task_item_counts(&task_id)
            .await
            .expect("load item counts");
        assert!(counts.0 >= 1);

        let ids = repo
            .list_task_ids_ordered_by_created_desc()
            .await
            .expect("list task ids");
        assert_eq!(ids[0], task_id);

        let resumable = repo
            .find_latest_resumable_task_id(true)
            .await
            .expect("find latest resumable");
        assert_eq!(resumable.as_deref(), Some(task_id.as_str()));

        let runtime = repo
            .load_task_runtime_row(&task_id)
            .await
            .expect("load runtime row");
        assert_eq!(runtime.goal, "async-repo-goal");

        let item_id = repo
            .first_task_item_id(&task_id)
            .await
            .expect("first item id query");
        assert!(item_id.is_some());

        let unresolved = repo
            .count_unresolved_items(&task_id)
            .await
            .expect("count unresolved");
        assert_eq!(unresolved, 0);

        let items = repo
            .list_task_items_for_cycle(&task_id)
            .await
            .expect("list items for cycle");
        assert!(!items.is_empty());

        let status = repo.load_task_status(&task_id).await.expect("load status");
        assert_eq!(status.as_deref(), Some("created"));

        let name = repo.load_task_name(&task_id).await.expect("load task name");
        assert_eq!(name.as_deref(), Some("async-repo"));

        let runs = repo
            .list_task_log_runs(&task_id, 10)
            .await
            .expect("list log runs");
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn async_repository_write_wrappers_update_task_and_item_state() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;
        let item_id = first_item_id(&state, &task_id);

        repo.mark_task_item_running(&item_id)
            .await
            .expect("mark item running");
        repo.update_task_item_status(&item_id, "qa_failed")
            .await
            .expect("update item status");
        repo.set_task_item_terminal_status(&item_id, "completed")
            .await
            .expect("set terminal status");
        repo.set_task_status(&task_id, "running", false)
            .await
            .expect("set task status");
        repo.update_task_cycle_state(&task_id, 3, true)
            .await
            .expect("update task cycle state");

        let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
        let task_row: (String, i64, i64) = conn
            .query_row(
                "SELECT status, current_cycle, init_done FROM tasks WHERE id = ?1",
                rusqlite::params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("load task row");
        assert_eq!(task_row.0, "running");
        assert_eq!(task_row.1, 3);
        assert_eq!(task_row.2, 1);

        let item_status: String = conn
            .query_row(
                "SELECT status FROM task_items WHERE id = ?1",
                rusqlite::params![item_id],
                |row| row.get(0),
            )
            .expect("load item status");
        assert_eq!(item_status, "completed");

        repo.set_task_status(&task_id, "paused", false)
            .await
            .expect("pause task before prepare");
        repo.prepare_task_for_start_batch(&task_id)
            .await
            .expect("prepare task for start");
        let prepared_status: String = conn
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                rusqlite::params![task_id],
                |row| row.get(0),
            )
            .expect("reload task status");
        assert_eq!(prepared_status, "running");
    }

    #[tokio::test]
    async fn async_repository_insert_and_delete_command_runs() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;
        let item_id = first_item_id(&state, &task_id);
        let stdout_path = state.app_root.join("logs/async-wrapper-stdout.log");
        let stderr_path = state.app_root.join("logs/async-wrapper-stderr.log");
        std::fs::create_dir_all(
            stdout_path
                .parent()
                .expect("stdout parent directory should exist"),
        )
        .expect("create logs dir");
        std::fs::write(&stdout_path, "stdout").expect("write stdout");
        std::fs::write(&stderr_path, "stderr").expect("write stderr");

        repo.insert_command_run(NewCommandRun {
            id: "async-run-1".to_string(),
            task_item_id: item_id,
            phase: "qa".to_string(),
            command: "echo repo".to_string(),
            cwd: state.app_root.display().to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: stdout_path.display().to_string(),
            stderr_path: stderr_path.display().to_string(),
            started_at: crate::config_load::now_ts(),
            ended_at: crate::config_load::now_ts(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: Some(1.0),
            quality_score: Some(1.0),
            validation_status: "passed".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        })
        .await
        .expect("insert command run");

        let runs = repo
            .list_task_log_runs(&task_id, 10)
            .await
            .expect("list runs after insert");
        assert_eq!(runs.len(), 1);

        let paths = repo
            .delete_task_and_collect_log_paths(&task_id)
            .await
            .expect("delete task and collect log paths");
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .any(|path| path.ends_with("async-wrapper-stdout.log")));
    }

    #[test]
    fn sqlite_repository_graph_debug_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo =
            SqliteTaskRepository::new(types::TaskRepositorySource::from(state.db_path.clone()));

        repo.insert_task_graph_run(&NewTaskGraphRun {
            graph_run_id: "sync-graph-run".to_string(),
            task_id: task_id.clone(),
            cycle: 4,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: Some("static_segment".to_string()),
            planner_failure_class: None,
            planner_failure_message: None,
            entry_node_id: Some("qa".to_string()),
            node_count: 2,
            edge_count: 1,
        })
        .expect("insert graph run");
        repo.update_task_graph_run_status("sync-graph-run", "completed")
            .expect("update graph run");
        repo.insert_task_graph_snapshot(&NewTaskGraphSnapshot {
            graph_run_id: "sync-graph-run".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"qa\"}".to_string(),
        })
        .expect("insert graph snapshot");

        let bundles = repo
            .load_task_graph_debug_bundles(&task_id)
            .expect("load graph bundles");
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].graph_run_id, "sync-graph-run");
        assert_eq!(bundles[0].status, "completed");
        assert_eq!(bundles[0].effective_graph_json, "{\"entry\":\"qa\"}");
    }

    #[tokio::test]
    async fn async_repository_graph_debug_wrappers_round_trip() {
        let mut fixture = TestState::new();
        let (state, task_id) = seed_task(&mut fixture);
        let repo = &state.task_repo;

        repo.insert_task_graph_run(NewTaskGraphRun {
            graph_run_id: "async-graph-run".to_string(),
            task_id: task_id.clone(),
            cycle: 5,
            mode: "dynamic_dag".to_string(),
            source: "adaptive_planner".to_string(),
            status: "materialized".to_string(),
            fallback_mode: Some("deterministic_dag".to_string()),
            planner_failure_class: Some("invalid_json".to_string()),
            planner_failure_message: Some("planner output broken".to_string()),
            entry_node_id: Some("fix".to_string()),
            node_count: 3,
            edge_count: 2,
        })
        .await
        .expect("insert graph run");
        repo.update_task_graph_run_status("async-graph-run", "completed")
            .await
            .expect("update graph run");
        repo.insert_task_graph_snapshot(NewTaskGraphSnapshot {
            graph_run_id: "async-graph-run".to_string(),
            task_id: task_id.clone(),
            snapshot_kind: "effective_graph".to_string(),
            payload_json: "{\"entry\":\"fix\"}".to_string(),
        })
        .await
        .expect("insert graph snapshot");

        let bundles = repo
            .load_task_graph_debug_bundles(&task_id)
            .await
            .expect("load graph bundles");
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].graph_run_id, "async-graph-run");
        assert_eq!(
            bundles[0].fallback_mode.as_deref(),
            Some("deterministic_dag")
        );
        assert_eq!(bundles[0].effective_graph_json, "{\"entry\":\"fix\"}");
    }
}
