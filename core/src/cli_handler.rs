use crate::cli::{
    generate_completion, AgentCommands, Cli, Commands, CompletionCommands, ConfigCommands,
    DbCommands, EditCommands, OutputFormat, QaCommands, QaProjectCommands, TaskCommands,
    TaskWorkerCommands, WorkflowCommands, WorkspaceCommands,
};
use crate::cli_types::{
    AgentSpec, AgentTemplatesSpec, OrchestratorResource, ResourceKind, ResourceMetadata,
    ResourceSpec, WorkflowFinalizeSpec, WorkflowLoopSpec, WorkflowSpec, WorkflowStepSpec,
    WorkspaceSpec,
};
use crate::config::OrchestratorConfig;
use crate::config::{
    AgentConfig, LoopMode, ProjectConfig, WorkflowConfig, WorkflowStepType, WorkspaceConfig,
};
use crate::config_load::{load_config_overview, persist_config_and_reload, read_active_config};
use crate::db::reset_db;
use crate::dto::ConfigOverview;
use crate::dto::{CreateTaskPayload, TaskDetail, TaskSummary};
use crate::resource::{dispatch_resource, kind_as_str, ApplyResult, RegisteredResource, Resource};
use crate::scheduler::{
    delete_task_impl, find_latest_resumable_task_id, get_task_details_impl, list_tasks_impl,
    load_task_summary, prepare_task_for_start, resolve_task_id, run_task_loop, stop_task_runtime,
    stop_task_runtime_for_delete, stream_task_logs_impl, RunningTask,
};
use crate::scheduler_service::{
    clear_worker_stop_signal, enqueue_task, next_pending_task_id, pending_task_count,
    signal_worker_stop, worker_stop_signal_path,
};
use crate::state::InnerState;
use crate::task_ops::{create_task_impl, reset_task_item_for_retry};
use anyhow::{Context, Result};
use clap_complete::Shell;
use serde_json::json;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::{Arc, OnceLock};

fn cli_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to initialize shared tokio runtime for CLI")
    })
}

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
            Commands::Apply { .. } => {
                unreachable!("apply is handled as a preflight command in main.rs")
            }
            Commands::Get {
                resource,
                output,
                selector,
            } => self.handle_get(resource, *output, selector.as_deref()),
            Commands::Describe { resource, output } => self.handle_describe(resource, *output),
            Commands::Delete { resource, force } => self.handle_delete(resource, *force),
            Commands::Task(cmd) => self.handle_task(cmd),
            Commands::Workspace(cmd) => self.handle_workspace(cmd),
            Commands::Agent(cmd) => self.handle_agent(cmd),
            Commands::Workflow(cmd) => self.handle_workflow(cmd),
            Commands::Config(cmd) => self.handle_config(cmd),
            Commands::Edit(cmd) => self.handle_edit(cmd),
            Commands::Db(cmd) => self.handle_db(cmd),
            Commands::Qa(cmd) => self.handle_qa(cmd),
            Commands::Completion(cmd) => self.handle_completion(cmd),
            Commands::Debug { component } => self.handle_debug(component.as_deref()),
        }
    }

    fn handle_debug(&self, component: Option<&str>) -> Result<i32> {
        let comp = component.unwrap_or("state");

        match comp {
            "state" => {
                println!("Debug Information");
                println!("=================");
                println!();
                println!("Note: MessageBus is an internal component.");
                println!("Use 'orchestrator task list' and 'orchestrator task logs' for runtime debugging.");
                println!();
                println!("Available debug components:");
                println!("  state     - Show runtime state info (this)");
                println!("  config    - Show active configuration");
                println!("  messagebus - Show MessageBus status (internal)");
                Ok(0)
            }
            "config" => {
                let config = read_active_config(&self.state)?;
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
                println!();
                println!("MessageBus is an internal component for agent-to-agent communication.");
                println!(
                    "It is initialized in InnerState and used for publishing/subscribing messages."
                );
                println!();
                println!("Implementation location: src/collab.rs (MessageBus)");
                println!();
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

    fn handle_get(
        &self,
        resource: &str,
        output: OutputFormat,
        selector: Option<&str>,
    ) -> Result<i32> {
        if resource.contains('/') {
            if selector.is_some() {
                anyhow::bail!("--selector/-l is only supported for list queries");
            }
            return self.handle_get_single(resource, output);
        }

        self.handle_get_list(resource, output, selector)
    }

    fn handle_get_single(&self, resource: &str, output: OutputFormat) -> Result<i32> {
        let (kind, name) = parse_resource_selector(resource)?;

        match kind {
            "ws" | "workspace" => self.handle_workspace(&WorkspaceCommands::Info {
                workspace_id: name.to_string(),
                output,
            }),
            "wf" | "workflow" => {
                let active = read_active_config(&self.state)?;
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
                let active = read_active_config(&self.state)?;
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

    fn handle_get_list(
        &self,
        resource_type: &str,
        output: OutputFormat,
        selector: Option<&str>,
    ) -> Result<i32> {
        let selector_terms = selector
            .map(parse_label_selector)
            .transpose()?
            .unwrap_or_default();
        let active = read_active_config(&self.state)?;

        match resource_type {
            "ws" | "workspace" | "workspaces" => {
                let rows: Vec<_> = active
                    .config
                    .workspaces
                    .iter()
                    .filter_map(|(name, ws)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            labels: active
                                .config
                                .resource_meta
                                .workspaces
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .workspaces
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        Some(json!({
                            "name": name,
                            "root_path": ws.root_path,
                            "qa_targets": ws.qa_targets,
                            "ticket_dir": ws.ticket_dir,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("WORKSPACE", rows, output, |row| {
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<40} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        row["root_path"].as_str().unwrap_or_default(),
                        labels
                    )
                })
            }
            "agent" | "agents" => {
                let rows: Vec<_> = active
                    .config
                    .agents
                    .iter()
                    .filter_map(|(name, agent)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            labels: active
                                .config
                                .resource_meta
                                .agents
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .agents
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        Some(json!({
                            "name": name,
                            "capabilities": agent.capabilities,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("AGENT", rows, output, |row| {
                    let capabilities = row["capabilities"]
                        .as_array()
                        .map(|caps| {
                            caps.iter()
                                .filter_map(|c| c.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<30} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        capabilities,
                        labels
                    )
                })
            }
            "wf" | "workflow" | "workflows" => {
                let rows: Vec<_> = active
                    .config
                    .workflows
                    .iter()
                    .filter_map(|(name, workflow)| {
                        let metadata = ResourceMetadata {
                            name: name.clone(),
                            labels: active
                                .config
                                .resource_meta
                                .workflows
                                .get(name)
                                .and_then(|m| m.labels.clone()),
                            annotations: active
                                .config
                                .resource_meta
                                .workflows
                                .get(name)
                                .and_then(|m| m.annotations.clone()),
                        };
                        if !matches_selector(&metadata.labels, &selector_terms) {
                            return None;
                        }
                        let steps: Vec<String> = workflow
                            .steps
                            .iter()
                            .filter_map(|s| s.step_type.as_ref().map(|t| t.as_str().to_string()))
                            .collect();
                        Some(json!({
                            "name": name,
                            "steps": steps,
                            "labels": metadata.labels,
                            "annotations": metadata.annotations,
                        }))
                    })
                    .collect();
                self.print_resource_rows("WORKFLOW", rows, output, |row| {
                    let steps = row["steps"]
                        .as_array()
                        .map(|steps| {
                            steps
                                .iter()
                                .filter_map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        })
                        .unwrap_or_default();
                    let labels = row
                        .get("labels")
                        .and_then(|v| v.as_object())
                        .map(string_map_to_csv)
                        .unwrap_or_else(|| "-".to_string());
                    format!(
                        "{:<20} {:<30} {:<30}",
                        row["name"].as_str().unwrap_or_default(),
                        steps,
                        labels
                    )
                })
            }
            _ => anyhow::bail!(
                "unknown list resource type: {} (supported: workspaces, agents, workflows)",
                resource_type
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
                let active = read_active_config(&self.state)?;
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
                let active = read_active_config(&self.state)?;
                if let Some(agent) = active.config.agents.get(name) {
                    match output {
                        OutputFormat::Json => {
                            let mut obj = serde_json::to_value(agent)?;
                            if let Some(map) = obj.as_object_mut() {
                                map.insert("output_schema".to_string(), json!({
                                    "type": "AgentOutput",
                                    "fields": {
                                        "exit_code": "i64",
                                        "stdout": "String",
                                        "stderr": "String",
                                        "artifacts": "[Artifact]",
                                        "confidence": "f32 (0.0-1.0)",
                                        "quality_score": "f32 (0.0-1.0)"
                                    },
                                    "artifact_kinds": ["ticket", "code_change", "test_result", "analysis", "decision"]
                                }));
                            }
                            println!("{}", serde_json::to_string_pretty(&obj)?);
                        }
                        OutputFormat::Yaml => {
                            println!("{}", serde_yaml::to_string(agent)?);
                        }
                        OutputFormat::Table => {
                            println!("Agent: {}", name);
                            println!("  Cost: {:?}", agent.metadata.cost);
                            println!("  Capabilities: {:?}", agent.capabilities);
                            println!("  Strategy: {:?}", agent.selection.strategy);
                            println!("  Templates:");
                            for (phase, tmpl) in &agent.templates {
                                println!("    {}: {}", phase, tmpl);
                            }
                            println!("  Output Schema: AgentOutput {{ exit_code, stdout, artifacts, confidence, quality_score }}");
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

    fn handle_delete(&self, resource: &str, force: bool) -> Result<i32> {
        let parts: Vec<&str> = resource.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!(
                "invalid resource format: {} (use format: kind/name, e.g., workspace/my-ws)",
                resource
            );
        }
        let (kind, name) = (parts[0], parts[1]);

        if !force {
            println!("Use --force to confirm deletion of {}/{}", kind, name);
            return Ok(0);
        }

        let mut config = {
            let active = read_active_config(&self.state)?;
            active.config.clone()
        };

        if (kind == "ws" || kind == "workspace") && config.defaults.workspace == name {
            anyhow::bail!(
                "cannot delete workspace '{}': it is the current default workspace",
                name
            );
        }
        if (kind == "wf" || kind == "workflow") && config.defaults.workflow == name {
            anyhow::bail!(
                "cannot delete workflow '{}': it is the current default workflow",
                name
            );
        }

        let deleted = crate::resource::delete_resource_by_kind(&mut config, kind, name)?;
        if !deleted {
            anyhow::bail!("{}/{} not found", kind, name);
        }

        let yaml = serde_yaml::to_string(&config)
            .context("failed to serialize configuration after delete")?;
        persist_config_and_reload(&self.state, config, yaml, "cli")?;
        println!("{}/{} deleted", kind, name);
        Ok(0)
    }

    fn handle_task(&self, cmd: &TaskCommands) -> Result<i32> {
        match cmd {
            TaskCommands::List {
                status,
                output,
                verbose,
            } => {
                let tasks = list_tasks_impl(&self.state)?;
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
                        enqueue_task(&self.state, &created.id)?;
                        println!("Task enqueued: {}", created.id);
                    } else {
                        prepare_task_for_start(&self.state, &created.id)?;
                        let runtime = RunningTask::new();
                        cli_runtime().block_on(run_task_loop(
                            self.state.clone(),
                            &created.id,
                            runtime,
                        ))?;
                        let summary = load_task_summary(&self.state, &created.id)?;
                        println!("Task finished: {} status={}", summary.id, summary.status);
                    }
                }
                Ok(0)
            }
            TaskCommands::Info { task_id, output } => {
                let detail = get_task_details_impl(&self.state, task_id)?;
                self.print_task_detail(&detail, *output)
            }
            TaskCommands::Start {
                task_id,
                latest,
                detach,
            } => {
                let id = if let Some(id) = task_id {
                    resolve_task_id(&self.state, id)?
                } else if *latest {
                    find_latest_resumable_task_id(&self.state, true)?
                        .context("no resumable task found")?
                } else {
                    anyhow::bail!("task_id or --latest required")
                };
                if *detach {
                    enqueue_task(&self.state, &id)?;
                    println!("Task enqueued: {}", id);
                } else {
                    prepare_task_for_start(&self.state, &id)?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(self.state.clone(), &id, runtime))?;
                    let summary = load_task_summary(&self.state, &id)?;
                    println!("Task finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Pause { task_id } => {
                let resolved_id = resolve_task_id(&self.state, task_id)?;
                cli_runtime().block_on(stop_task_runtime(
                    self.state.clone(),
                    &resolved_id,
                    "paused",
                ))?;
                println!("Task paused: {}", resolved_id);
                Ok(0)
            }
            TaskCommands::Resume { task_id, detach } => {
                let resolved_id = resolve_task_id(&self.state, task_id)?;
                if *detach {
                    enqueue_task(&self.state, &resolved_id)?;
                    println!("Task enqueued: {}", resolved_id);
                } else {
                    prepare_task_for_start(&self.state, &resolved_id)?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(
                        self.state.clone(),
                        &resolved_id,
                        runtime,
                    ))?;
                    let summary = load_task_summary(&self.state, &resolved_id)?;
                    println!("Task finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Logs {
                task_id,
                follow: _,
                tail,
                timestamps,
            } => {
                let resolved_id = resolve_task_id(&self.state, task_id)?;
                let logs = stream_task_logs_impl(&self.state, &resolved_id, *tail, *timestamps)?;
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
                let resolved_id = resolve_task_id(&self.state, task_id)?;
                cli_runtime().block_on(stop_task_runtime_for_delete(
                    self.state.clone(),
                    &resolved_id,
                ))?;
                delete_task_impl(&self.state, &resolved_id)?;
                println!("Task deleted: {}", resolved_id);
                Ok(0)
            }
            TaskCommands::Retry {
                task_item_id,
                detach,
            } => {
                let task_id = reset_task_item_for_retry(&self.state, task_item_id)?;
                if *detach {
                    enqueue_task(&self.state, &task_id)?;
                    println!("Task enqueued: {}", task_id);
                } else {
                    prepare_task_for_start(&self.state, &task_id)?;
                    let runtime = RunningTask::new();
                    cli_runtime().block_on(run_task_loop(self.state.clone(), &task_id, runtime))?;
                    let summary = load_task_summary(&self.state, &task_id)?;
                    println!("Retry finished: {} status={}", summary.id, summary.status);
                }
                Ok(0)
            }
            TaskCommands::Worker(cmd) => self.handle_task_worker(cmd),
        }
    }

    fn handle_task_worker(&self, cmd: &TaskWorkerCommands) -> Result<i32> {
        match cmd {
            TaskWorkerCommands::Start { poll_ms } => {
                clear_worker_stop_signal(&self.state)?;
                println!("Worker started (poll={}ms)", poll_ms);
                let stop_file = worker_stop_signal_path(&self.state);
                loop {
                    if stop_file.exists() {
                        clear_worker_stop_signal(&self.state)?;
                        println!("Worker stopped");
                        break;
                    }
                    if let Some(task_id) = next_pending_task_id(&self.state)? {
                        prepare_task_for_start(&self.state, &task_id)?;
                        let runtime = RunningTask::new();
                        cli_runtime().block_on(run_task_loop(
                            self.state.clone(),
                            &task_id,
                            runtime,
                        ))?;
                        let summary = load_task_summary(&self.state, &task_id)?;
                        println!(
                            "Worker finished task: {} status={}",
                            summary.id, summary.status
                        );
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(*poll_ms));
                    }
                }
                Ok(0)
            }
            TaskWorkerCommands::Stop => {
                signal_worker_stop(&self.state)?;
                println!("Worker stop signal written");
                Ok(0)
            }
            TaskWorkerCommands::Status => {
                let pending = pending_task_count(&self.state)?;
                let stop_signal = worker_stop_signal_path(&self.state).exists();
                println!("pending_tasks: {}", pending);
                println!("stop_signal: {}", stop_signal);
                Ok(0)
            }
        }
    }

    fn handle_workspace(&self, cmd: &WorkspaceCommands) -> Result<i32> {
        let active = read_active_config(&self.state)?;
        match cmd {
            WorkspaceCommands::List { output, project } => {
                let ws_map = if let Some(proj_id) = project {
                    active
                        .config
                        .projects
                        .get(proj_id)
                        .map(|p| &p.workspaces)
                        .unwrap_or(&active.config.workspaces)
                } else {
                    &active.config.workspaces
                };
                let workspaces: Vec<_> = ws_map.keys().cloned().collect();
                self.print_workspaces(&workspaces, ws_map, *output)
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
            WorkspaceCommands::Create {
                name,
                root_path,
                qa_target,
                ticket_dir,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = WorkspaceSpec {
                    root_path: root_path.clone(),
                    qa_targets: if qa_target.is_empty() {
                        vec!["docs/qa".to_string()]
                    } else {
                        qa_target.clone()
                    },
                    ticket_dir: ticket_dir.clone(),
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v1".to_string(),
                    kind: ResourceKind::Workspace,
                    metadata,
                    spec: ResourceSpec::Workspace(spec),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }

    fn handle_agent(&self, cmd: &AgentCommands) -> Result<i32> {
        match cmd {
            AgentCommands::Create {
                name,
                template_init_once,
                template_qa,
                template_fix,
                template_retest,
                template_loop_guard,
                capability,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = AgentSpec {
                    templates: AgentTemplatesSpec {
                        init_once: template_init_once.clone(),
                        qa: template_qa.clone(),
                        fix: template_fix.clone(),
                        retest: template_retest.clone(),
                        loop_guard: template_loop_guard.clone(),
                    },
                    capabilities: if capability.is_empty() {
                        None
                    } else {
                        Some(capability.clone())
                    },
                    metadata: None,
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v1".to_string(),
                    kind: ResourceKind::Agent,
                    metadata,
                    spec: ResourceSpec::Agent(spec),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }

    fn handle_workflow(&self, cmd: &WorkflowCommands) -> Result<i32> {
        match cmd {
            WorkflowCommands::Create {
                name,
                step,
                loop_mode,
                max_cycles,
                labels,
                annotations,
                dry_run,
                output,
            } => {
                let loop_mode_normalized = normalize_loop_mode(loop_mode)?;
                let steps: Vec<WorkflowStepSpec> = step
                    .iter()
                    .map(|step_type| validate_workflow_step_type(step_type))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .map(|step_type| WorkflowStepSpec {
                        id: step_type.clone(),
                        step_type,
                        enabled: true,
                        prehook: None,
                    })
                    .collect();

                let metadata = build_resource_metadata(name, labels, annotations)?;
                let spec = WorkflowSpec {
                    steps,
                    loop_policy: WorkflowLoopSpec {
                        mode: loop_mode_normalized,
                        max_cycles: *max_cycles,
                    },
                    finalize: WorkflowFinalizeSpec { rules: vec![] },
                };
                let manifest = OrchestratorResource {
                    api_version: "orchestrator.dev/v1".to_string(),
                    kind: ResourceKind::Workflow,
                    metadata,
                    spec: ResourceSpec::Workflow(spec),
                };
                self.apply_or_preview_manifest(manifest, *dry_run, *output)
            }
        }
    }

    fn handle_config(&self, cmd: &ConfigCommands) -> Result<i32> {
        match cmd {
            ConfigCommands::View { output } => {
                let overview = load_config_overview(&self.state)?;
                self.print_config(&overview, *output)
            }
            ConfigCommands::Set { config_file } => {
                let content = std::fs::read_to_string(config_file)?;
                let config: OrchestratorConfig = serde_yaml::from_str(&content)?;
                persist_config_and_reload(&self.state, config, content, "cli")?;
                println!("Configuration updated");
                Ok(0)
            }
            ConfigCommands::Export { output, file } => {
                let overview = load_config_overview(&self.state)?;
                let content = match output {
                    OutputFormat::Yaml => overview.yaml,
                    OutputFormat::Json => serde_json::to_string_pretty(&overview.config)?,
                    OutputFormat::Table => {
                        anyhow::bail!("unsupported export output format: table")
                    }
                };
                if let Some(path) = file {
                    std::fs::write(path, &content)
                        .with_context(|| format!("failed to write export file: {}", path))?;
                    println!("Configuration exported to {}", path);
                } else {
                    println!("{}", content);
                }
                Ok(0)
            }
            ConfigCommands::Validate { .. } => {
                anyhow::bail!("config validate is handled as a preflight command")
            }
            ConfigCommands::ListWorkflows { output } => {
                let active = read_active_config(&self.state)?;
                self.print_workflows(&active.config.workflows, *output)
            }
            ConfigCommands::ListAgents { output } => {
                let active = read_active_config(&self.state)?;
                self.print_agents(&active.config.agents, *output)
            }
        }
    }

    fn handle_edit(&self, cmd: &EditCommands) -> Result<i32> {
        match cmd {
            EditCommands::Export { selector } => {
                let (kind_str, name) = parse_resource_selector(selector)?;
                let active = read_active_config(&self.state)?;
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
            let active = read_active_config(&self.state)?;
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

            let result = registered.apply(&mut merged_config);
            let merged_yaml = serde_yaml::to_string(&merged_config)
                .context("failed to serialize edited configuration")?;
            persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;

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
            DbCommands::Reset {
                force,
                include_history,
                include_config,
            } => {
                if !force {
                    eprintln!("Use --force to confirm database reset");
                    return Ok(1);
                }
                reset_db(&self.state, *include_history, *include_config)?;
                println!("Database reset completed");
                if *include_config {
                    println!("All config versions deleted (next apply starts from blank)");
                } else if *include_history {
                    println!("Config version history cleared (active version preserved)");
                }
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

    fn handle_qa(&self, cmd: &QaCommands) -> Result<i32> {
        match cmd {
            QaCommands::Project(project_cmd) => self.handle_qa_project(project_cmd),
            QaCommands::Doctor { output } => self.handle_qa_doctor(*output),
        }
    }

    fn handle_qa_project(&self, cmd: &QaProjectCommands) -> Result<i32> {
        match cmd {
            QaProjectCommands::Create {
                project_id,
                from_workspace,
                workflow,
                workspace,
                root_path,
                qa_target,
                ticket_dir,
                force,
            } => {
                let mut config = {
                    let active = read_active_config(&self.state)?;
                    active.config.clone()
                };

                let source_workspace = config
                    .workspaces
                    .get(from_workspace)
                    .with_context(|| format!("source workspace not found: {}", from_workspace))?
                    .clone();
                let workflow_id = workflow
                    .clone()
                    .unwrap_or_else(|| config.defaults.workflow.clone());
                let source_workflow = config
                    .workflows
                    .get(&workflow_id)
                    .with_context(|| format!("workflow not found: {}", workflow_id))?
                    .clone();

                let workspace_id = workspace
                    .clone()
                    .unwrap_or_else(|| format!("{}-ws", project_id));
                let resolved_root_path = root_path
                    .clone()
                    .unwrap_or_else(|| format!("workspace/{}", project_id));
                let resolved_qa_targets = if qa_target.is_empty() {
                    source_workspace.qa_targets
                } else {
                    qa_target.clone()
                };

                let new_workspace = WorkspaceConfig {
                    root_path: resolved_root_path.clone(),
                    qa_targets: resolved_qa_targets,
                    ticket_dir: ticket_dir.clone(),
                };

                let project = config
                    .projects
                    .entry(project_id.clone())
                    .or_insert_with(|| ProjectConfig {
                        description: Some("qa isolated project".to_string()),
                        workspaces: std::collections::HashMap::new(),
                        agents: std::collections::HashMap::new(),
                        workflows: std::collections::HashMap::new(),
                    });

                if !*force && !project.workspaces.is_empty() {
                    anyhow::bail!(
                        "project '{}' already exists; pass --force to overwrite project workspace/workflow",
                        project_id
                    );
                }

                project
                    .workspaces
                    .insert(workspace_id.clone(), new_workspace);
                project
                    .workflows
                    .insert(workflow_id.clone(), source_workflow);

                let workspace_root = self.state.app_root.join(&resolved_root_path);
                std::fs::create_dir_all(&workspace_root).with_context(|| {
                    format!(
                        "failed to create workspace root for project '{}': {}",
                        project_id,
                        workspace_root.display()
                    )
                })?;
                if let Some(ws) = project.workspaces.get(&workspace_id) {
                    for target in &ws.qa_targets {
                        std::fs::create_dir_all(workspace_root.join(target)).with_context(
                            || {
                                format!(
                                    "failed to create qa target dir for project '{}': {}",
                                    project_id, target
                                )
                            },
                        )?;
                    }
                    std::fs::create_dir_all(workspace_root.join(&ws.ticket_dir)).with_context(
                        || {
                            format!(
                                "failed to create ticket dir for project '{}': {}",
                                project_id, ws.ticket_dir
                            )
                        },
                    )?;
                }

                let yaml = serde_yaml::to_string(&config)
                    .context("failed to serialize configuration after qa project create")?;
                persist_config_and_reload(&self.state, config, yaml, "qa-project-create")?;

                println!(
                    "qa project created: project={} workspace={} workflow={}",
                    project_id, workspace_id, workflow_id
                );
                Ok(0)
            }
            QaProjectCommands::Reset {
                project_id,
                keep_config,
                force,
            } => {
                if !force {
                    println!(
                        "Use --force to confirm qa project reset for '{}' (sqlite DB file is preserved)",
                        project_id
                    );
                    return Ok(0);
                }

                let removed = crate::db::reset_project_data(&self.state, project_id)?;

                if !keep_config {
                    let mut config = {
                        let active = read_active_config(&self.state)?;
                        active.config.clone()
                    };
                    config.projects.remove(project_id);
                    let yaml = serde_yaml::to_string(&config).context(
                        "failed to serialize configuration after qa project config cleanup",
                    )?;
                    persist_config_and_reload(&self.state, config, yaml, "qa-project-reset")?;
                }

                println!(
                    "qa project reset completed: project={} tasks={} items={} runs={} events={} config_kept={}",
                    project_id,
                    removed.tasks,
                    removed.task_items,
                    removed.command_runs,
                    removed.events,
                    keep_config
                );
                Ok(0)
            }
        }
    }

    fn handle_qa_doctor(&self, format: OutputFormat) -> Result<i32> {
        let conn = crate::db::open_conn(&self.state.db_path)?;
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .unwrap_or_default();
        let busy_timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .unwrap_or(0);
        let total_task_metrics: i64 =
            conn.query_row("SELECT COUNT(*) FROM task_execution_metrics", [], |row| {
                row.get(0)
            })?;
        let completed_task_metrics: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_execution_metrics WHERE status = 'completed'",
            [],
            |row| row.get(0),
        )?;
        let recent24_task_metrics: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_execution_metrics WHERE datetime(created_at) >= datetime('now', '-1 day')",
            [],
            |row| row.get(0),
        )?;
        let completion_rate = if total_task_metrics > 0 {
            (completed_task_metrics as f64) / (total_task_metrics as f64)
        } else {
            0.0
        };

        let active = read_active_config(&self.state)?;
        let checks = json!({
            "sqlite": {
                "journal_mode": journal_mode,
                "busy_timeout_ms": busy_timeout_ms,
            },
            "observability": {
                "task_execution_metrics_total": total_task_metrics,
                "task_execution_metrics_last_24h": recent24_task_metrics,
                "task_completion_rate": completion_rate,
            },
            "config": {
                "default_project": active.default_project_id,
                "project_count": active.config.projects.len(),
            },
            "recommendations": [
                "Use unique qa project id per scenario run",
                "Use `orchestrator qa project reset <project> --keep-config --force` between reruns",
            ]
        });

        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&checks)?),
            OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&checks)?),
            OutputFormat::Table => {
                println!("QA Doctor");
                println!("---------");
                println!("sqlite.journal_mode: {}", checks["sqlite"]["journal_mode"]);
                println!(
                    "sqlite.busy_timeout_ms: {}",
                    checks["sqlite"]["busy_timeout_ms"]
                );
                println!(
                    "config.default_project: {}",
                    checks["config"]["default_project"]
                        .as_str()
                        .unwrap_or_default()
                );
                println!(
                    "config.project_count: {}",
                    checks["config"]["project_count"].as_u64().unwrap_or(0)
                );
                println!(
                    "observability.task_execution_metrics_total: {}",
                    checks["observability"]["task_execution_metrics_total"]
                );
                println!(
                    "observability.task_execution_metrics_last_24h: {}",
                    checks["observability"]["task_execution_metrics_last_24h"]
                );
                println!(
                    "observability.task_completion_rate: {:.3}",
                    checks["observability"]["task_completion_rate"]
                        .as_f64()
                        .unwrap_or(0.0)
                );
            }
        }
        Ok(0)
    }

    fn apply_or_preview_manifest(
        &self,
        manifest: OrchestratorResource,
        dry_run: bool,
        output: OutputFormat,
    ) -> Result<i32> {
        manifest
            .validate_version()
            .map_err(anyhow::Error::msg)
            .context("invalid apiVersion in generated manifest")?;
        let registered = dispatch_resource(manifest.clone())?;
        registered.validate()?;

        if dry_run {
            match output {
                OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&manifest)?),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
                OutputFormat::Table => {
                    anyhow::bail!("dry-run output format does not support table; use yaml or json")
                }
            }
            return Ok(0);
        }

        let mut merged_config = {
            let active = read_active_config(&self.state)?;
            active.config.clone()
        };
        let result = registered.apply(&mut merged_config);
        let merged_yaml = serde_yaml::to_string(&merged_config)
            .context("failed to serialize updated configuration")?;
        persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;

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
        Ok(0)
    }

    fn print_resource_rows<F>(
        &self,
        kind: &str,
        rows: Vec<serde_json::Value>,
        format: OutputFormat,
        table_row: F,
    ) -> Result<i32>
    where
        F: Fn(&serde_json::Value) -> String,
    {
        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
            OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&rows)?),
            OutputFormat::Table => {
                println!("{kind} LIST");
                println!("{}", "-".repeat(kind.len() + 5));
                for row in &rows {
                    println!("{}", table_row(row));
                }
            }
        }
        Ok(0)
    }

    fn print_tasks(
        &self,
        tasks: &[TaskSummary],
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

    fn print_task_detail(&self, detail: &TaskDetail, format: OutputFormat) -> Result<i32> {
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
        workspaces: &std::collections::HashMap<String, WorkspaceConfig>,
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
        ws: &WorkspaceConfig,
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

    fn print_config(&self, overview: &ConfigOverview, format: OutputFormat) -> Result<i32> {
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
        workflows: &std::collections::HashMap<String, WorkflowConfig>,
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
        agents: &std::collections::HashMap<String, AgentConfig>,
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

fn parse_key_value_pairs(
    values: &[String],
    field_name: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    for raw in values {
        let (key, value) = raw.split_once('=').with_context(|| {
            format!("invalid {} entry '{}': expected key=value", field_name, raw)
        })?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            anyhow::bail!(
                "invalid {} entry '{}': key/value cannot be empty",
                field_name,
                raw
            );
        }
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

fn build_resource_metadata(
    name: &str,
    labels: &[String],
    annotations: &[String],
) -> Result<ResourceMetadata> {
    let label_map = parse_key_value_pairs(labels, "label")?;
    let annotation_map = parse_key_value_pairs(annotations, "annotation")?;
    Ok(ResourceMetadata {
        name: name.to_string(),
        labels: if label_map.is_empty() {
            None
        } else {
            Some(label_map)
        },
        annotations: if annotation_map.is_empty() {
            None
        } else {
            Some(annotation_map)
        },
    })
}

fn parse_label_selector(selector: &str) -> Result<Vec<(String, String)>> {
    let mut terms = Vec::new();
    for part in selector.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            anyhow::bail!("invalid label selector '{}': empty segment", selector);
        }
        let (key, value) = trimmed
            .split_once('=')
            .with_context(|| format!("invalid label selector '{}': expected key=value", trimmed))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            anyhow::bail!(
                "invalid label selector '{}': key/value cannot be empty",
                trimmed
            );
        }
        terms.push((key.to_string(), value.to_string()));
    }
    Ok(terms)
}

fn matches_selector(
    labels: &Option<std::collections::HashMap<String, String>>,
    selector: &[(String, String)],
) -> bool {
    if selector.is_empty() {
        return true;
    }
    let Some(labels) = labels else {
        return false;
    };
    selector
        .iter()
        .all(|(key, expected)| labels.get(key) == Some(expected))
}

fn string_map_to_csv(map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut items: Vec<String> = map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}={s}")))
        .collect();
    items.sort();
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(",")
    }
}

fn normalize_loop_mode(loop_mode: &str) -> Result<String> {
    let parsed = loop_mode.parse::<LoopMode>().map_err(|_| {
        anyhow::anyhow!(
            "invalid --loop-mode '{}': expected one of once|infinite",
            loop_mode
        )
    })?;
    let normalized = match parsed {
        LoopMode::Once => "once",
        LoopMode::Infinite => "infinite",
    };
    Ok(normalized.to_string())
}

fn validate_workflow_step_type(value: &str) -> Result<String> {
    let parsed = value.parse::<WorkflowStepType>().map_err(|_| {
        anyhow::anyhow!(
            "invalid --step '{}': expected init_once|qa|ticket_scan|fix|retest|loop_guard",
            value
        )
    })?;
    Ok(parsed.as_str().to_string())
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
    use crate::db::open_conn;
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
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler.execute(&cli).expect("edit open should succeed")
        });
        assert_eq!(code, 0);

        let active = read_active_config(&state).expect("config should be readable");
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

        let active = read_active_config(&state).expect("config should be readable");
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
    fn multi_document_yaml_parses_all_documents() {
        let yaml = r#"
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: ws-a
spec:
  root_path: workspace/ws-a
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v1
kind: Workspace
metadata:
  name: ws-b
spec:
  root_path: workspace/ws-b
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#;

        let parsed = crate::resource::parse_resources_from_yaml(yaml)
            .expect("multi-document parsing should succeed");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].metadata.name, "ws-a");
        assert_eq!(parsed[1].metadata.name, "ws-b");
    }

    #[test]
    fn delete_requires_force_flag() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/default".to_string(),
                force: false,
            },
            verbose: false,
        };

        let code = handler
            .execute(&cli)
            .expect("should succeed without deleting");
        assert_eq!(code, 0);

        let active = read_active_config(&state).expect("config should be readable");
        assert!(active.config.workspaces.contains_key("default"));
    }

    #[test]
    fn delete_rejects_default_workspace() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/default".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("default workspace"));
    }

    #[test]
    fn delete_rejects_default_workflow() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workflow/basic".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("default workflow"));
    }

    #[test]
    fn delete_nonexistent_resource_returns_error() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Delete {
                resource: "workspace/nonexistent".to_string(),
                force: true,
            },
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("not found"));
    }

    #[test]
    fn parse_label_selector_supports_comma_separated_equals() {
        let parsed = parse_label_selector("env=prod,tier=backend").expect("should parse");
        assert_eq!(
            parsed,
            vec![
                ("env".to_string(), "prod".to_string()),
                ("tier".to_string(), "backend".to_string())
            ]
        );
    }

    #[test]
    fn parse_label_selector_rejects_invalid_term() {
        let err = parse_label_selector("env").expect_err("selector should be invalid");
        assert!(err.to_string().contains("expected key=value"));
    }

    #[test]
    fn get_single_resource_rejects_selector_flag() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let cli = Cli {
            command: Commands::Get {
                resource: "workspace/default".to_string(),
                output: OutputFormat::Table,
                selector: Some("env=dev".to_string()),
            },
            verbose: false,
        };

        let err = handler
            .execute(&cli)
            .expect_err("selector should fail for single get");
        assert!(err.to_string().contains("only supported for list queries"));
    }

    #[test]
    fn workspace_create_dry_run_emits_manifest() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let cli = Cli {
            command: Commands::Workspace(WorkspaceCommands::Create {
                name: "dry-run-ws".to_string(),
                root_path: "workspace/dry-run".to_string(),
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                labels: vec!["env=dev".to_string()],
                annotations: vec![],
                dry_run: true,
                output: OutputFormat::Yaml,
            }),
            verbose: false,
        };

        let code = handler.execute(&cli).expect("dry-run should pass");
        assert_eq!(code, 0);
    }

    #[test]
    fn qa_project_create_then_reset_keep_config_cleans_only_project_data() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let create_project = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id: "qa-isolated".to_string(),
                from_workspace: "default".to_string(),
                workflow: Some("basic".to_string()),
                workspace: Some("qa-isolated-ws".to_string()),
                root_path: Some("workspace/qa-isolated".to_string()),
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                force: true,
            })),
            verbose: false,
        };
        assert_eq!(
            handler
                .execute(&create_project)
                .expect("qa project create should succeed"),
            0
        );
        let qa_file = fixture
            .temp_root()
            .join("workspace/qa-isolated/docs/qa/sample.md");
        std::fs::write(&qa_file, "# sample\n").expect("qa sample file should be writable");

        let create_task = Cli {
            command: Commands::Task(TaskCommands::Create {
                name: Some("qa-proj-task".to_string()),
                goal: Some("verify reset".to_string()),
                project: Some("qa-isolated".to_string()),
                workspace: Some("qa-isolated-ws".to_string()),
                workflow: Some("basic".to_string()),
                target_file: vec![],
                no_start: true,
                detach: false,
            }),
            verbose: false,
        };
        assert_eq!(
            handler
                .execute(&create_task)
                .expect("task create should succeed"),
            0
        );

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let before_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = 'qa-isolated'",
                [],
                |row| row.get(0),
            )
            .expect("count before reset");
        assert!(before_count >= 1);

        let reset = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id: "qa-isolated".to_string(),
                keep_config: true,
                force: true,
            })),
            verbose: false,
        };
        assert_eq!(handler.execute(&reset).expect("qa reset should succeed"), 0);

        let after_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = 'qa-isolated'",
                [],
                |row| row.get(0),
            )
            .expect("count after reset");
        assert_eq!(after_count, 0);
        drop(conn);
        assert!(state.db_path.exists());

        let active = read_active_config(&state).expect("config should be readable");
        assert!(active.config.projects.contains_key("qa-isolated"));
    }

    #[test]
    fn qa_project_reset_without_keep_config_removes_project_entry() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let create = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id: "qa-drop".to_string(),
                from_workspace: "default".to_string(),
                workflow: Some("basic".to_string()),
                workspace: None,
                root_path: None,
                qa_target: vec![],
                ticket_dir: "docs/ticket".to_string(),
                force: true,
            })),
            verbose: false,
        };
        handler
            .execute(&create)
            .expect("qa project create should succeed");

        let reset = Cli {
            command: Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id: "qa-drop".to_string(),
                keep_config: false,
                force: true,
            })),
            verbose: false,
        };
        handler
            .execute(&reset)
            .expect("qa project reset should succeed");

        let active = read_active_config(&state).expect("config should be readable");
        assert!(!active.config.projects.contains_key("qa-drop"));
    }
}
