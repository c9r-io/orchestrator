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
pub use types::{
    NewTaskGraphRun, NewTaskGraphSnapshot, TaskLogRunRow, TaskRepositoryConn, TaskRepositorySource,
    TaskRuntimeRow,
};

use crate::async_database::{flatten_err, AsyncDatabase};
use crate::dto::{CommandRunDto, EventDto, TaskGraphDebugBundle, TaskItemDto};
use anyhow::Result;
use std::sync::Arc;

pub type TaskDetailRows = (
    Vec<TaskItemDto>,
    Vec<CommandRunDto>,
    Vec<EventDto>,
    Vec<TaskGraphDebugBundle>,
);

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
        assert_eq!(status.as_deref(), Some("pending"));

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
}
