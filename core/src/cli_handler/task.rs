use crate::cli::TaskCommands;
use crate::dto::CreateTaskPayload;
use crate::scheduler::{
    delete_task_impl, find_latest_resumable_task_id, follow_task_logs, get_task_details_impl,
    list_tasks_impl, load_task_summary, prepare_task_for_start, resolve_task_id, run_task_loop,
    stop_task_runtime, stop_task_runtime_for_delete, stream_task_logs_impl, watch_task,
    RunningTask,
};
use crate::scheduler_service::enqueue_task;
use crate::task_ops::{create_task_impl, reset_task_item_for_retry};
use anyhow::{Context, Result};

use super::{cli_runtime, CliHandler};

impl CliHandler {
    pub(super) fn handle_task(&self, cmd: &TaskCommands) -> Result<i32> {
        match cmd {
            TaskCommands::List {
                status,
                output,
                verbose,
            } => {
                let tasks = cli_runtime().block_on(list_tasks_impl(&self.state))?;
                let filtered: Vec<_> = match status {
                    Some(s) => tasks.into_iter().filter(|t| t.status == *s).collect(),
                    None => tasks,
                };
                self.print_tasks(&filtered, *output, *verbose)
            }
            TaskCommands::Create {
                name,
                goal,
                project,
                workspace,
                workflow,
                target_file,
                no_start,
                detach,
            } => {
                let payload = CreateTaskPayload {
                    name: name.clone(),
                    goal: goal.clone(),
                    project_id: project.clone(),
                    workspace_id: workspace.clone(),
                    workflow_id: workflow.clone(),
                    target_files: if target_file.is_empty() {
                        None
                    } else {
                        Some(target_file.clone())
                    },
                };
                let created = create_task_impl(&self.state, payload)?;
                println!("Task created: {}", created.id);
                if !no_start {
                    if *detach {
                        cli_runtime().block_on(enqueue_task(&self.state, &created.id))?;
                        println!("Task enqueued: {}", created.id);
                    } else {
                        cli_runtime().block_on(prepare_task_for_start(&self.state, &created.id))?;
                        let runtime = RunningTask::new();
                        cli_runtime().block_on(run_task_loop(
                            self.state.clone(),
                            &created.id,
                            runtime,
                        ))?;
                        let summary =
                            cli_runtime().block_on(load_task_summary(&self.state, &created.id))?;
                        println!("Task finished: {} status={}", summary.id, summary.status);
                    }
                }
                Ok(0)
            }
            TaskCommands::Info { task_id, output } => {
                let detail = cli_runtime().block_on(get_task_details_impl(&self.state, task_id))?;
                self.print_task_detail(&detail, *output)
            }
            TaskCommands::Start {
                task_id,
                latest,
                detach,
            } => {
                let id = if let Some(id) = task_id {
                    cli_runtime().block_on(resolve_task_id(&self.state, id))?
                } else if *latest {
                    cli_runtime()
                        .block_on(find_latest_resumable_task_id(&self.state, true))?
                        .context("no resumable task found")?
                } else {
                    anyhow::bail!("task_id or --latest required")
                };
                if *detach {
                    cli_runtime().block_on(enqueue_task(&self.state, &id))?;
                    println!("Task enqueued: {}", id);
                } else {
                    cli_runtime().block_on(prepare_task_for_start(&self.state, &id))?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(self.state.clone(), &id, runtime))?;
                    let summary = cli_runtime().block_on(load_task_summary(&self.state, &id))?;
                    println!("Task finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Pause { task_id } => {
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                cli_runtime().block_on(stop_task_runtime(
                    self.state.clone(),
                    &resolved_id,
                    "paused",
                ))?;
                println!("Task paused: {}", resolved_id);
                Ok(0)
            }
            TaskCommands::Resume { task_id, detach } => {
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                if *detach {
                    cli_runtime().block_on(enqueue_task(&self.state, &resolved_id))?;
                    println!("Task enqueued: {}", resolved_id);
                } else {
                    cli_runtime().block_on(prepare_task_for_start(&self.state, &resolved_id))?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(
                        self.state.clone(),
                        &resolved_id,
                        runtime,
                    ))?;
                    let summary =
                        cli_runtime().block_on(load_task_summary(&self.state, &resolved_id))?;
                    println!("Task finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Logs {
                task_id,
                follow,
                tail,
                timestamps,
            } => {
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                let logs = cli_runtime().block_on(stream_task_logs_impl(
                    &self.state,
                    &resolved_id,
                    *tail,
                    *timestamps,
                ))?;
                for chunk in logs {
                    println!("{}", chunk.content);
                }
                if *follow {
                    cli_runtime().block_on(follow_task_logs(&self.state, &resolved_id))?;
                }
                Ok(0)
            }
            TaskCommands::Watch { task_id, interval } => {
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                cli_runtime().block_on(watch_task(&self.state, &resolved_id, *interval))?;
                Ok(0)
            }
            TaskCommands::Delete { task_id, force } => {
                if !force && !self.is_unsafe() {
                    println!("Use --force to confirm deletion of task {}", task_id);
                    return Ok(0);
                }
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                cli_runtime().block_on(stop_task_runtime_for_delete(
                    self.state.clone(),
                    &resolved_id,
                ))?;
                cli_runtime().block_on(delete_task_impl(&self.state, &resolved_id))?;
                println!("Task deleted: {}", resolved_id);
                Ok(0)
            }
            TaskCommands::Retry {
                task_item_id,
                detach,
                force,
            } => {
                if !force && !self.is_unsafe() {
                    eprintln!("⚠ This will reset task item execution state for retry.");
                    eprintln!(
                        "  Use --force to confirm: orchestrator task retry <ITEM_ID> --force"
                    );
                    return Ok(1);
                }
                let task_id = reset_task_item_for_retry(&self.state, task_item_id)?;
                if *detach {
                    cli_runtime().block_on(enqueue_task(&self.state, &task_id))?;
                    println!("Task enqueued: {}", task_id);
                } else {
                    cli_runtime().block_on(prepare_task_for_start(&self.state, &task_id))?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(self.state.clone(), &task_id, runtime))?;
                    let summary =
                        cli_runtime().block_on(load_task_summary(&self.state, &task_id))?;
                    println!("Retry finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Trace {
                task_id,
                json,
                verbose,
            } => {
                let resolved_id = cli_runtime().block_on(resolve_task_id(&self.state, task_id))?;
                let detail =
                    cli_runtime().block_on(get_task_details_impl(&self.state, &resolved_id))?;
                let trace = crate::scheduler::trace::build_trace_with_meta(
                    crate::scheduler::trace::TraceTaskMeta {
                        task_id: &detail.task.id,
                        status: &detail.task.status,
                        created_at: &detail.task.created_at,
                        started_at: detail.task.started_at.as_deref(),
                        completed_at: detail.task.completed_at.as_deref(),
                        updated_at: &detail.task.updated_at,
                    },
                    &detail.events,
                    &detail.runs,
                );
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&trace)
                            .context("failed to serialize trace")?
                    );
                } else {
                    crate::scheduler::trace::render_trace_terminal(&trace, *verbose);
                }
                Ok(0)
            }
            TaskCommands::Worker(cmd) => self.handle_task_worker(cmd),
            TaskCommands::Session(cmd) => self.handle_task_session(cmd),
            TaskCommands::Edit {
                task_id,
                insert_before,
                step,
                capability,
                tty,
                repeatable,
            } => self.handle_task_edit(
                task_id,
                insert_before,
                step,
                capability.as_deref(),
                *tty,
                *repeatable,
            ),
        }
    }
}
