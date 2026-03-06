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

#[cfg(test)]
mod tests;

pub use command_run::NewCommandRun;
pub use trait_def::TaskRepository;
pub use types::{TaskLogRunRow, TaskRepositoryConn, TaskRepositorySource, TaskRuntimeRow};

use crate::async_database::{flatten_err, AsyncDatabase};
use anyhow::Result;
use std::sync::Arc;

pub struct SqliteTaskRepository {
    source: types::TaskRepositorySource,
}

impl SqliteTaskRepository {
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

    fn load_task_detail_rows(
        &self,
        task_id: &str,
    ) -> Result<(
        Vec<crate::dto::TaskItemDto>,
        Vec<crate::dto::CommandRunDto>,
        Vec<crate::dto::EventDto>,
    )> {
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

    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>> {
        let conn = self.connection()?;
        items::delete_task_and_collect_log_paths(&conn, task_id)
    }

    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = self.connection()?;
        items::insert_command_run(&conn, run)
    }
}

pub struct AsyncSqliteTaskRepository {
    async_db: Arc<AsyncDatabase>,
}

impl AsyncSqliteTaskRepository {
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }

    // ── Read operations (use reader) ──

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

    pub async fn load_task_detail_rows(
        &self,
        task_id: &str,
    ) -> Result<(
        Vec<crate::dto::TaskItemDto>,
        Vec<crate::dto::CommandRunDto>,
        Vec<crate::dto::EventDto>,
    )> {
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

    // ── Write operations (use writer) ──

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
}
