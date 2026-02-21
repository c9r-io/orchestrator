use crate::cli::{
    generate_completion, Cli, Commands, CompletionCommands, ConfigCommands, DbCommands,
    EditCommands, OutputFormat, TaskCommands, WorkspaceCommands,
};
use crate::cli_types::OrchestratorResource;
use crate::resource::{
    dispatch_resource, AgentResource, ApplyResult, RegisteredResource, Resource, WorkflowResource,
    WorkspaceResource,
};
use crate::InnerState;
use anyhow::{Context, Result};
use clap_complete::Shell;
use serde::{Deserialize, Serialize};
use serde_yaml;
use std::path::Path;
use std::process::{Command, ExitStatus};
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
            Commands::Init { .. } => {
                // Init command is handled in main.rs before reaching here
                Ok(0)
            }
            Commands::Apply { file, dry_run } => self.handle_apply(file, *dry_run),
            Commands::Get { resource, output } => self.handle_get(resource, *output),
            Commands::Describe { resource, output } => self.handle_describe(resource, *output),
            Commands::Task(cmd) => self.handle_task(cmd),
            Commands::Workspace(cmd) => self.handle_workspace(cmd),
            Commands::Config(cmd) => self.handle_config(cmd),
            Commands::Edit(cmd) => self.handle_edit(cmd),
            Commands::Db(cmd) => self.handle_db(cmd),
            Commands::Completion(cmd) => self.handle_completion(cmd),
            Commands::Daemon => {
                println!("Starting daemon mode (UI)... use --cli flag for CLI mode");
                Ok(0)
            }
            Commands::Debug { component } => self.handle_debug(component.as_deref()),
        }
    }

    fn handle_debug(&self, component: Option<&str>) -> Result<i32> {
        let comp = component.unwrap_or("state");

        match comp {
            "state" => {
                println!("Debug Information");
                println!("=================");
                println!("");
                println!("Note: MessageBus is an internal component.");
                println!("Use 'orchestrator task list' and 'orchestrator task logs' for runtime debugging.");
                println!("");
                println!("Available debug components:");
                println!("  state     - Show runtime state info (this)");
                println!("  config    - Show active configuration");
                println!("  messagebus - Show MessageBus status (internal)");
                Ok(0)
            }
            "config" => {
                let config = crate::read_active_config(&self.state)?;
                println!("Active Configuration:");
                println!(
                    "{}",
                    serde_yaml::to_string(&config.config).unwrap_or_default()
                );
                Ok(0)
            }
            "messagebus" => {
                println!("MessageBus Debug Information");
                println!("============================");
                println!("");
                println!("MessageBus is an internal component for agent-to-agent communication.");
                println!(
                    "It is initialized in InnerState and used for publishing/subscribing messages."
                );
                println!("");
                println!("Implementation location: src/message_bus.rs");
                println!("");
                println!("To verify MessageBus is working:");
                println!("  1. Run a task with multiple agents");
                println!("  2. Check logs for message_bus events");
                Ok(0)
            }
            _ => {
                eprintln!("Unknown debug component: {}", comp);
                eprintln!("Available: state, config, messagebus");
                Ok(1)
            }
        }
    }

    fn handle_apply(&self, file: &str, dry_run: bool) -> Result<i32> {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read manifest file: {}", file))?;
        let resources = Self::parse_resources_from_yaml(&content)?;
        let mut merged_config = {
            let active = crate::read_active_config(&self.state)?;
            active.config.clone()
        };

        let mut has_errors = false;
        let mut applied_results = Vec::new();
        for (index, manifest) in resources.into_iter().enumerate() {
            let resource = match manifest.validate_version() {
                Ok(()) => manifest,
                Err(error) => {
                    eprintln!("document {}: {}", index + 1, error);
                    has_errors = true;
                    continue;
                }
            };

            let registered = match dispatch_resource(resource) {
                Ok(resource) => resource,
                Err(error) => {
                    eprintln!("document {}: {}", index + 1, error);
                    has_errors = true;
                    continue;
                }
            };

            if let Err(error) = registered.validate() {
                eprintln!(
                    "{} / {} invalid: {}",
                    kind_as_str(registered.kind()),
                    registered.name(),
                    error
                );
                has_errors = true;
                continue;
            }

            let result = self.apply_resource(&mut merged_config, &registered);
            applied_results.push(result);
            let action = match result {
                ApplyResult::Created => "created",
                ApplyResult::Configured | ApplyResult::Unchanged => "configured",
            };

            if dry_run {
                println!(
                    "{}/{} would be {} (dry run)",
                    kind_as_str(registered.kind()),
                    registered.name(),
                    action
                );
            } else {
                println!(
                    "{}/{} {}",
                    kind_as_str(registered.kind()),
                    registered.name(),
                    action
                );
            }
        }

        if has_errors {
            return Ok(1);
        }

        if !dry_run && !applied_results.is_empty() {
            let merged_yaml = serde_yaml::to_string(&merged_config)
                .context("failed to serialize applied configuration")?;
            crate::persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;
        }

        Ok(0)
    }

    fn handle_get(&self, resource: &str, output: OutputFormat) -> Result<i32> {
        let parts: Vec<&str> = resource.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid resource format: {} (use format: resource/name, e.g., ws/default)",
                resource
            );
        }
        let (kind, name) = (parts[0], parts[1]);

        match kind {
            "ws" | "workspace" => self.handle_workspace(&WorkspaceCommands::Info {
                workspace_id: name.to_string(),
                output,
            }),
            "wf" | "workflow" => {
                let active = crate::read_active_config(&self.state)?;
                if let Some(wf) = active.config.workflows.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(wf)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(wf)?);
                        }
                        OutputFormat::Table => {
                            let step_types: Vec<String> = wf
                                .steps
                                .iter()
                                .map(|s| format!("{:?}", s.step_type))
                                .collect();
                            println!("{:<20} {:<40}", name, step_types.join(", "));
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("workflow not found: {}", name)
                }
            }
            "agent" => {
                let active = crate::read_active_config(&self.state)?;
                if let Some(agent) = active.config.agents.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(agent)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(agent)?);
                        }
                        OutputFormat::Table => {
                            let templates: Vec<&str> = [
                                agent.templates.get("init_once").map(|s| s.as_str()),
                                agent.templates.get("qa").map(|s| s.as_str()),
                                agent.templates.get("fix").map(|s| s.as_str()),
                                agent.templates.get("retest").map(|s| s.as_str()),
                                agent.templates.get("loop_guard").map(|s| s.as_str()),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            println!("{:<20} {:?}", name, templates);
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("agent not found: {}", name)
                }
            }
            "task" | "t" => self.handle_task(&TaskCommands::Info {
                task_id: name.to_string(),
                output,
            }),
            _ => anyhow::bail!(
                "unknown resource type: {} (supported: ws/workspace, wf/workflow, agent, task)",
                kind
            ),
        }
    }

    fn handle_describe(&self, resource: &str, output: OutputFormat) -> Result<i32> {
        let parts: Vec<&str> = resource.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid resource format: {} (use format: resource/name)",
                resource
            );
        }
        let (kind, name) = (parts[0], parts[1]);

        match kind {
            "ws" | "workspace" => self.handle_workspace(&WorkspaceCommands::Info {
                workspace_id: name.to_string(),
                output,
            }),
            "wf" | "workflow" => {
                let active = crate::read_active_config(&self.state)?;
                if let Some(wf) = active.config.workflows.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(wf)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(wf)?);
                        }
                        OutputFormat::Table => {
                            let step_types: Vec<String> = wf
                                .steps
                                .iter()
                                .map(|s| format!("{:?}", s.step_type))
                                .collect();
                            println!("{:<20} {:<40}", name, step_types.join(", "));
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("workflow not found: {}", name)
                }
            }
            "agent" => {
                let active = crate::read_active_config(&self.state)?;
                if let Some(agent) = active.config.agents.get(name) {
                    match output {
                        OutputFormat::Json => {
                            println!("{}", serde_json::to_string_pretty(agent)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(agent)?);
                        }
                        OutputFormat::Table => {
                            let templates: Vec<&str> = [
                                agent.templates.get("init_once").map(|s| s.as_str()),
                                agent.templates.get("qa").map(|s| s.as_str()),
                                agent.templates.get("fix").map(|s| s.as_str()),
                                agent.templates.get("retest").map(|s| s.as_str()),
                                agent.templates.get("loop_guard").map(|s| s.as_str()),
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                            println!("{:<20} {:?}", name, templates);
                        }
                    }
                    Ok(0)
                } else {
                    anyhow::bail!("agent not found: {}", name)
                }
            }
            "task" | "t" => self.handle_task(&TaskCommands::Info {
                task_id: name.to_string(),
                output,
            }),
            _ => anyhow::bail!(
                "unknown resource type: {} (supported: ws/workspace, wf/workflow, agent, task)",
                kind
            ),
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
                    project_id: None,
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
                    crate::resolve_task_id(&self.state, id)?
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
                let resolved_id = crate::resolve_task_id(&self.state, task_id)?;
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::stop_task_runtime(
                    self.state.clone(),
                    &resolved_id,
                    "paused",
                ))?;
                println!("Task paused: {}", resolved_id);
                Ok(0)
            }
            TaskCommands::Resume { task_id } => {
                let resolved_id = crate::resolve_task_id(&self.state, task_id)?;
                crate::prepare_task_for_start(&self.state, &resolved_id)?;
                let runtime = crate::RunningTask::new();
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::run_task_loop(
                    self.state.clone(),
                    None,
                    &resolved_id,
                    runtime,
                ))?;
                let summary = crate::load_task_summary(&self.state, &resolved_id)?;
                println!("Task finished: {} status={}", summary.id, summary.status);
                Ok(0)
            }
            TaskCommands::Logs {
                task_id,
                follow: _,
                tail: _,
                timestamps: _,
            } => {
                let resolved_id = crate::resolve_task_id(&self.state, task_id)?;
                let logs = crate::stream_task_logs_impl(&self.state, &resolved_id, 300)?;
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
                let resolved_id = crate::resolve_task_id(&self.state, task_id)?;
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(crate::stop_task_runtime_for_delete(
                    self.state.clone(),
                    &resolved_id,
                ))?;
                crate::delete_task_impl(&self.state, &resolved_id)?;
                println!("Task deleted: {}", resolved_id);
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

    fn handle_edit(&self, cmd: &EditCommands) -> Result<i32> {
        match cmd {
            EditCommands::Export { selector } => {
                let (kind_str, name) = parse_resource_selector(selector)?;
                let active = crate::read_active_config(&self.state)?;
                let resource = RegisteredResource::get_from(&active.config, name)
                    .with_context(|| format!("resource not found: {}/{}", kind_str, name))?;
                let yaml = resource.to_yaml()?;
                let temp_file = write_to_temp_file(&yaml)?;
                println!("{}", temp_file.display());
                Ok(0)
            }
            EditCommands::Open { selector } => self.edit_open(selector),
        }
    }

    fn edit_open(&self, selector: &str) -> Result<i32> {
        let (kind_str, name) = parse_resource_selector(selector)?;
        let (resource, mut merged_config) = {
            let active = crate::read_active_config(&self.state)?;
            let resource = RegisteredResource::get_from(&active.config, name)
                .with_context(|| format!("resource not found: {}/{}", kind_str, name))?;
            (resource, active.config.clone())
        };

        let yaml = resource.to_yaml()?;
        let temp_file = write_to_temp_file(&yaml)?;
        let _temp_guard = TempFileGuard::new(temp_file.clone());

        let editor = std::env::var("EDITOR").context("$EDITOR is not set")?;
        loop {
            let status = self.run_editor(&editor, &temp_file)?;
            if is_ctrl_c_exit(&status) {
                eprintln!("Edit aborted by Ctrl+C");
                return Ok(130);
            }

            if !status.success() {
                anyhow::bail!("editor exited with non-zero status: {}", status);
            }

            let edited = std::fs::read_to_string(&temp_file)
                .with_context(|| format!("failed to read temp file: {}", temp_file.display()))?;
            if edited.trim().is_empty() {
                eprintln!("Edit aborted: empty file");
                return Ok(1);
            }

            let manifest: OrchestratorResource = match serde_yaml::from_str(&edited) {
                Ok(resource) => resource,
                Err(error) => {
                    eprintln!("Edited manifest is invalid YAML: {}", error);
                    continue;
                }
            };

            if let Err(error) = manifest.validate_version() {
                eprintln!("Edited manifest has invalid apiVersion: {}", error);
                continue;
            }

            let registered = match dispatch_resource(manifest) {
                Ok(resource) => resource,
                Err(error) => {
                    eprintln!("Edited manifest has invalid kind/spec: {}", error);
                    continue;
                }
            };

            if let Err(error) = registered.validate() {
                eprintln!(
                    "{} / {} invalid: {}",
                    kind_as_str(registered.kind()),
                    registered.name(),
                    error
                );
                continue;
            }

            let result = self.apply_resource(&mut merged_config, &registered);
            let merged_yaml = serde_yaml::to_string(&merged_config)
                .context("failed to serialize edited configuration")?;
            crate::persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;

            let action = match result {
                ApplyResult::Created => "created",
                ApplyResult::Configured | ApplyResult::Unchanged => "configured",
            };
            println!(
                "{}/{} {}",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            );
            return Ok(0);
        }
    }

    fn run_editor(&self, editor: &str, temp_file: &std::path::Path) -> Result<ExitStatus> {
        Command::new(editor)
            .arg(temp_file)
            .status()
            .with_context(|| format!("failed to start editor command: {}", editor))
    }

    fn handle_db(&self, cmd: &DbCommands) -> Result<i32> {
        match cmd {
            DbCommands::Reset { force } => {
                if !force {
                    eprintln!("Use --force to confirm database reset");
                    return Ok(1);
                }
                crate::reset_db(&self.state)?;
                println!("Database reset completed");
                Ok(0)
            }
        }
    }

    fn handle_completion(&self, cmd: &CompletionCommands) -> Result<i32> {
        let shell = match cmd {
            CompletionCommands::Bash => Shell::Bash,
            CompletionCommands::Zsh => Shell::Zsh,
            CompletionCommands::Fish => Shell::Fish,
            CompletionCommands::PowerShell => Shell::PowerShell,
        };
        generate_completion(shell);
        Ok(0)
    }

    fn parse_resources_from_yaml(content: &str) -> Result<Vec<OrchestratorResource>> {
        let mut resources = Vec::new();
        for document in serde_yaml::Deserializer::from_str(content) {
            let value = serde_yaml::Value::deserialize(document)?;
            if value.is_null() {
                continue;
            }
            let resource = serde_yaml::from_value::<OrchestratorResource>(value)?;
            resources.push(resource);
        }
        Ok(resources)
    }

    fn apply_resource(
        &self,
        config: &mut crate::OrchestratorConfig,
        resource: &RegisteredResource,
    ) -> ApplyResult {
        let existed = match resource {
            RegisteredResource::Workspace(current) => {
                WorkspaceResource::get_from(config, current.name()).is_some()
            }
            RegisteredResource::Agent(current) => {
                AgentResource::get_from(config, current.name()).is_some()
            }
            RegisteredResource::Workflow(current) => {
                WorkflowResource::get_from(config, current.name()).is_some()
            }
        };

        let _ = resource.apply(config);
        if existed {
            ApplyResult::Configured
        } else {
            ApplyResult::Created
        }
    }

    fn print_tasks(
        &self,
        tasks: &[crate::TaskSummary],
        format: OutputFormat,
        _verbose: bool,
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
                        let root_path = Path::new(&ws.root_path);
                        let absolute_path = if root_path.is_absolute() {
                            root_path.to_path_buf()
                        } else {
                            self.state
                                .app_root
                                .join(&ws.root_path)
                                .canonicalize()
                                .unwrap_or_else(|_| self.state.app_root.join(&ws.root_path))
                        };
                        println!("{:<20} {:<40}", id, absolute_path.display());
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
                        .map(|s| s.id.as_str())
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
                println!("{:<20} {:<20}", "ID", "CAPABILITIES");
                println!("{:-<20} {:-<20}", "", "");
                for (id, cfg) in agents {
                    println!("{:<20} {:<20}", id, cfg.capabilities.join(", "));
                }
            }
        }
        Ok(0)
    }
}

fn kind_as_str(kind: crate::cli_types::ResourceKind) -> &'static str {
    match kind {
        crate::cli_types::ResourceKind::Workspace => "workspace",
        crate::cli_types::ResourceKind::Agent => "agent",
        crate::cli_types::ResourceKind::Workflow => "workflow",
    }
}

fn parse_resource_selector(selector: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = selector.splitn(2, '/').collect();
    match parts.as_slice() {
        [kind, name] => {
            if kind.trim().is_empty() || name.trim().is_empty() {
                anyhow::bail!(
                    "invalid resource selector format: expected kind/name, got '{}'",
                    selector
                );
            }
            Ok((kind, name))
        }
        _ => anyhow::bail!(
            "invalid resource selector format: expected kind/name, got '{}'",
            selector
        ),
    }
}

fn write_to_temp_file(content: &str) -> Result<std::path::PathBuf> {
    let temp_dir = std::env::temp_dir();
    let uuid = uuid::Uuid::new_v4();
    let filename = format!("orchestrator-edit-{}.yaml", uuid);
    let temp_file = temp_dir.join(&filename);
    std::fs::write(&temp_file, content)
        .with_context(|| format!("failed to write temp file: {}", temp_file.display()))?;
    Ok(temp_file)
}

struct TempFileGuard {
    path: std::path::PathBuf,
}

impl TempFileGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn is_ctrl_c_exit(status: &ExitStatus) -> bool {
    if status.code() == Some(130) {
        return true;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(2) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Mutex, OnceLock};

    fn write_manifest(path: &std::path::Path, content: &str) {
        std::fs::write(path, content).expect("manifest should be writable");
    }

    fn workspace_manifest_yaml(name: &str, root_path: &str) -> String {
        format!(
            "apiVersion: orchestrator.dev/v1\nkind: Workspace\nmetadata:\n  name: {name}\nspec:\n  root_path: {root_path}\n  qa_targets:\n    - docs/qa\n  ticket_dir: docs/ticket\n"
        )
    }

    fn ensure_workspace_structure(temp_root: &std::path::Path, root_path: &str) {
        let root = temp_root.join(root_path);
        std::fs::create_dir_all(root.join("docs/qa")).expect("qa dir should be creatable");
        std::fs::create_dir_all(root.join("docs/ticket")).expect("ticket dir should be creatable");
    }

    fn editor_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_editor_env<T>(editor: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = editor_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var("EDITOR").ok();

        match editor {
            Some(value) => unsafe { std::env::set_var("EDITOR", value) },
            None => unsafe { std::env::remove_var("EDITOR") },
        }

        let result = f();

        match previous {
            Some(value) => unsafe { std::env::set_var("EDITOR", value) },
            None => unsafe { std::env::remove_var("EDITOR") },
        }

        result
    }

    fn write_mock_editor_script(path: &std::path::Path, body: &str) {
        let script = format!("#!/bin/sh\nset -eu\n{}\n", body);
        std::fs::write(path, script).expect("mock editor script should be writable");
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms).expect("mock editor script should be executable");
    }

    #[test]
    fn parse_resource_selector_workspace_default() {
        let (kind, name) =
            parse_resource_selector("workspace/default").expect("parsing should succeed");
        assert_eq!(kind, "workspace");
        assert_eq!(name, "default");
    }

    #[test]
    fn parse_resource_selector_agent_opencode() {
        let (kind, name) =
            parse_resource_selector("agent/opencode").expect("parsing should succeed");
        assert_eq!(kind, "agent");
        assert_eq!(name, "opencode");
    }

    #[test]
    fn parse_resource_selector_with_slash_in_name_uses_first_slash_only() {
        let (kind, name) =
            parse_resource_selector("workflow/my/workflow").expect("parsing should succeed");
        assert_eq!(kind, "workflow");
        assert_eq!(name, "my/workflow");
    }

    #[test]
    fn parse_resource_selector_rejects_missing_kind() {
        let err = parse_resource_selector("/name").expect_err("should reject missing kind");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn parse_resource_selector_rejects_missing_name() {
        let err = parse_resource_selector("workspace/").expect_err("should reject missing name");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn parse_resource_selector_rejects_no_separator() {
        let err = parse_resource_selector("workspace-default")
            .expect_err("should reject missing separator");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn edit_export_returns_temp_file_path() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Export {
                selector: "workspace/default".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let code = handler.execute(&cli).expect("edit export should succeed");
        assert_eq!(code, 0);
    }

    #[test]
    fn edit_export_returns_error_for_missing_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Export {
                selector: "workspace/nonexistent".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result);
        assert!(err_msg.contains("not found"));
    }

    #[test]
    fn edit_open_requires_editor_env() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let result = with_editor_env(None, || handler.execute(&cli));
        assert!(result.is_err());
        let err_text = format!(
            "{:#}",
            result.expect_err("should fail when EDITOR is unset")
        );
        assert!(err_text.contains("$EDITOR is not set"));
    }

    #[test]
    fn edit_open_applies_valid_edit() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-valid.sh");

        ensure_workspace_structure(fixture.temp_root(), "workspace/default-updated");

        write_mock_editor_script(
            &editor_path,
            r#"cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default-updated
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML"#,
        );

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler.execute(&cli).expect("edit open should succeed")
        });
        assert_eq!(code, 0);

        let active = crate::read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("default")
            .expect("workspace should exist");
        assert_eq!(workspace.root_path, "workspace/default-updated");
    }

    #[test]
    fn edit_validation_reopens_until_manifest_is_valid() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-reopen.sh");
        let count_file = fixture.temp_root().join("mock-editor-count.txt");

        ensure_workspace_structure(fixture.temp_root(), "workspace/default-reopened");

        write_mock_editor_script(
            &editor_path,
            &format!(
                r#"count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf "%s" "$count" > "$count_file"

if [ "$count" -eq 1 ]; then
  cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: default
spec:
  root_path: ""
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML
else
  cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default-reopened
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML
fi"#,
                count_file.display()
            ),
        );

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler
                .execute(&cli)
                .expect("edit open should eventually succeed")
        });
        assert_eq!(code, 0);

        let count = std::fs::read_to_string(&count_file).expect("count file should be present");
        assert_eq!(count.trim(), "2");

        let active = crate::read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("default")
            .expect("workspace should exist");
        assert_eq!(workspace.root_path, "workspace/default-reopened");
    }

    #[test]
    fn edit_open_handles_ctrl_c_gracefully() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-ctrlc.sh");

        write_mock_editor_script(&editor_path, "exit 130");

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            config: None,
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler
                .execute(&cli)
                .expect("ctrl+c should return exit code, not error")
        });
        assert_eq!(code, 130);
    }

    #[test]
    fn apply_dry_run_does_not_persist_created_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let manifest_path = fixture.temp_root().join("apply-created.yaml");

        write_manifest(
            &manifest_path,
            r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: dry-run-created
spec:
  root_path: workspace/dry-run-created
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#,
        );

        let cli = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: true,
            },
            config: None,
            verbose: false,
        };

        let code = handler.execute(&cli).expect("dry-run should succeed");
        assert_eq!(code, 0);

        let active = crate::read_active_config(&state).expect("config should be readable");
        assert!(!active.config.workspaces.contains_key("dry-run-created"));
    }

    #[test]
    fn apply_dry_run_returns_one_on_invalid_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let manifest_path = fixture.temp_root().join("apply-invalid.yaml");

        write_manifest(
            &manifest_path,
            r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: invalid-workspace
spec:
  root_path: ""
  qa_targets: []
  ticket_dir: docs/ticket
"#,
        );

        let cli = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: true,
            },
            config: None,
            verbose: false,
        };

        let code = handler
            .execute(&cli)
            .expect("invalid dry-run should still return exit code");
        assert_eq!(code, 1);
    }

    #[test]
    fn multi_document_apply_dry_run_parses_all_documents() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let manifest_path = fixture.temp_root().join("apply-multi.yaml");

        write_manifest(
            &manifest_path,
            r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default
  qa_targets:
    - docs/qa
    - docs/security
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: dry-run-multi
spec:
  root_path: workspace/dry-run-multi
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#,
        );

        let parsed = CliHandler::parse_resources_from_yaml(
            &std::fs::read_to_string(&manifest_path).expect("manifest should be readable"),
        )
        .expect("multi-document parsing should succeed");
        assert_eq!(parsed.len(), 2);

        let cli = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: true,
            },
            config: None,
            verbose: false,
        };

        let code = handler.execute(&cli).expect("dry-run should succeed");
        assert_eq!(code, 0);

        let active = crate::read_active_config(&state).expect("config should be readable");
        assert!(!active.config.workspaces.contains_key("dry-run-multi"));
        assert!(active.config.workspaces.contains_key("default"));
    }

    #[test]
    fn apply_create_non_dry_run_creates_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let manifest_path = fixture.temp_root().join("apply-create.yaml");

        ensure_workspace_structure(fixture.temp_root(), "workspace/apply-create");

        write_manifest(
            &manifest_path,
            &workspace_manifest_yaml("apply-create", "workspace/apply-create"),
        );

        let cli = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: false,
            },
            config: None,
            verbose: false,
        };

        let code = handler.execute(&cli).expect("apply create should succeed");
        assert_eq!(code, 0);

        let active = crate::read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("apply-create")
            .expect("workspace should be created");
        assert_eq!(workspace.root_path, "workspace/apply-create");
    }

    #[test]
    fn apply_update_non_dry_run_updates_existing_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let manifest_path = fixture.temp_root().join("apply-update.yaml");

        ensure_workspace_structure(fixture.temp_root(), "workspace/apply-update-v1");
        ensure_workspace_structure(fixture.temp_root(), "workspace/apply-update-v2");

        write_manifest(
            &manifest_path,
            &workspace_manifest_yaml("apply-update", "workspace/apply-update-v1"),
        );

        let first_apply = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: false,
            },
            config: None,
            verbose: false,
        };
        assert_eq!(handler.execute(&first_apply).expect("first apply"), 0);

        write_manifest(
            &manifest_path,
            &workspace_manifest_yaml("apply-update", "workspace/apply-update-v2"),
        );
        let second_apply = Cli {
            command: Commands::Apply {
                file: manifest_path.display().to_string(),
                dry_run: false,
            },
            config: None,
            verbose: false,
        };
        assert_eq!(handler.execute(&second_apply).expect("second apply"), 0);

        let active = crate::read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("apply-update")
            .expect("workspace should be updated");
        assert_eq!(workspace.root_path, "workspace/apply-update-v2");
    }

    #[test]
    fn apply_persist_non_dry_run_writes_new_config_version() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let dry_manifest_path = fixture.temp_root().join("apply-persist-dry.yaml");
        let apply_manifest_path = fixture.temp_root().join("apply-persist.yaml");

        ensure_workspace_structure(fixture.temp_root(), "workspace/apply-persist-dry");
        ensure_workspace_structure(fixture.temp_root(), "workspace/apply-persist");

        let baseline_version = crate::load_config_overview(&state)
            .expect("baseline overview should be readable")
            .version;

        write_manifest(
            &dry_manifest_path,
            &workspace_manifest_yaml("apply-persist-dry", "workspace/apply-persist-dry"),
        );
        let dry_run_cli = Cli {
            command: Commands::Apply {
                file: dry_manifest_path.display().to_string(),
                dry_run: true,
            },
            config: None,
            verbose: false,
        };
        assert_eq!(
            handler
                .execute(&dry_run_cli)
                .expect("dry run should succeed"),
            0
        );
        let version_after_dry_run = crate::load_config_overview(&state)
            .expect("overview after dry run should be readable")
            .version;
        assert_eq!(version_after_dry_run, baseline_version);

        write_manifest(
            &apply_manifest_path,
            &workspace_manifest_yaml("apply-persist", "workspace/apply-persist"),
        );
        let apply_cli = Cli {
            command: Commands::Apply {
                file: apply_manifest_path.display().to_string(),
                dry_run: false,
            },
            config: None,
            verbose: false,
        };
        assert_eq!(
            handler.execute(&apply_cli).expect("apply should succeed"),
            0
        );

        let version_after_apply = crate::load_config_overview(&state)
            .expect("overview after apply should be readable")
            .version;
        assert_eq!(version_after_apply, baseline_version + 1);
    }
}
