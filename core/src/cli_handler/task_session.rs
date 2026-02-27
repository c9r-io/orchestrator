use crate::cli::{OutputFormat, TaskSessionCommands};
use crate::config::{TaskExecutionPlan, TaskExecutionStep, WorkflowStepType};
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
                let rows = session_store::list_task_sessions(&self.state.db_path, &task_id)?;
                match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
                    OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&rows)?),
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
                let row = session_store::load_session(&self.state.db_path, session_id)?
                    .with_context(|| format!("session not found: {}", session_id))?;
                match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&row)?),
                    OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&row)?),
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
                let row = session_store::load_session(&self.state.db_path, session_id)?
                    .with_context(|| format!("session not found: {}", session_id))?;
                if row.pid > 0 {
                    let sig = if *force { "-9" } else { "-15" };
                    let _ = Command::new("kill")
                        .arg(sig)
                        .arg(row.pid.to_string())
                        .status();
                }
                session_store::update_session_state(
                    &self.state.db_path,
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
        let parsed_step_type = step.parse::<WorkflowStepType>().map_err(|_| {
            anyhow::anyhow!(
                "invalid --step '{}': expected init_once|plan|qa|ticket_scan|fix|retest|loop_guard",
                step
            )
        })?;

        let repo = SqliteTaskRepository::new(self.state.db_path.clone());
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

        let mut new_id = format!("{}-{}", parsed_step_type.as_str(), plan.steps.len() + 1);
        if !plan.steps.iter().any(|s| s.id == new_id) {
            // keep generated id
        } else {
            new_id = format!("{}-{}", parsed_step_type.as_str(), uuid::Uuid::new_v4());
        }

        let builtin = match parsed_step_type {
            WorkflowStepType::InitOnce => Some("init_once".to_string()),
            WorkflowStepType::TicketScan => Some("ticket_scan".to_string()),
            WorkflowStepType::LoopGuard => Some("loop_guard".to_string()),
            _ => None,
        };
        let required_capability = capability.map(|v| v.to_string()).or_else(|| {
            if matches!(
                parsed_step_type,
                WorkflowStepType::Plan
                    | WorkflowStepType::Qa
                    | WorkflowStepType::Fix
                    | WorkflowStepType::Retest
            ) {
                Some(parsed_step_type.as_str().to_string())
            } else {
                None
            }
        });
        let inserted_step = TaskExecutionStep {
            id: new_id.clone(),
            step_type: Some(parsed_step_type.clone()),
            required_capability,
            builtin,
            enabled: true,
            repeatable,
            is_guard: parsed_step_type == WorkflowStepType::LoopGuard,
            cost_preference: None,
            prehook: None,
            tty,
            outputs: Vec::new(),
            pipe_to: None,
            command: None,
            chain_steps: vec![],
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
