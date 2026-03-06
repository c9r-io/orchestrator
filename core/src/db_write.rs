use crate::async_database::{flatten_err, AsyncDatabase};
use crate::config_load::now_ts;
use crate::task_repository::NewCommandRun;
use anyhow::Result;
use rusqlite::params;
use std::sync::Arc;

pub struct DbWriteCoordinator {
    async_db: Arc<AsyncDatabase>,
}

/// Owned event record suitable for sending into `call()` closures (`'static + Send`).
pub struct DbEventRecord {
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub event_type: String,
    pub payload_json: String,
}

/// Extract promoted fields (step, step_scope, cycle) from event payload JSON.
fn extract_event_promoted_fields(
    payload_json: &str,
) -> (Option<String>, Option<String>, Option<i64>) {
    let v: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(v) => v,
        Err(_) => return (None, None, None),
    };
    let step = v["step"]
        .as_str()
        .or_else(|| v["phase"].as_str())
        .map(String::from);
    let step_scope = v["step_scope"].as_str().map(String::from);
    let cycle = v["cycle"].as_i64();
    (step, step_scope, cycle)
}

impl DbWriteCoordinator {
    pub fn new(async_db: Arc<AsyncDatabase>) -> Self {
        Self { async_db }
    }

    pub async fn insert_event(
        &self,
        task_id: &str,
        task_item_id: Option<&str>,
        event_type: &str,
        payload_json: &str,
    ) -> Result<()> {
        let task_id = task_id.to_owned();
        let task_item_id = task_item_id.map(str::to_owned);
        let event_type = event_type.to_owned();
        let payload_json = payload_json.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                let (step, step_scope, cycle) = extract_event_promoted_fields(&payload_json);
                conn.execute(
                    "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at, step, step_scope, cycle) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![task_id, task_item_id, event_type, payload_json, now_ts(), step, step_scope, cycle],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

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
                let now = now_ts();
                if set_completed {
                    conn.execute(
                        "UPDATE tasks SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = ?4, updated_at = ?5 WHERE id = ?1",
                        params![task_id, status, now.clone(), now.clone(), now],
                    )?;
                } else if status == "running" {
                    conn.execute(
                        "UPDATE tasks SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = NULL, updated_at = ?4 WHERE id = ?1",
                        params![task_id, status, now.clone(), now],
                    )?;
                } else if matches!(status.as_str(), "pending" | "paused" | "interrupted") {
                    conn.execute(
                        "UPDATE tasks SET status = ?2, completed_at = NULL, updated_at = ?3 WHERE id = ?1",
                        params![task_id, status, now],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
                        params![task_id, status, now],
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let run = run.clone();
        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted, session_id, machine_output_source, output_json_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                    params![
                        run.id,
                        run.task_item_id,
                        run.phase,
                        run.command,
                        run.cwd,
                        run.workspace_id,
                        run.agent_id,
                        run.exit_code,
                        run.stdout_path,
                        run.stderr_path,
                        run.output_json,
                        run.artifacts_json,
                        run.confidence,
                        run.quality_score,
                        run.validation_status,
                        run.started_at,
                        run.ended_at,
                        run.interrupted,
                        run.session_id,
                        run.machine_output_source,
                        run.output_json_path
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn update_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let run = run.clone();
        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "UPDATE command_runs SET exit_code = ?2, ended_at = ?3, interrupted = ?4, output_json = ?5, artifacts_json = ?6, confidence = ?7, quality_score = ?8, validation_status = ?9, session_id = ?10, machine_output_source = ?11, output_json_path = ?12 WHERE id = ?1",
                    params![
                        run.id,
                        run.exit_code,
                        run.ended_at,
                        run.interrupted,
                        run.output_json,
                        run.artifacts_json,
                        run.confidence,
                        run.quality_score,
                        run.validation_status,
                        run.session_id,
                        run.machine_output_source,
                        run.output_json_path
                    ],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn update_command_run_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        let run = run.clone();
        let events: Vec<_> = events
            .iter()
            .map(|e| DbEventRecord {
                task_id: e.task_id.clone(),
                task_item_id: e.task_item_id.clone(),
                event_type: e.event_type.clone(),
                payload_json: e.payload_json.clone(),
            })
            .collect();
        self.async_db
            .writer()
            .call(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "UPDATE command_runs SET exit_code = ?2, ended_at = ?3, interrupted = ?4, output_json = ?5, artifacts_json = ?6, confidence = ?7, quality_score = ?8, validation_status = ?9, session_id = ?10, machine_output_source = ?11, output_json_path = ?12 WHERE id = ?1",
                    params![
                        run.id,
                        run.exit_code,
                        run.ended_at,
                        run.interrupted,
                        run.output_json,
                        run.artifacts_json,
                        run.confidence,
                        run.quality_score,
                        run.validation_status,
                        run.session_id,
                        run.machine_output_source,
                        run.output_json_path
                    ],
                )?;
                for event in &events {
                    let (step, step_scope, cycle) = extract_event_promoted_fields(&event.payload_json);
                    tx.execute(
                        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at, step, step_scope, cycle) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            event.task_id,
                            event.task_item_id,
                            event.event_type,
                            event.payload_json,
                            now_ts(),
                            step,
                            step_scope,
                            cycle
                        ],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

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

    pub async fn persist_phase_result_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord],
    ) -> Result<()> {
        let run = run.clone();
        let events: Vec<_> = events
            .iter()
            .map(|e| DbEventRecord {
                task_id: e.task_id.clone(),
                task_item_id: e.task_item_id.clone(),
                event_type: e.event_type.clone(),
                payload_json: e.payload_json.clone(),
            })
            .collect();
        self.async_db
            .writer()
            .call(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT INTO command_runs (id, task_item_id, phase, command, cwd, workspace_id, agent_id, exit_code, stdout_path, stderr_path, output_json, artifacts_json, confidence, quality_score, validation_status, started_at, ended_at, interrupted, session_id, machine_output_source, output_json_path) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                    params![
                        run.id,
                        run.task_item_id,
                        run.phase,
                        run.command,
                        run.cwd,
                        run.workspace_id,
                        run.agent_id,
                        run.exit_code,
                        run.stdout_path,
                        run.stderr_path,
                        run.output_json,
                        run.artifacts_json,
                        run.confidence,
                        run.quality_score,
                        run.validation_status,
                        run.started_at,
                        run.ended_at,
                        run.interrupted,
                        run.session_id,
                        run.machine_output_source,
                        run.output_json_path
                    ],
                )?;
                for event in &events {
                    let (step, step_scope, cycle) = extract_event_promoted_fields(&event.payload_json);
                    tx.execute(
                        "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at, step, step_scope, cycle) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            event.task_id,
                            event.task_item_id,
                            event.event_type,
                            event.payload_json,
                            now_ts(),
                            step,
                            step_scope,
                            cycle
                        ],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn update_command_run_pid(&self, run_id: &str, pid: i64) -> Result<()> {
        let run_id = run_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                conn.execute(
                    "UPDATE command_runs SET pid = ?2 WHERE id = ?1",
                    params![run_id, pid],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn find_active_child_pids(&self, task_id: &str) -> Result<Vec<i64>> {
        let task_id = task_id.to_owned();
        self.async_db
            .reader()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT cr.pid FROM command_runs cr
                     JOIN task_items ti ON cr.task_item_id = ti.id
                     WHERE ti.task_id = ?1 AND cr.exit_code = -1 AND cr.pid IS NOT NULL",
                )?;
                let pids = stmt
                    .query_map(params![task_id], |row| row.get::<_, i64>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(pids)
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
                conn.execute(
                    "UPDATE tasks SET current_cycle = ?2, init_done = ?3, updated_at = ?4 WHERE id = ?1",
                    params![
                        task_id,
                        current_cycle as i64,
                        if init_done { 1 } else { 0 },
                        now_ts()
                    ],
                )?;
                Ok(())
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
                conn.execute(
                    "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
                    params![task_item_id, status, now_ts()],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn mark_task_item_running(&self, task_item_id: &str) -> Result<()> {
        let task_item_id = task_item_id.to_owned();
        self.async_db
            .writer()
            .call(move |conn| {
                let now = now_ts();
                conn.execute(
                    "UPDATE task_items SET status = 'running', started_at = COALESCE(started_at, ?2), completed_at = NULL, updated_at = ?3 WHERE id = ?1",
                    params![task_item_id, now.clone(), now],
                )?;
                Ok(())
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
                let now = now_ts();
                conn.execute(
                    "UPDATE task_items SET status = ?2, started_at = COALESCE(started_at, ?3), completed_at = ?4, updated_at = ?5 WHERE id = ?1",
                    params![task_item_id, status, now.clone(), now.clone(), now],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

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
                conn.execute(
                    "UPDATE tasks SET pipeline_vars_json = ?2, updated_at = ?3 WHERE id = ?1",
                    params![task_id, pipeline_vars_json, now_ts()],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
    }

    pub async fn update_task_pipeline_vars_sync(
        &self,
        task_id: &str,
        pipeline_vars_json: &str,
    ) -> Result<()> {
        self.update_task_pipeline_vars(task_id, pipeline_vars_json)
            .await
    }

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
                conn.execute(
                    "UPDATE task_items SET ticket_files_json = ?2, ticket_content_json = ?3, updated_at = ?4 WHERE id = ?1",
                    params![task_item_id, ticket_files_json, ticket_content_json, now_ts()],
                )?;
                Ok(())
            })
            .await
            .map_err(flatten_err)
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
            .app_root
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
}
