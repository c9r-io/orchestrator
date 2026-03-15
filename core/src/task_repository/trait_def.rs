use crate::dto::{TaskGraphDebugBundle, TaskItemRow, TaskSummary};
use anyhow::Result;

use super::command_run::NewCommandRun;
use super::types::{DbEventRecord, TaskLogRunRow, TaskRuntimeRow};
use super::TaskDetailRows;

/// Synchronous repository interface for task, run, and event persistence.
pub trait TaskRepository {
    /// Resolves a full task identifier from an exact ID or prefix.
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String>;
    /// Loads the summary row for a task.
    fn load_task_summary(&self, task_id: &str) -> Result<TaskSummary>;
    /// Loads the full detail row bundle for a task.
    fn load_task_detail_rows(&self, task_id: &str) -> Result<TaskDetailRows>;
    /// Loads `(total, resolved, unresolved)` item counts for a task.
    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)>;
    /// Lists task identifiers ordered from newest to oldest.
    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>>;
    /// Returns the latest resumable task, optionally including pending tasks.
    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>>;
    /// Loads execution state needed to resume a task.
    fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow>;
    /// Returns the first task-item identifier for a task, if any.
    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>>;
    /// Counts unresolved items for the task.
    fn count_unresolved_items(&self, task_id: &str) -> Result<i64>;
    /// Lists task items participating in the current cycle.
    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<TaskItemRow>>;
    /// Loads the current task status string.
    fn load_task_status(&self, task_id: &str) -> Result<Option<String>>;
    /// Updates the task status and optionally stamps completion metadata.
    fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()>;
    /// Prepares a task for a fresh start by resetting batch-execution state.
    fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()>;
    /// Persists cycle counters and `init_once` completion state for a task.
    fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()>;
    /// Marks a task item as currently running.
    fn mark_task_item_running(&self, task_item_id: &str) -> Result<()>;
    /// Sets a terminal status for a task item.
    fn set_task_item_terminal_status(&self, task_item_id: &str, status: &str) -> Result<()>;
    /// Updates a task item to an arbitrary status value.
    fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()>;
    /// Loads the human-readable name of a task.
    fn load_task_name(&self, task_id: &str) -> Result<Option<String>>;
    /// Lists recent command runs for log streaming or inspection.
    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>>;
    /// Inserts a new task-graph planning run record.
    fn insert_task_graph_run(&self, run: &super::types::NewTaskGraphRun) -> Result<()>;
    /// Updates the status of an existing task-graph run.
    fn update_task_graph_run_status(&self, graph_run_id: &str, status: &str) -> Result<()>;
    /// Persists one task-graph snapshot.
    fn insert_task_graph_snapshot(
        &self,
        snapshot: &super::types::NewTaskGraphSnapshot,
    ) -> Result<()>;
    /// Loads debug bundles for graph-planning diagnostics.
    fn load_task_graph_debug_bundles(&self, task_id: &str) -> Result<Vec<TaskGraphDebugBundle>>;
    /// Deletes a task and returns log paths that should be cleaned up afterward.
    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>>;
    /// Inserts a command-run record.
    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()>;
    /// Inserts an event record.
    fn insert_event(&self, event: &DbEventRecord) -> Result<()>;
    /// Updates an existing command-run record.
    fn update_command_run(&self, run: &NewCommandRun) -> Result<()>;
    /// Updates a command run and appends events in a single repository call.
    fn update_command_run_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()>;
    /// Persists a completed phase result together with its emitted events.
    fn persist_phase_result_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()>;
    /// Updates the operating-system PID associated with a running command.
    fn update_command_run_pid(&self, run_id: &str, pid: i64) -> Result<()>;
    /// Returns active child PIDs for a task.
    fn find_active_child_pids(&self, task_id: &str) -> Result<Vec<i64>>;
    /// Returns in-flight command runs for a task (FR-038).
    fn find_inflight_command_runs_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<super::write_ops::InflightRunRecord>>;
    /// Returns completed runs whose parent items are still `pending` (FR-038).
    fn find_completed_runs_for_pending_items(
        &self,
        task_id: &str,
    ) -> Result<Vec<super::write_ops::CompletedRunRecord>>;
    /// Counts stale pending items (FR-038).
    fn count_stale_pending_items(&self, task_id: &str) -> Result<i64>;
    /// Counts recent heartbeat events for specified item IDs since cutoff (FR-052).
    fn count_recent_heartbeats_for_items(
        &self,
        task_id: &str,
        item_ids: &[String],
        cutoff_ts: &str,
    ) -> Result<i64>;
    /// Persists the serialized pipeline-variable map for a task.
    fn update_task_pipeline_vars(&self, task_id: &str, pipeline_vars_json: &str) -> Result<()>;
    /// Persists the active ticket lists and preview content for a task item.
    fn update_task_item_tickets(
        &self,
        task_item_id: &str,
        ticket_files_json: &str,
        ticket_content_json: &str,
    ) -> Result<()>;
}
