#![allow(dead_code)]

use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::task_repository::NewCommandRun;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct DbWriteCoordinator {
    conn: Mutex<Connection>,
}

pub struct DbEventRecord<'a> {
    pub task_id: &'a str,
    pub task_item_id: Option<&'a str>,
    pub event_type: &'a str,
    pub payload_json: &'a str,
}

impl DbWriteCoordinator {
    pub fn new(db_path: &Path) -> Result<Self> {
        Ok(Self {
            conn: Mutex::new(open_conn(db_path)?),
        })
    }

    pub fn insert_event(
        &self,
        task_id: &str,
        task_item_id: Option<&str>,
        event_type: &str,
        payload_json: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
        conn.execute(
            "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![task_id, task_item_id, event_type, payload_json, now_ts()],
        )?;
        Ok(())
    }

    pub fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
        let now = now_ts();
        if set_completed {
            conn.execute(
                "UPDATE tasks SET status = ?2, completed_at = ?3, updated_at = ?4 WHERE id = ?1",
                params![task_id, status, now.clone(), now],
            )?;
        } else if matches!(status, "pending" | "running" | "paused" | "interrupted") {
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
    }

    pub fn insert_command_run(&self, run: &NewCommandRun) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
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
    }

    pub fn persist_phase_result(
        &self,
        run: &NewCommandRun,
        event: Option<DbEventRecord<'_>>,
    ) -> Result<()> {
        let events = match event {
            Some(single) => vec![single],
            None => Vec::new(),
        };
        self.persist_phase_result_with_events(run, &events)
    }

    pub fn persist_phase_result_with_events(
        &self,
        run: &NewCommandRun,
        events: &[DbEventRecord<'_>],
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
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

        for event in events {
            tx.execute(
                "INSERT INTO events (task_id, task_item_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event.task_id,
                    event.task_item_id,
                    event.event_type,
                    event.payload_json,
                    now_ts()
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
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
    }

    pub fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
        conn.execute(
            "UPDATE task_items SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_item_id, status, now_ts()],
        )?;
        Ok(())
    }

    pub fn update_task_item_tickets(
        &self,
        task_item_id: &str,
        ticket_files_json: &str,
        ticket_content_json: &str,
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("db write coordinator lock poisoned"))?;
        conn.execute(
            "UPDATE task_items SET ticket_files_json = ?2, ticket_content_json = ?3, updated_at = ?4 WHERE id = ?1",
            params![task_item_id, ticket_files_json, ticket_content_json, now_ts()],
        )?;
        Ok(())
    }
}
