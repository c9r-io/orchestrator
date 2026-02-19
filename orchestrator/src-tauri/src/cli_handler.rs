use crate::cli::{Cli, Commands, ConfigCommands, OutputFormat, TaskCommands, WorkspaceCommands};
use crate::InnerState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub struct CliHandler {
    state: Arc<InnerState>,
}

impl CliHandler {
    pub fn new(state: Arc<InnerState>) -> Self {
        Self { state }
    }

    pub fn execute(&self, cli: &Cli) -> Result<i32> {
        match &cli.command {
            Commands::Task(cmd) => self.handle_task(cmd),
            Commands::Workspace(cmd) => self.handle_workspace(cmd),
            Commands::Config(cmd) => self.handle_config(cmd),
            Commands::Daemon => {
                println!("Starting daemon mode (UI)... use --cli flag for CLI mode");
                Ok(0)
            }
        }
    }

    fn handle_task(&self, cmd: &TaskCommands) -> Result<i32> {
        match cmd {
            TaskCommands::List {
                status,
                output,
                verbose,
            } => {
                let tasks = crate::list_tasks_impl(&self.state)?;
                let filtered: Vec<_> = match status {
                    Some(s) => tasks.into_iter().filter(|t| t.status == *s).collect(),
                    None => tasks,
                };
                self.print_tasks(&filtered, *output, *verbose)
            }
            TaskCommands::Create {
                name,
                goal,
                workspace,
                workflow,
                target_file,
                no_start,
            } => {
                let payload = crate::CreateTaskPayload {
                    name: name.clone(),
                    goal: goal.clone(),
                    workspace_id: workspace.clone(),
                    workflow_id: workflow.clone(),
                    target_files: if target_file.is_empty() {
                        None
                    } else {
                        Some(target_file.clone())
                    },
                };
                let created = crate::create_task_impl(&self.state, payload)?;
                println!("Task created: {}", created.id);
                if !no_start {
                    crate::prepare_task_for_start(&self.state, &created.id)?;
                    let runtime = crate::RunningTask::new();
                    let rt = tokio::runtime::Runtime::new()?;
                    rt.block_on(crate::run_task_loop(
                        self.state.clone(),
                        None,
                        &created.id,
                        runtime,
                    ))?;
                    let summary = crate::load_task_summary(&self.state, &created.id)?;
                    println!("Task finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Info { task_id, output } => {
                let detail = crate::get_task_details_impl(&self.state, task_id)?;
                self.print_task_detail(&detail, *output)
            }
            TaskCommands::Start { task_id, latest } => {
                let id = if let Some(id) = task_id {
                    id.clone()
                } else if *latest {
                    crate::find_latest_resumable_task_id(&self.state, true)?
                        .context("no resumable task found")?
                } else {
                    anyhow::bail!("task_id or --latest required")
                };
                crate::prepare_task_for_start(&self.state, &id)?;
                let runtime = crate::RunningTask::new();
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::run_task_loop(self.state.clone(), None, &id, runtime))?;
                let summary = crate::load_task_summary(&self.state, &id)?;
                println!("Task finished: {} status={}", summary.id, summary.status);
                Ok(0)
            }
            TaskCommands::Pause { task_id } => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::stop_task_runtime(
                    self.state.clone(),
                    task_id,
                    "paused",
                ))?;
                println!("Task paused: {}", task_id);
                Ok(0)
            }
            TaskCommands::Resume { task_id } => {
                crate::prepare_task_for_start(&self.state, task_id)?;
                let runtime = crate::RunningTask::new();
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::run_task_loop(
                    self.state.clone(),
                    None,
                    task_id,
                    runtime,
                ))?;
                let summary = crate::load_task_summary(&self.state, task_id)?;
                println!("Task finished: {} status={}", summary.id, summary.status);
                Ok(0)
            }
            TaskCommands::Logs {
                task_id,
                follow,
                tail: _,
                timestamps,
            } => {
                let logs = crate::stream_task_logs_impl(&self.state, task_id, 300)?;
                for chunk in logs {
                    println!("{}", chunk.content);
                }
                Ok(0)
            }
            TaskCommands::Delete { task_id, force } => {
                if !force {
                    println!("Use --force to confirm deletion of task {}", task_id);
                    return Ok(0);
                }
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::stop_task_runtime_for_delete(
                    self.state.clone(),
                    task_id,
                ))?;
                crate::delete_task_impl(&self.state, task_id)?;
                println!("Task deleted: {}", task_id);
                Ok(0)
            }
            TaskCommands::Retry { task_item_id } => {
                let task_id = crate::reset_task_item_for_retry(&self.state, task_item_id)?;
                crate::prepare_task_for_start(&self.state, &task_id)?;
                let runtime = crate::RunningTask::new();
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::run_task_loop(
                    self.state.clone(),
                    None,
                    &task_id,
                    runtime,
                ))?;
                let summary = crate::load_task_summary(&self.state, &task_id)?;
                println!("Retry finished: {} status={}", summary.id, summary.status);
                Ok(0)
            }
        }
    }

    fn handle_workspace(&self, cmd: &WorkspaceCommands) -> Result<i32> {
        let active = crate::read_active_config(&self.state)?;
        match cmd {
            WorkspaceCommands::List { output } => {
                let workspaces: Vec<_> = active.config.workspaces.keys().cloned().collect();
                self.print_workspaces(&workspaces, &active.config.workspaces, *output)
            }
            WorkspaceCommands::Info {
                workspace_id,
                output,
            } => {
                let ws = active
                    .config
                    .workspaces
                    .get(workspace_id)
                    .context(format!("workspace not found: {}", workspace_id))?;
                self.print_workspace_detail(workspace_id, ws, *output)
            }
        }
    }

    fn handle_config(&self, cmd: &ConfigCommands) -> Result<i32> {
        match cmd {
            ConfigCommands::View { output } => {
                let overview = crate::load_config_overview(&self.state)?;
                self.print_config(&overview, *output)
            }
            ConfigCommands::Set { config_file } => {
                let content = std::fs::read_to_string(config_file)?;
                let config: crate::OrchestratorConfig = serde_yaml::from_str(&content)?;
                crate::persist_config_and_reload(&self.state, config, content, "cli")?;
                println!("Configuration updated");
                Ok(0)
            }
            ConfigCommands::Validate { config_file } => {
                let content = std::fs::read_to_string(config_file)?;
                let config: crate::OrchestratorConfig = serde_yaml::from_str(&content)?;
                let candidate = crate::build_active_config(&self.state.app_root, config)?;
                let normalized = serde_yaml::to_string(&candidate.config)?;
                println!("Configuration is valid:\n{}", normalized);
                Ok(0)
            }
            ConfigCommands::ListWorkflows { output } => {
                let active = crate::read_active_config(&self.state)?;
                self.print_workflows(&active.config.workflows, *output)
            }
            ConfigCommands::ListAgents { output } => {
                let active = crate::read_active_config(&self.state)?;
                self.print_agents(&active.config.agents, *output)
            }
        }
    }

    fn print_tasks(
        &self,
        tasks: &[crate::TaskSummary],
        format: OutputFormat,
        verbose: bool,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(tasks)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(tasks)?;
                println!("{}", yaml);
            }
            OutputFormat::Table => {
                println!(
                    "{:<38} {:<12} {:<10} {:<8} {:<8}",
                    "ID", "NAME", "STATUS", "FINISHED", "FAILED"
                );
                println!("{:-<38} {:-<12} {:-<10} {:-<8} {:-<8}", "", "", "", "", "");
                for t in tasks {
                    println!(
                        "{:<38} {:<12} {:<10} {:<8} {:<8}",
                        &t.id[..8],
                        &t.name[..std::cmp::min(12, t.name.len())],
                        t.status,
                        t.finished_items,
                        t.failed_items
                    );
                }
            }
        }
        Ok(0)
    }

    fn print_task_detail(&self, detail: &crate::TaskDetail, format: OutputFormat) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(detail)?);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(detail)?);
            }
            OutputFormat::Table => {
                let t = &detail.task;
                println!("Task: {}", t.id);
                println!("  Name: {}", t.name);
                println!("  Status: {}", t.status);
                println!("  Workspace: {}", t.workspace_id);
                println!("  Workflow: {}", t.workflow_id);
                println!("  Progress: {}/{} items", t.finished_items, t.total_items);
                println!("  Failed: {}", t.failed_items);
                if !t.goal.is_empty() {
                    println!("  Goal: {}", t.goal);
                }
                if !t.target_files.is_empty() {
                    println!("  Target Files: {:?}", t.target_files);
                }
            }
        }
        Ok(0)
    }

    fn print_workspaces(
        &self,
        ids: &[String],
        workspaces: &std::collections::HashMap<String, crate::WorkspaceConfig>,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(workspaces)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(workspaces)?);
            }
            OutputFormat::Table => {
                println!("{:<20} {:<40}", "ID", "ROOT PATH");
                println!("{:-<20} {:-<40}", "", "");
                for id in ids {
                    if let Some(ws) = workspaces.get(id) {
                        println!("{:<20} {:<40}", id, ws.root_path);
                    }
                }
            }
        }
        Ok(0)
    }

    fn print_workspace_detail(
        &self,
        id: &str,
        ws: &crate::WorkspaceConfig,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(ws)?;
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(ws)?);
            }
            OutputFormat::Table => {
                println!("Workspace: {}", id);
                println!("  Root Path: {}", ws.root_path);
                println!("  QA Targets: {:?}", ws.qa_targets);
                println!("  Ticket Dir: {}", ws.ticket_dir);
            }
        }
        Ok(0)
    }

    fn print_config(&self, overview: &crate::ConfigOverview, format: OutputFormat) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&overview.config)?);
            }
            OutputFormat::Yaml | OutputFormat::Table => {
                println!("{}", overview.yaml);
            }
        }
        Ok(0)
    }

    fn print_workflows(
        &self,
        workflows: &std::collections::HashMap<String, crate::WorkflowConfig>,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(workflows)?);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(workflows)?);
            }
            OutputFormat::Table => {
                println!("{:<20} {:<30}", "ID", "STEPS");
                println!("{:-<20} {:-<30}", "", "");
                for (id, wf) in workflows {
                    let steps: Vec<_> = wf
                        .steps
                        .iter()
                        .filter(|s| s.enabled)
                        .map(|s| s.step_type.as_str())
                        .collect();
                    println!("{:<20} {:<30}", id, steps.join(", "));
                }
            }
        }
        Ok(0)
    }

    fn print_agents(
        &self,
        agents: &std::collections::HashMap<String, crate::AgentConfig>,
        format: OutputFormat,
    ) -> Result<i32> {
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(agents)?);
            }
            OutputFormat::Yaml => {
                println!("{}", serde_yaml::to_string(agents)?);
            }
            OutputFormat::Table => {
                println!("{:<20} {:<20}", "ID", "PHASES");
                println!("{:-<20} {:-<20}", "", "");
                for (id, cfg) in agents {
                    let mut phases = Vec::new();
                    let t = &cfg.templates;
                    if t.init_once.is_some() {
                        phases.push("init_once");
                    }
                    if t.qa.is_some() {
                        phases.push("qa");
                    }
                    if t.fix.is_some() {
                        phases.push("fix");
                    }
                    if t.retest.is_some() {
                        phases.push("retest");
                    }
                    println!("{:<20} {:<20}", id, phases.join(", "));
                }
            }
        }
        Ok(0)
    }
}
