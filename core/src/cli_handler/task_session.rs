use crate::cli::{OutputFormat, TaskSessionCommands};
use crate::config::{TaskExecutionPlan, TaskExecutionStep};
use crate::config_load::now_ts;
use crate::db::open_conn;
use crate::scheduler::resolve_task_id;
use crate::session_store;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};
use std::process::Command;

use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_task_session(&self, cmd: &TaskSessionCommands) -> Result<i32> {
        match cmd {
            TaskSessionCommands::List { task_id, output } => {
                let task_id = resolve_task_id(&self.state, task_id)?;
                let conn = self.state.database.connection()?;
                let rows = session_store::list_task_sessions(&conn, &task_id)?;
                match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
                    OutputFormat::Yaml => println!("{}", serde_yml::to_string(&rows)?),
                    OutputFormat::Table => {
                        if rows.is_empty() {
                            println!("No sessions for task {}", task_id);
                        } else {
                            println!(
                                "{:<38} {:<18} {:<8} {:<10} {:<12} {:<8}",
                                "SESSION_ID", "STEP", "PHASE", "STATE", "WRITER", "PID"
                            );
                            for s in rows {
                                println!(
                                    "{:<38} {:<18} {:<8} {:<10} {:<12} {:<8}",
                                    s.id,
                                    s.step_id,
                                    s.phase,
                                    s.state,
                                    s.writer_client_id.unwrap_or_else(|| "-".to_string()),
                                    s.pid
                                );
                            }
                        }
                    }
                }
                Ok(0)
            }
            TaskSessionCommands::Info { session_id, output } => {
                let conn = self.state.database.connection()?;
                let row = session_store::load_session(&conn, session_id)?
                    .with_context(|| format!("session not found: {}", session_id))?;
                match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&row)?),
                    OutputFormat::Yaml => println!("{}", serde_yml::to_string(&row)?),
                    OutputFormat::Table => {
                        println!("id: {}", row.id);
                        println!("task_id: {}", row.task_id);
                        println!("step_id: {}", row.step_id);
                        println!("phase: {}", row.phase);
                        println!("state: {}", row.state);
                        println!("agent_id: {}", row.agent_id);
                        println!("pid: {}", row.pid);
                        println!("fifo: {}", row.input_fifo_path);
                        println!("stdout: {}", row.stdout_path);
                        println!("stderr: {}", row.stderr_path);
                        println!(
                            "writer: {}",
                            row.writer_client_id.unwrap_or_else(|| "-".to_string())
                        );
                    }
                }
                Ok(0)
            }
            TaskSessionCommands::Close { session_id, force } => {
                let conn = self.state.database.connection()?;
                let row = session_store::load_session(&conn, session_id)?
                    .with_context(|| format!("session not found: {}", session_id))?;
                if row.pid > 0 {
                    let sig = if *force { "-9" } else { "-15" };
                    let _ = Command::new("kill")
                        .arg(sig)
                        .arg(row.pid.to_string())
                        .status();
                }
                session_store::update_session_state(
                    &conn,
                    session_id,
                    "closed",
                    None,
                    true,
                )?;
                println!("Session closed: {}", session_id);
                Ok(0)
            }
        }
    }

    pub(super) fn handle_task_edit(
        &self,
        task_id: &str,
        insert_before: &str,
        step: &str,
        capability: Option<&str>,
        tty: bool,
        repeatable: bool,
    ) -> Result<i32> {
        let resolved_id = resolve_task_id(&self.state, task_id)?;
        crate::config::validate_step_type(step)
            .map_err(|e| anyhow::anyhow!("invalid --step '{}': {}", step, e))?;

        let repo = SqliteTaskRepository::new(self.state.database.clone());
        let runtime_row = repo.load_task_runtime_row(&resolved_id)?;
        let mut plan = serde_json::from_str::<TaskExecutionPlan>(&runtime_row.execution_plan_json)
            .with_context(|| format!("failed to parse execution plan for task {}", resolved_id))?;

        let insert_idx = plan
            .steps
            .iter()
            .position(|s| s.id == insert_before)
            .with_context(|| {
                format!(
                    "step '{}' not found in task '{}' execution plan",
                    insert_before, resolved_id
                )
            })?;

        let new_id = if !plan.steps.iter().any(|s| s.id == step) {
            step.to_string()
        } else {
            let mut candidate = format!("{}-{}", step, plan.steps.len() + 1);
            if plan.steps.iter().any(|s| s.id == candidate) {
                candidate = format!("{}-{}", step, uuid::Uuid::new_v4());
            }
            candidate
        };

        let builtin = match step {
            "init_once" | "ticket_scan" | "loop_guard" => Some(step.to_string()),
            _ => None,
        };
        let required_capability = capability.map(|v| v.to_string()).or_else(|| match step {
            "plan" | "qa" | "fix" | "retest" => Some(step.to_string()),
            _ => None,
        });
        let is_guard = step == "loop_guard";
        let inserted_step = TaskExecutionStep {
            id: new_id.clone(),
            required_capability,
            builtin,
            enabled: true,
            repeatable,
            is_guard,
            cost_preference: None,
            prehook: None,
            tty,
            template: None,
            outputs: Vec::new(),
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: Default::default(),
            max_parallel: None,
        };
        plan.steps.insert(insert_idx, inserted_step);

        let conn = open_conn(&self.state.db_path)?;
        let updated_plan = serde_json::to_string(&plan)?;
        conn.execute(
            "UPDATE tasks SET execution_plan_json = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![resolved_id, updated_plan, now_ts()],
        )?;

        println!(
            "Task plan updated: {} inserted step '{}' before '{}'",
            task_id, new_id, insert_before
        );
        Ok(0)
    }
}
