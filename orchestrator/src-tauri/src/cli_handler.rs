use crate::cli::{
    Cli, Commands, ConfigCommands, DbCommands, OutputFormat, TaskCommands, WorkspaceCommands,
};
use crate::cli_types::OrchestratorResource;
use crate::resource::{
    dispatch_resource, AgentGroupResource, AgentResource, ApplyResult, RegisteredResource,
    Resource, WorkflowResource, WorkspaceResource,
};
use crate::InnerState;
use anyhow::{Context, Result};
use serde::Deserialize;
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
            Commands::Apply { file, dry_run } => self.handle_apply(file, *dry_run),
            Commands::Task(cmd) => self.handle_task(cmd),
            Commands::Workspace(cmd) => self.handle_workspace(cmd),
            Commands::Config(cmd) => self.handle_config(cmd),
            Commands::Db(cmd) => self.handle_db(cmd),
            Commands::Daemon => {
                println!("Starting daemon mode (UI)... use --cli flag for CLI mode");
                Ok(0)
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
                follow: _,
                tail: _,
                timestamps: _,
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
            RegisteredResource::AgentGroup(current) => {
                AgentGroupResource::get_from(config, current.name()).is_some()
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

fn kind_as_str(kind: crate::cli_types::ResourceKind) -> &'static str {
    match kind {
        crate::cli_types::ResourceKind::Workspace => "workspace",
        crate::cli_types::ResourceKind::Agent => "agent",
        crate::cli_types::ResourceKind::AgentGroup => "agentgroup",
        crate::cli_types::ResourceKind::Workflow => "workflow",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

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
