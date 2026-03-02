use crate::config::TaskExecutionPlan;
use crate::config_load::read_active_config;
use crate::session_store;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};
use std::process::Command;

use super::parse::{resolve_exec_target, shell_quote};
use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_exec(
        &self,
        _stdin: bool,
        tty: bool,
        target: &str,
        command: &[String],
    ) -> Result<i32> {
        let resolved = resolve_exec_target(&self.state, target)?;
        if let Some(sess) = &resolved.session {
            return self.exec_against_session(sess, tty, command);
        }
        let task_id = resolved.task_id;
        let step_id = resolved.step_id;
        if !resolved.step_tty && tty {
            anyhow::bail!(
                "step '{}' has tty disabled; enable it via `orchestrator task edit ... --tty`",
                step_id
            );
        }
        let (workspace_root, qa_file_path) = {
            let repo = SqliteTaskRepository::new(self.state.database.clone());
            let runtime_row = repo.load_task_runtime_row(&task_id)?;
            let items = repo.list_task_items_for_cycle(&task_id)?;
            let first = items
                .first()
                .with_context(|| format!("task '{}' has no task items", task_id))?;
            (
                std::path::PathBuf::from(runtime_row.workspace_root_raw.clone()),
                first.qa_file_path.clone(),
            )
        };

        let (agent_id, template) = {
            let repo = SqliteTaskRepository::new(self.state.database.clone());
            let runtime_row = repo.load_task_runtime_row(&task_id)?;
            let plan = serde_json::from_str::<TaskExecutionPlan>(&runtime_row.execution_plan_json)
                .with_context(|| format!("failed to parse execution plan for task {}", task_id))?;
            let step = plan
                .steps
                .iter()
                .find(|s| s.id == step_id)
                .with_context(|| format!("step '{}' not found in task '{}'", step_id, task_id))?;
            let active = read_active_config(&self.state)?;
            if let Some(cap) = step.required_capability.as_deref() {
                let found =
                    active.config.agents.iter().find_map(|(id, cfg)| {
                        cfg.get_template(cap).map(|t| (id.clone(), t.clone()))
                    });
                found
                    .with_context(|| format!("no agent template found for capability '{}'", cap))?
            } else {
                let cap = step.id.clone();
                let found =
                    active.config.agents.iter().find_map(|(id, cfg)| {
                        cfg.get_template(&cap).map(|t| (id.clone(), t.clone()))
                    });
                found.with_context(|| format!("no agent template found for '{}'", cap))?
            }
        };

        let runtime_row = SqliteTaskRepository::new(self.state.database.clone())
            .load_task_runtime_row(&task_id)?;
        let rendered = template
            .replace("{rel_path}", &qa_file_path)
            .replace("{ticket_paths}", "")
            .replace("{phase}", &step_id)
            .replace("{cycle}", &runtime_row.current_cycle.to_string())
            .replace("{task_id}", &task_id);

        let to_run = if command.is_empty() {
            rendered
        } else {
            command.join(" ")
        };
        if tty {
            let status = Command::new("/bin/bash")
                .arg("-lc")
                .arg(&to_run)
                .current_dir(workspace_root)
                .status()
                .with_context(|| {
                    format!("failed to execute interactive command for {}", agent_id)
                })?;
            return Ok(status.code().unwrap_or(1));
        }

        let output = Command::new("/bin/bash")
            .arg("-lc")
            .arg(&to_run)
            .current_dir(workspace_root)
            .output()
            .with_context(|| format!("failed to execute command for {}", agent_id))?;
        if !output.stdout.is_empty() {
            println!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output.status.code().unwrap_or(1))
    }

    pub(super) fn exec_against_session(
        &self,
        sess: &session_store::SessionRow,
        tty: bool,
        command: &[String],
    ) -> Result<i32> {
        if sess.state != "active" && sess.state != "detached" {
            anyhow::bail!(
                "session '{}' is not attachable (state={})",
                sess.id,
                sess.state
            );
        }
        let client_id = format!("cli-{}", std::process::id());
        if tty {
            if !command.is_empty() {
                session_store::acquire_writer(&self.state.db_path, &sess.id, &client_id)?;
                let cmdline = command.join(" ");
                let status = Command::new("/bin/bash")
                    .arg("-lc")
                    .arg(&cmdline)
                    .current_dir(&sess.cwd)
                    .status()
                    .context("exec interactive command in session context")?;
                session_store::release_attachment(
                    &self.state.db_path,
                    &sess.id,
                    &client_id,
                    "detach",
                )?;
                return Ok(status.code().unwrap_or(1));
            }

            let writable =
                session_store::acquire_writer(&self.state.db_path, &sess.id, &client_id)?;
            if !writable {
                session_store::attach_reader(&self.state.db_path, &sess.id, &client_id)?;
            }
            let status_res = if writable {
                Command::new("/bin/bash")
                    .arg("-lc")
                    .arg(format!(
                        "tail -n +1 -f {} & TPID=$!; cat > {}; kill $TPID",
                        shell_quote(&sess.stdout_path),
                        shell_quote(&sess.input_fifo_path),
                    ))
                    .status()
                    .context("attach writable session")
            } else {
                Command::new("tail")
                    .arg("-n")
                    .arg("+1")
                    .arg("-f")
                    .arg(&sess.stdout_path)
                    .status()
                    .context("attach read-only session")
            };
            session_store::release_attachment(&self.state.db_path, &sess.id, &client_id, "detach")?;
            let status = status_res?;
            return Ok(status.code().unwrap_or(1));
        }

        if command.is_empty() {
            anyhow::bail!(
                "active session exists for step '{}'; provide command args or use -it",
                sess.step_id
            );
        }
        let cmdline = command.join(" ");
        let output = Command::new("/bin/bash")
            .arg("-lc")
            .arg(&cmdline)
            .current_dir(&sess.cwd)
            .output()
            .context("exec command in session context")?;
        if !output.stdout.is_empty() {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .open(&sess.stdout_path)
            {
                let _ = f.write_all(&output.stdout);
            }
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output.status.code().unwrap_or(1))
    }
}
