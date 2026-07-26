use crate::async_database::AsyncDatabase;
use crate::task_repository::{AsyncSqliteTaskRepository, NewCommandRun};
use anyhow::Result;
use std::sync::Arc;

pub use crate::task_repository::DbEventRecord;

/// Async facade for persistence writes that need serialized database access.
pub struct DbWriteCoordinator {
    repo: AsyncSqliteTaskRepository,
}

impl DbWriteCoordinator {
    /// Creates a database write coordinator backed by the async task repository.
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self {
            repo: AsyncSqliteTaskRepository::new(async_db),
        }
    }

    /// Inserts one event row for a task or task item.
    pub async fn insert_event(
        &self,
        task_id: &str,
        task_item_id: Option<&str>,
        event_type: &str,
        payload_json: &str,
    ) -> Result<()> {
        self.repo
            .insert_event(DbEventRecord {
                task_id: task_id.to_owned(),
                task_item_id: task_item_id.map(str::to_owned),
                event_type: event_type.to_owned(),
                payload_json: payload_json.to_owned(),
            })
            .await
    }

    /// Updates task status and optionally marks completion time.
    pub async fn set_task_status(
        &self,
        task_id: &str,
        status: &str,
        set_completed: bool,
    ) -> Result<()> {
        self.repo
            .set_task_status(task_id, status, set_completed)
            .await
    }

    /// Inserts a command run by cloning the provided payload.
    pub async fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        self.insert_command_run_owned(run.clone()).await
    }

    /// Inserts a command run using an owned payload.
    pub async fn insert_command_run_owned(&self, run: NewCommandRun) -> Result<()> {
        self.repo.insert_command_run(run).await
    }

    /// Updates a command run by cloning the provided payload.
    pub async fn update_command_run(&self, run: &NewCommandRun) -> Result<()> {
        self.update_command_run_owned(run.clone()).await
    }

    /// Updates a command run using an owned payload.
    pub async fn update_command_run_owned(&self, run: NewCommandRun) -> Result<()> {
        self.repo.update_command_run(run).await
    }

    /// Updates a command run and appends follow-up events.
    pub async fn update_command_run_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        self.update_command_run_with_owned_events(run.clone(), events.to_vec())
            .await
    }

    /// Updates a command run and appends owned follow-up events.
    pub async fn update_command_run_with_owned_events(
        &self,
        run: NewCommandRun,
        events: Vec<DbEventRecord>,
    ) -> Result<()> {
        self.repo.update_command_run_with_events(run, events).await
    }

    /// Persists one completed phase result with an optional event.
    pub async fn persist_phase_result(
        &self,
        run: &NewCommandRun,
        event: Option<DbEventRecord>,
    ) -> Result<()> {
        let events = match event {
            Some(single) => vec![single],
            None => Vec::new(),
        };
        self.persist_phase_result_with_events(run, &events).await
    }

    /// Persists one completed phase result with borrowed events.
    pub async fn persist_phase_result_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        self.persist_phase_result_with_owned_events(run.clone(), events.to_vec())
            .await
    }

    /// Persists one completed phase result with owned events.
    pub async fn persist_phase_result_with_owned_events(
        &self,
        run: NewCommandRun,
        events: Vec<DbEventRecord>,
    ) -> Result<()> {
        self.repo
            .persist_phase_result_with_events(run, events)
            .await
    }

    /// Updates the recorded process id for an in-flight command run.
    pub async fn update_command_run_pid(&self, run_id: &str, pid: i64) -> Result<()> {
        self.repo.update_command_run_pid(run_id, pid).await
    }

    /// Returns active child process ids associated with a task.
    pub async fn find_active_child_pids(&self, task_id: &str) -> Result<Vec<i64>> {
        self.repo.find_active_child_pids(task_id).await
    }

    /// Returns in-flight command runs for a task (FR-038).
    pub async fn find_inflight_command_runs_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::task_repository::InflightRunRecord>> {
        self.repo.find_inflight_command_runs_for_task(task_id).await
    }

    /// Returns completed runs whose parent items are still `pending` (FR-038).
    pub async fn find_completed_runs_for_pending_items(
        &self,
        task_id: &str,
    ) -> Result<Vec<crate::task_repository::CompletedRunRecord>> {
        self.repo
            .find_completed_runs_for_pending_items(task_id)
            .await
    }

    /// Counts stale pending items (FR-038).
    pub async fn count_stale_pending_items(&self, task_id: &str) -> Result<i64> {
        self.repo.count_stale_pending_items(task_id).await
    }

    /// Updates task-cycle counters and init-step state.
    pub async fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()> {
        self.repo
            .update_task_cycle_state(task_id, current_cycle, init_done)
            .await
    }

    /// Updates the status of one task item.
    pub async fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        self.repo
            .update_task_item_status(task_item_id, status)
            .await
    }

    /// Marks one task item as running.
    pub async fn mark_task_item_running(&self, task_item_id: &str) -> Result<()> {
        self.repo.mark_task_item_running(task_item_id).await
    }

    /// Sets one task item to a terminal status.
    pub async fn set_task_item_terminal_status(
        &self,
        task_item_id: &str,
        status: &str,
    ) -> Result<()> {
        self.repo
            .set_task_item_terminal_status(task_item_id, status)
            .await
    }

    /// Replaces the task-level pipeline variable snapshot.
    pub async fn update_task_pipeline_vars(
        &self,
        task_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        self.repo
            .update_task_pipeline_vars(task_id, pipeline_vars_json)
            .await
    }

    /// Sync-compatible alias for [`Self::update_task_pipeline_vars`].
    pub async fn update_task_pipeline_vars_sync(
        &self,
        task_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        self.update_task_pipeline_vars(task_id, pipeline_vars_json)
            .await
    }

    /// Persists accumulated pipeline variables back to the task item's dynamic_vars column.
    pub async fn update_task_item_pipeline_vars(
        &self,
        task_item_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        self.repo
            .update_task_item_pipeline_vars(task_item_id, pipeline_vars_json)
            .await
    }

    /// Replaces the ticket file and preview payloads for one task item.
    pub async fn update_task_item_tickets(
        &self,
        task_item_id: &str,
        ticket_files_json: &str,
        ticket_content_json: &str,
    ) -> Result<()> {
        self.repo
            .update_task_item_tickets(task_item_id, ticket_files_json, ticket_content_json)
            .await
    }
}
