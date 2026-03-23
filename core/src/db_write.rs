use crate::async_database::AsyncDatabase;
use crate::task_repository::{AsyncSqliteTaskRepository, NewCommandRun};
use anyhow::Result;
use std::sync::Arc;

#[cfg(test)]
use rusqlite::params;

pub use crate::task_repository::DbEventRecord;

/// Async facade for persistence writes that need serialized database access.
pub struct DbWriteCoordinator {
    repo: AsyncSqliteTaskRepository,
}

#[cfg(test)]
fn extract_event_promoted_fields(
    payload_json: &str,
) -> (Option<String>, Option<String>, Option<i64>) {
    let value: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(value) => value,
        Err(_) => return (None, None, None),
    };
    let step = value["step"]
        .as_str()
        .or_else(|| value["phase"].as_str())
        .map(str::to_owned);
    let step_scope = value["step_scope"].as_str().map(str::to_owned);
    let cycle = value["cycle"].as_i64();
    (step, step_scope, cycle)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;

    /// Helper: build a TestState, seed a QA file, create a task, return (state, task_id, first task_item_id).
    fn setup_task() -> (std::sync::Arc<crate::state::InnerState>, String, String) {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let qa_file = state
            .data_dir
            .join("workspace/default/docs/qa/db_write_test.md");
        std::fs::write(&qa_file, "# db_write test\n").expect("seed qa file");

        let created = create_task_impl(&state, CreateTaskPayload::default()).expect("create task");
        let task_id = created.id.clone();

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let item_id: String = conn
            .query_row(
                "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("fetch first task_item_id");

        // Leak fixture so the temp dir survives for the test
        std::mem::forget(fixture);

        (state, task_id, item_id)
    }

    // ── insert_event ──

    #[tokio::test]
    async fn insert_event_stores_row() {
        let (state, task_id, _item_id) = setup_task();

        state
            .db_writer
            .insert_event(&task_id, None, "test_event", r#"{"key":"value"}"#)
            .await
            .expect("insert_event");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (evt_type, payload): (String, String) = conn
            .query_row(
                "SELECT event_type, payload_json FROM events WHERE task_id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query event");

        assert_eq!(evt_type, "test_event");
        assert_eq!(payload, r#"{"key":"value"}"#);
    }

    #[tokio::test]
    async fn insert_event_with_task_item_id() {
        let (state, task_id, item_id) = setup_task();

        state
            .db_writer
            .insert_event(&task_id, Some(&item_id), "item_evt", "{}")
            .await
            .expect("insert_event with item_id");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let stored_item_id: String = conn
            .query_row(
                "SELECT task_item_id FROM events WHERE task_id = ?1 AND event_type = 'item_evt'",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query event item_id");

        assert_eq!(stored_item_id, item_id);
    }

    // ── set_task_status ──

    #[tokio::test]
    async fn set_task_status_completed_sets_completed_at() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, completed_at): (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");

        assert_eq!(status, "completed");
        assert!(completed_at.is_some(), "completed_at should be set");
    }

    #[tokio::test]
    async fn set_task_status_running_clears_completed_at() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        state
            .db_writer
            .set_task_status(&task_id, "running", false)
            .await
            .expect("set running");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, completed_at): (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");

        assert_eq!(status, "running");
        assert!(
            completed_at.is_none(),
            "completed_at should be cleared for running"
        );
    }

    #[tokio::test]
    async fn set_task_status_pending_clears_completed_at() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        state
            .db_writer
            .set_task_status(&task_id, "pending", false)
            .await
            .expect("set pending");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let completed_at: Option<String> = conn
            .query_row(
                "SELECT completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query task");

        assert!(
            completed_at.is_none(),
            "completed_at should be cleared for pending"
        );
    }

    #[tokio::test]
    async fn set_task_status_other_preserves_completed_at() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        state
            .db_writer
            .set_task_status(&task_id, "failed", false)
            .await
            .expect("set failed");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, completed_at): (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");

        assert_eq!(status, "failed");
        assert!(
            completed_at.is_some(),
            "completed_at should be preserved for non-clearing status"
        );
    }

    // ── update_task_cycle_state ──

    #[tokio::test]
    async fn update_task_cycle_state_sets_fields() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .update_task_cycle_state(&task_id, 3, true)
            .await
            .expect("update cycle state");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (cycle, init_done): (i64, i64) = conn
            .query_row(
                "SELECT current_cycle, init_done FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");

        assert_eq!(cycle, 3);
        assert_eq!(init_done, 1);
    }

    #[tokio::test]
    async fn update_task_cycle_state_init_done_false() {
        let (state, task_id, _) = setup_task();

        state
            .db_writer
            .update_task_cycle_state(&task_id, 0, false)
            .await
            .expect("update cycle state");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let init_done: i64 = conn
            .query_row(
                "SELECT init_done FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query task");

        assert_eq!(init_done, 0);
    }

    // ── update_task_item_status ──

    #[tokio::test]
    async fn update_task_item_status_changes_status() {
        let (state, _task_id, item_id) = setup_task();

        state
            .db_writer
            .update_task_item_status(&item_id, "running")
            .await
            .expect("update item status");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let status: String = conn
            .query_row(
                "SELECT status FROM task_items WHERE id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .expect("query item");

        assert_eq!(status, "running");
    }

    #[tokio::test]
    async fn mark_task_item_running_sets_started_at() {
        let (state, _task_id, item_id) = setup_task();

        state
            .db_writer
            .mark_task_item_running(&item_id)
            .await
            .expect("mark task item running");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, started_at, completed_at): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, started_at, completed_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query item");

        assert_eq!(status, "running");
        assert!(started_at.is_some(), "started_at should be set");
        assert!(completed_at.is_none(), "completed_at should be cleared");
    }

    #[tokio::test]
    async fn set_task_item_terminal_status_sets_completed_at() {
        let (state, _task_id, item_id) = setup_task();

        state
            .db_writer
            .set_task_item_terminal_status(&item_id, "qa_passed")
            .await
            .expect("set task item terminal status");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, started_at, completed_at): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, started_at, completed_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query item");

        assert_eq!(status, "qa_passed");
        assert!(started_at.is_some(), "started_at should be backfilled");
        assert!(completed_at.is_some(), "completed_at should be set");
    }

    // ── update_task_item_tickets ──

    #[tokio::test]
    async fn update_task_item_tickets_sets_json() {
        let (state, _task_id, item_id) = setup_task();

        let files_json = r#"["ticket1.md","ticket2.md"]"#;
        let content_json = r#"[{"title":"bug"}]"#;

        state
            .db_writer
            .update_task_item_tickets(&item_id, files_json, content_json)
            .await
            .expect("update tickets");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (stored_files, stored_content): (String, String) = conn
            .query_row(
                "SELECT ticket_files_json, ticket_content_json FROM task_items WHERE id = ?1",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query item tickets");

        assert_eq!(stored_files, files_json);
        assert_eq!(stored_content, content_json);
    }

    // ── persist_phase_result ──

    fn make_command_run(item_id: &str) -> crate::task_repository::NewCommandRun {
        crate::task_repository::NewCommandRun {
            id: uuid::Uuid::new_v4().to_string(),
            task_item_id: item_id.to_string(),
            phase: "qa".to_string(),
            command: "echo test".to_string(),
            command_template: None,
            cwd: "/tmp".to_string(),
            workspace_id: "default".to_string(),
            agent_id: "echo".to_string(),
            exit_code: 0,
            stdout_path: "/tmp/stdout.log".to_string(),
            stderr_path: "/tmp/stderr.log".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            ended_at: "2026-01-01T00:00:01Z".to_string(),
            interrupted: 0,
            output_json: "{}".to_string(),
            artifacts_json: "[]".to_string(),
            confidence: Some(0.95),
            quality_score: None,
            validation_status: "pass".to_string(),
            session_id: None,
            machine_output_source: "stdout".to_string(),
            output_json_path: None,
        }
    }

    #[tokio::test]
    async fn persist_phase_result_without_event() {
        let (state, _task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .persist_phase_result(&run, None)
            .await
            .expect("persist_phase_result");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let phase: String = conn
            .query_row(
                "SELECT phase FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("query command_run");

        assert_eq!(phase, "qa");
    }

    #[tokio::test]
    async fn persist_phase_result_with_single_event() {
        let (state, task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        let event = DbEventRecord {
            task_id: task_id.clone(),
            task_item_id: Some(item_id.clone()),
            event_type: "phase_complete".to_string(),
            payload_json: r#"{"phase":"qa"}"#.to_string(),
        };

        state
            .db_writer
            .persist_phase_result(&run, Some(event))
            .await
            .expect("persist_phase_result with event");

        let conn = open_conn(&state.db_path).expect("open sqlite");

        let exit_code: i64 = conn
            .query_row(
                "SELECT exit_code FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("query command_run");
        assert_eq!(exit_code, 0);

        let evt_type: String = conn
            .query_row(
                "SELECT event_type FROM events WHERE task_id = ?1 AND event_type = 'phase_complete'",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query event");
        assert_eq!(evt_type, "phase_complete");
    }

    // ── persist_phase_result_with_events ──

    #[tokio::test]
    async fn persist_phase_result_with_multiple_events() {
        let (state, task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        let events = vec![
            DbEventRecord {
                task_id: task_id.clone(),
                task_item_id: Some(item_id.clone()),
                event_type: "started".to_string(),
                payload_json: "{}".to_string(),
            },
            DbEventRecord {
                task_id: task_id.clone(),
                task_item_id: None,
                event_type: "finished".to_string(),
                payload_json: r#"{"ok":true}"#.to_string(),
            },
        ];

        state
            .db_writer
            .persist_phase_result_with_events(&run, &events)
            .await
            .expect("persist with events");

        let conn = open_conn(&state.db_path).expect("open sqlite");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("count command_runs");
        assert_eq!(count, 1);

        let event_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("count events");
        assert_eq!(event_count, 2);
    }

    #[tokio::test]
    async fn persist_phase_result_with_empty_events() {
        let (state, _task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .persist_phase_result_with_events(&run, &[])
            .await
            .expect("persist with empty events");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("count command_runs");
        assert_eq!(count, 1);
    }

    // ── insert_command_run ──

    #[tokio::test]
    async fn insert_command_run_stores_fields() {
        let (state, _task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (phase, cmd, confidence): (String, String, Option<f64>) = conn
            .query_row(
                "SELECT phase, command, confidence FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query command_run");

        assert_eq!(phase, "qa");
        assert_eq!(cmd, "echo test");
        assert!((confidence.expect("confidence should be persisted") - 0.95).abs() < 0.01);
    }

    // ── extract_event_promoted_fields ──

    #[test]
    fn extract_event_promoted_fields_with_step_key() {
        let (step, step_scope, cycle) =
            extract_event_promoted_fields(r#"{"step":"qa","step_scope":"item","cycle":3}"#);
        assert_eq!(step, Some("qa".to_string()));
        assert_eq!(step_scope, Some("item".to_string()));
        assert_eq!(cycle, Some(3));
    }

    #[test]
    fn extract_event_promoted_fields_phase_fallback() {
        let (step, step_scope, cycle) =
            extract_event_promoted_fields(r#"{"phase":"fix","cycle":1}"#);
        assert_eq!(step, Some("fix".to_string()));
        assert_eq!(step_scope, None);
        assert_eq!(cycle, Some(1));
    }

    #[test]
    fn extract_event_promoted_fields_step_takes_priority_over_phase() {
        let (step, _, _) = extract_event_promoted_fields(r#"{"step":"qa","phase":"fix"}"#);
        assert_eq!(step, Some("qa".to_string()));
    }

    #[test]
    fn extract_event_promoted_fields_invalid_json() {
        let (step, step_scope, cycle) = extract_event_promoted_fields("not json at all");
        assert_eq!(step, None);
        assert_eq!(step_scope, None);
        assert_eq!(cycle, None);
    }

    #[test]
    fn extract_event_promoted_fields_empty_json_object() {
        let (step, step_scope, cycle) = extract_event_promoted_fields("{}");
        assert_eq!(step, None);
        assert_eq!(step_scope, None);
        assert_eq!(cycle, None);
    }

    #[test]
    fn extract_event_promoted_fields_cycle_as_string() {
        // cycle is a string, not an integer
        let (_, _, cycle) = extract_event_promoted_fields(r#"{"cycle":"not_a_number"}"#);
        assert_eq!(cycle, None);
    }

    // ── insert_event promoted fields verification ──

    #[tokio::test]
    async fn insert_event_stores_promoted_fields() {
        let (state, task_id, _item_id) = setup_task();

        state
            .db_writer
            .insert_event(
                &task_id,
                None,
                "phase_result",
                r#"{"step":"qa","step_scope":"task","cycle":5}"#,
            )
            .await
            .expect("insert_event");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (step, step_scope, cycle): (Option<String>, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT step, step_scope, cycle FROM events WHERE task_id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query promoted fields");

        assert_eq!(step, Some("qa".to_string()));
        assert_eq!(step_scope, Some("task".to_string()));
        assert_eq!(cycle, Some(5));
    }

    // ── update_command_run ──

    #[tokio::test]
    async fn update_command_run_updates_fields() {
        let (state, _task_id, item_id) = setup_task();
        let mut run = make_command_run(&item_id);
        let run_id = run.id.clone();

        // First insert the command run
        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        // Now update it
        run.exit_code = 0;
        run.ended_at = "2026-01-02T00:00:00Z".to_string();
        run.interrupted = 1;
        run.confidence = Some(0.99);
        run.validation_status = "verified".to_string();

        state
            .db_writer
            .update_command_run(&run)
            .await
            .expect("update_command_run");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (exit_code, ended_at, interrupted, confidence, validation): (
            Option<i64>,
            Option<String>,
            bool,
            Option<f64>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT exit_code, ended_at, interrupted, confidence, validation_status FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .expect("query updated command_run");

        assert_eq!(exit_code, Some(0));
        assert!(ended_at.is_some());
        assert!(interrupted);
        assert!((confidence.unwrap() - 0.99).abs() < 0.01);
        assert_eq!(validation, Some("verified".to_string()));
    }

    // ── update_command_run_with_events ──

    #[tokio::test]
    async fn update_command_run_with_events_updates_and_inserts() {
        let (state, task_id, item_id) = setup_task();
        let mut run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        run.exit_code = 1;
        let events = vec![
            DbEventRecord {
                task_id: task_id.clone(),
                task_item_id: Some(item_id.clone()),
                event_type: "phase_start".to_string(),
                payload_json: r#"{"step":"qa","cycle":1}"#.to_string(),
            },
            DbEventRecord {
                task_id: task_id.clone(),
                task_item_id: Some(item_id.clone()),
                event_type: "phase_end".to_string(),
                payload_json: r#"{"step":"qa","cycle":1}"#.to_string(),
            },
        ];

        state
            .db_writer
            .update_command_run_with_events(&run, &events)
            .await
            .expect("update_command_run_with_events");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let exit_code: Option<i64> = conn
            .query_row(
                "SELECT exit_code FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("query updated run");
        assert_eq!(exit_code, Some(1));

        let event_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE task_id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("count events");
        assert_eq!(event_count, 2);
    }

    #[tokio::test]
    async fn update_command_run_with_events_empty_events() {
        let (state, _task_id, item_id) = setup_task();
        let mut run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        run.exit_code = 0;
        state
            .db_writer
            .update_command_run_with_events(&run, &[])
            .await
            .expect("update with empty events");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let exit_code: Option<i64> = conn
            .query_row(
                "SELECT exit_code FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("query run");
        assert_eq!(exit_code, Some(0));
    }

    // ── update_command_run_pid ──

    #[tokio::test]
    async fn update_command_run_pid_sets_pid() {
        let (state, _task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        state
            .db_writer
            .update_command_run_pid(&run_id, 12345)
            .await
            .expect("update pid");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let pid: Option<i64> = conn
            .query_row(
                "SELECT pid FROM command_runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("query pid");
        assert_eq!(pid, Some(12345));
    }

    // ── find_active_child_pids ──

    #[tokio::test]
    async fn find_active_child_pids_empty() {
        let (state, task_id, _item_id) = setup_task();

        let pids = state
            .db_writer
            .find_active_child_pids(&task_id)
            .await
            .expect("find pids");
        assert!(pids.is_empty());
    }

    #[tokio::test]
    async fn find_active_child_pids_returns_active_pids() {
        let (state, task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        // Set exit_code = -1 (active) and pid
        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE command_runs SET exit_code = -1, pid = 9999 WHERE id = ?1",
            params![run_id],
        )
        .expect("set active pid");
        drop(conn);

        let pids = state
            .db_writer
            .find_active_child_pids(&task_id)
            .await
            .expect("find pids");
        assert_eq!(pids, vec![9999]);
    }

    #[tokio::test]
    async fn find_active_child_pids_ignores_completed_runs() {
        let (state, task_id, item_id) = setup_task();
        let run = make_command_run(&item_id);
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert_command_run");

        // Set exit_code = 0 (completed) and pid — should NOT be returned
        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE command_runs SET exit_code = 0, pid = 8888 WHERE id = ?1",
            params![run_id],
        )
        .expect("set completed pid");
        drop(conn);

        let pids = state
            .db_writer
            .find_active_child_pids(&task_id)
            .await
            .expect("find pids");
        assert!(pids.is_empty());
    }

    // ── update_task_pipeline_vars ──

    #[tokio::test]
    async fn update_task_pipeline_vars_stores_json() {
        let (state, task_id, _item_id) = setup_task();

        state
            .db_writer
            .update_task_pipeline_vars(&task_id, r#"{"key":"value"}"#)
            .await
            .expect("update pipeline vars");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let vars: Option<String> = conn
            .query_row(
                "SELECT pipeline_vars_json FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .expect("query pipeline vars");
        assert_eq!(vars, Some(r#"{"key":"value"}"#.to_string()));
    }

    // ── set_task_status: paused and interrupted branches ──

    #[tokio::test]
    async fn set_task_status_paused_clears_completed_at() {
        let (state, task_id, _item_id) = setup_task();

        // First set to completed
        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        // Now pause
        state
            .db_writer
            .set_task_status(&task_id, "paused", false)
            .await
            .expect("set paused");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, completed_at): (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");
        assert_eq!(status, "paused");
        assert!(completed_at.is_none());
    }

    #[tokio::test]
    async fn set_task_status_interrupted_clears_completed_at() {
        let (state, task_id, _item_id) = setup_task();

        state
            .db_writer
            .set_task_status(&task_id, "completed", true)
            .await
            .expect("set completed");

        state
            .db_writer
            .set_task_status(&task_id, "interrupted", false)
            .await
            .expect("set interrupted");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (status, completed_at): (String, Option<String>) = conn
            .query_row(
                "SELECT status, completed_at FROM tasks WHERE id = ?1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query task");
        assert_eq!(status, "interrupted");
        assert!(completed_at.is_none());
    }

    // ── mark_task_item_running idempotency ──

    #[tokio::test]
    async fn mark_task_item_running_preserves_started_at_on_second_call() {
        let (state, _task_id, item_id) = setup_task();

        state
            .db_writer
            .mark_task_item_running(&item_id)
            .await
            .expect("mark running first time");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let first_started_at: Option<String> = conn
            .query_row(
                "SELECT started_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .expect("get started_at");
        drop(conn);

        // Small delay to ensure timestamps differ
        std::thread::sleep(std::time::Duration::from_millis(10));

        state
            .db_writer
            .mark_task_item_running(&item_id)
            .await
            .expect("mark running second time");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let second_started_at: Option<String> = conn
            .query_row(
                "SELECT started_at FROM task_items WHERE id = ?1",
                params![item_id],
                |row| row.get(0),
            )
            .expect("get started_at after second mark");

        // COALESCE should preserve the original started_at
        assert_eq!(first_started_at, second_started_at);
    }

    // ── FR-038: find_inflight_command_runs_for_task ──

    #[tokio::test]
    async fn find_inflight_command_runs_empty() {
        let (state, task_id, _item_id) = setup_task();
        let runs = state
            .db_writer
            .find_inflight_command_runs_for_task(&task_id)
            .await
            .expect("find inflight");
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn find_inflight_command_runs_returns_active() {
        let (state, task_id, item_id) = setup_task();
        let mut run = make_command_run(&item_id);
        run.exit_code = -1;
        run.ended_at = String::new();
        let run_id = run.id.clone();

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE command_runs SET pid = 12345 WHERE id = ?1",
            params![run_id],
        )
        .expect("set pid");
        drop(conn);

        let inflight = state
            .db_writer
            .find_inflight_command_runs_for_task(&task_id)
            .await
            .expect("find inflight");
        assert_eq!(inflight.len(), 1);
        assert_eq!(inflight[0].1, item_id);
        assert_eq!(inflight[0].3, Some(12345));
    }

    #[tokio::test]
    async fn find_inflight_ignores_completed_runs() {
        let (state, task_id, item_id) = setup_task();
        let run = make_command_run(&item_id); // exit_code=0, ended_at set

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let inflight = state
            .db_writer
            .find_inflight_command_runs_for_task(&task_id)
            .await
            .expect("find inflight");
        assert!(inflight.is_empty());
    }

    // ── FR-038: find_completed_runs_for_pending_items ──

    #[tokio::test]
    async fn find_completed_runs_for_pending_items_empty_when_no_pending() {
        let (state, task_id, _item_id) = setup_task();
        let runs = state
            .db_writer
            .find_completed_runs_for_pending_items(&task_id)
            .await
            .expect("find completed");
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn find_completed_runs_for_pending_items_returns_matching() {
        let (state, task_id, item_id) = setup_task();
        // Item starts as 'pending' by default
        let mut run = make_command_run(&item_id);
        run.phase = "qa_testing".to_string();
        run.exit_code = 0;
        run.ended_at = "2026-01-01T00:01:00Z".to_string();
        run.confidence = Some(0.9);

        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let completed = state
            .db_writer
            .find_completed_runs_for_pending_items(&task_id)
            .await
            .expect("find completed");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].task_item_id, item_id);
        assert_eq!(completed[0].phase, "qa_testing");
        assert_eq!(completed[0].exit_code, 0);
    }

    #[tokio::test]
    async fn find_completed_runs_excludes_non_pending_items() {
        let (state, task_id, item_id) = setup_task();
        // Set item to qa_passed (non-pending)
        state
            .db_writer
            .set_task_item_terminal_status(&item_id, "qa_passed")
            .await
            .expect("set terminal");

        let run = make_command_run(&item_id);
        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let completed = state
            .db_writer
            .find_completed_runs_for_pending_items(&task_id)
            .await
            .expect("find completed");
        assert!(completed.is_empty());
    }

    // ── FR-038: count_stale_pending_items ──

    #[tokio::test]
    async fn count_stale_pending_items_zero_with_no_runs() {
        let (state, task_id, _item_id) = setup_task();
        let count = state
            .db_writer
            .count_stale_pending_items(&task_id)
            .await
            .expect("count stale");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn count_stale_pending_items_counts_stale() {
        let (state, task_id, item_id) = setup_task();
        // Item is pending; insert a completed run
        let run = make_command_run(&item_id);
        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let count = state
            .db_writer
            .count_stale_pending_items(&task_id)
            .await
            .expect("count stale");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn count_stale_pending_items_ignores_inflight() {
        let (state, task_id, item_id) = setup_task();
        // Item is pending; insert an in-flight run (exit_code=-1, no ended_at)
        let mut run = make_command_run(&item_id);
        run.exit_code = -1;
        run.ended_at = String::new();
        state
            .db_writer
            .insert_command_run(&run)
            .await
            .expect("insert");

        let count = state
            .db_writer
            .count_stale_pending_items(&task_id)
            .await
            .expect("count stale");
        assert_eq!(count, 0);
    }
}
