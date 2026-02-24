use crate::cli_handler::CliHandler;
use crate::state::InnerState;
use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::sync::Arc;

/// Agent Orchestrator CLI - kubectl-like command-line interface
#[derive(Parser, Debug)]
#[command(
    name = "orchestrator",
    version = "0.1.0",
    about = "Agent Orchestrator - workflow automation CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Initialize orchestrator runtime directories and SQLite schema
    Init {
        /// Workspace root path (default: current directory)
        #[arg(short, long)]
        root: Option<String>,

        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },

    #[command(alias = "ap")]
    Apply {
        #[arg(short = 'f', long = "file")]
        file: String,

        #[arg(long)]
        dry_run: bool,
    },

    #[command(alias = "g")]
    Get {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Label selector (e.g., env=prod,tier=backend) for list queries
        #[arg(short = 'l', long = "selector")]
        selector: Option<String>,
    },

    #[command(alias = "desc")]
    Describe {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },

    /// Delete a resource by kind/name (e.g., workspace/my-ws)
    #[command(alias = "rm")]
    Delete {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        /// Force deletion without confirmation
        #[arg(short, long)]
        force: bool,
    },

    #[command(alias = "t", subcommand)]
    Task(TaskCommands),

    #[command(alias = "ws", subcommand)]
    Workspace(WorkspaceCommands),

    #[command(subcommand)]
    Agent(AgentCommands),

    #[command(subcommand)]
    Workflow(WorkflowCommands),

    #[command(alias = "m", subcommand)]
    Manifest(ManifestCommands),

    #[command(alias = "e", subcommand)]
    Edit(EditCommands),

    #[command(subcommand)]
    Db(DbCommands),

    #[command(subcommand)]
    Qa(QaCommands),

    #[command(alias = "comp", subcommand)]
    Completion(CompletionCommands),

    #[command(alias = "dbg")]
    Debug {
        #[arg(long)]
        component: Option<String>,
    },

    /// Execute a command in a task step context (use -it for interactive mode)
    Exec {
        /// Keep stdin open
        #[arg(short = 'i', long)]
        stdin: bool,

        /// Allocate interactive terminal behavior
        #[arg(short = 't', long)]
        tty: bool,

        /// Target selector: task/<task_id>/step/<step_id>
        target: String,

        /// Command to execute in the selected step context
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TaskCommands {
    /// List all tasks
    #[command(alias = "ls")]
    List {
        /// Filter by status (pending, running, paused, completed, failed)
        #[arg(short, long)]
        status: Option<String>,

        /// Output format (table, json, yaml)
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Show all columns including verbose info
        #[arg(short, long)]
        verbose: bool,
    },

    /// Create a new task
    #[command(alias = "new")]
    Create {
        /// Task name
        #[arg(short, long)]
        name: Option<String>,

        /// Task goal/description
        #[arg(short, long)]
        goal: Option<String>,

        /// Project ID to use
        #[arg(short, long)]
        project: Option<String>,

        /// Workspace ID to use
        #[arg(short, long)]
        workspace: Option<String>,

        /// Workflow ID to use
        #[arg(short = 'W', long)]
        workflow: Option<String>,

        /// Target files to process (can be specified multiple times)
        #[arg(short, long)]
        target_file: Vec<String>,

        /// Don't auto-start the task after creation
        #[arg(long)]
        no_start: bool,

        /// Enqueue task for background worker instead of running inline
        #[arg(long)]
        detach: bool,
    },

    /// Get task details
    #[command(alias = "get")]
    Info {
        /// Task ID
        task_id: String,

        /// Output format
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Start a task
    Start {
        /// Task ID to start
        task_id: Option<String>,

        /// Auto-select latest resumable task
        #[arg(long, short)]
        latest: bool,

        /// Enqueue task for background worker instead of running inline
        #[arg(long)]
        detach: bool,
    },

    /// Pause a running task
    Pause {
        /// Task ID to pause
        task_id: String,
    },

    /// Resume a paused task
    Resume {
        /// Task ID to resume
        task_id: String,

        /// Enqueue task for background worker instead of running inline
        #[arg(long)]
        detach: bool,
    },

    /// View task logs
    #[command(alias = "log")]
    Logs {
        /// Task ID
        task_id: String,

        /// Follow logs in real-time
        #[arg(short, long)]
        follow: bool,

        /// Show last N lines
        #[arg(short = 'n', long, default_value = "100")]
        tail: usize,

        /// Include timestamps
        #[arg(long)]
        timestamps: bool,
    },

    /// Delete a task
    #[command(alias = "rm")]
    Delete {
        /// Task ID to delete
        task_id: String,

        /// Force delete without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Retry a failed task item
    Retry {
        /// Task item ID to retry
        task_item_id: String,

        /// Enqueue task for background worker instead of running inline
        #[arg(long)]
        detach: bool,
    },

    /// Edit a task execution plan by inserting a step before another step
    Edit {
        /// Task ID
        task_id: String,

        /// Insert before this existing step ID
        #[arg(long = "insert-before")]
        insert_before: String,

        /// Step type to insert (init_once|plan|qa|ticket_scan|fix|retest|loop_guard)
        #[arg(long)]
        step: String,

        /// Optional required capability for the inserted step
        #[arg(long)]
        capability: Option<String>,

        /// Enable interactive tty for this step
        #[arg(long)]
        tty: bool,

        /// Whether the inserted step is repeatable
        #[arg(long)]
        repeatable: bool,
    },

    /// Worker control commands
    #[command(subcommand)]
    Worker(TaskWorkerCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub enum TaskWorkerCommands {
    /// Start scheduler worker loop
    Start {
        /// Polling interval in milliseconds
        #[arg(long, default_value = "1000")]
        poll_ms: u64,

        /// Number of concurrent worker consumers
        #[arg(long, default_value = "1")]
        workers: usize,
    },
    /// Signal worker to stop
    Stop,
    /// Show worker-related queue status
    Status,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkspaceCommands {
    #[command(alias = "ls")]
    List {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    #[command(alias = "get")]
    Info {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "root-path")]
        root_path: String,

        #[arg(long = "qa-target")]
        qa_target: Vec<String>,

        #[arg(long = "ticket-dir", default_value = "docs/ticket")]
        ticket_dir: String,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum AgentCommands {
    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "template-init-once")]
        template_init_once: Option<String>,

        #[arg(long = "template-qa")]
        template_qa: Option<String>,

        #[arg(long = "template-plan")]
        template_plan: Option<String>,

        #[arg(long = "template-fix")]
        template_fix: Option<String>,

        #[arg(long = "template-retest")]
        template_retest: Option<String>,

        #[arg(long = "template-loop-guard")]
        template_loop_guard: Option<String>,

        #[arg(long = "capability")]
        capability: Vec<String>,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkflowCommands {
    Create {
        #[arg(value_name = "NAME")]
        name: String,

        #[arg(long = "step", required = true)]
        step: Vec<String>,

        #[arg(long = "loop-mode", default_value = "once")]
        loop_mode: String,

        #[arg(long = "max-cycles")]
        max_cycles: Option<u32>,

        #[arg(long = "label")]
        labels: Vec<String>,

        #[arg(long = "annotation")]
        annotations: Vec<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum EditCommands {
    #[command(alias = "ex")]
    Export {
        #[arg(value_name = "RESOURCE")]
        selector: String,
    },

    #[command(alias = "op")]
    Open {
        #[arg(value_name = "RESOURCE")]
        selector: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManifestCommands {
    Validate {
        #[arg(short = 'f', long = "file")]
        file: String,
    },

    Export {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        #[arg(short = 'f', long = "file")]
        file: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DbCommands {
    Reset {
        #[arg(short, long)]
        force: bool,

        /// Also clear config version history (preserves current active config)
        #[arg(long)]
        include_history: bool,

        /// Delete ALL config versions (full reset for test isolation)
        #[arg(long)]
        include_config: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum QaCommands {
    #[command(subcommand)]
    Project(QaProjectCommands),

    /// Validate QA concurrency guardrails and sqlite settings
    Doctor {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum QaProjectCommands {
    /// Create or update an isolated QA project scaffold
    Create {
        #[arg(value_name = "PROJECT_ID")]
        project_id: String,

        #[arg(long, default_value = "default")]
        from_workspace: String,

        #[arg(long)]
        workflow: Option<String>,

        #[arg(long)]
        workspace: Option<String>,

        #[arg(long)]
        root_path: Option<String>,

        #[arg(long = "qa-target")]
        qa_target: Vec<String>,

        #[arg(long, default_value = "docs/ticket")]
        ticket_dir: String,

        #[arg(short, long)]
        force: bool,
    },

    /// Reset one QA project data/resources without deleting the sqlite database
    Reset {
        #[arg(value_name = "PROJECT_ID")]
        project_id: String,

        /// Keep project config and only clean task/runtime rows
        #[arg(long)]
        keep_config: bool,

        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CompletionCommands {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

pub fn generate_completion(shell: Shell) {
    let mut app = Cli::command();
    clap_complete::generate(shell, &mut app, "orchestrator", &mut std::io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_apply_file_and_dry_run_flags() {
        let cli = Cli::parse_from(["orchestrator", "apply", "-f", "resources.yaml", "--dry-run"]);

        match cli.command {
            Commands::Apply { file, dry_run } => {
                assert_eq!(file, "resources.yaml");
                assert!(dry_run);
            }
            _ => panic!("expected apply command"),
        }
    }

    #[test]
    fn parse_apply_defaults_dry_run_to_false() {
        let cli = Cli::parse_from(["orchestrator", "apply", "-f", "resources.yaml"]);

        match cli.command {
            Commands::Apply { file, dry_run } => {
                assert_eq!(file, "resources.yaml");
                assert!(!dry_run);
            }
            _ => panic!("expected apply command"),
        }
    }

    #[test]
    fn parse_edit_export_command() {
        let cli = Cli::parse_from(["orchestrator", "edit", "export", "workspace/default"]);

        match cli.command {
            Commands::Edit(EditCommands::Export { selector }) => {
                assert_eq!(selector, "workspace/default");
            }
            _ => panic!("expected edit export command"),
        }
    }

    #[test]
    fn parse_edit_export_with_agent_selector() {
        let cli = Cli::parse_from(["orchestrator", "edit", "export", "agent/opencode"]);

        match cli.command {
            Commands::Edit(EditCommands::Export { selector }) => {
                assert_eq!(selector, "agent/opencode");
            }
            _ => panic!("expected edit export command"),
        }
    }

    #[test]
    fn parse_edit_open_command() {
        let cli = Cli::parse_from(["orchestrator", "edit", "open", "workspace/default"]);

        match cli.command {
            Commands::Edit(EditCommands::Open { selector }) => {
                assert_eq!(selector, "workspace/default");
            }
            _ => panic!("expected edit open command"),
        }
    }

    #[test]
    fn parse_workspace_info_with_positional_arg() {
        let cli = Cli::parse_from(["orchestrator", "workspace", "info", "new-workspace"]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Info {
                workspace_id,
                output,
            }) => {
                assert_eq!(workspace_id, "new-workspace");
                assert_eq!(output, OutputFormat::Table);
            }
            _ => panic!("expected workspace info command"),
        }
    }

    #[test]
    fn parse_workspace_info_with_output_format() {
        let cli = Cli::parse_from(["orchestrator", "workspace", "info", "my-ws", "-o", "json"]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Info {
                workspace_id,
                output,
            }) => {
                assert_eq!(workspace_id, "my-ws");
                assert_eq!(output, OutputFormat::Json);
            }
            _ => panic!("expected workspace info command"),
        }
    }

    #[test]
    fn parse_workspace_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "workspace",
            "create",
            "new-ws",
            "--root-path",
            "workspace/new",
            "--qa-target",
            "docs/qa",
            "--label",
            "env=dev",
            "--dry-run",
            "-o",
            "json",
        ]);

        match cli.command {
            Commands::Workspace(WorkspaceCommands::Create {
                name,
                root_path,
                qa_target,
                labels,
                dry_run,
                output,
                ..
            }) => {
                assert_eq!(name, "new-ws");
                assert_eq!(root_path, "workspace/new");
                assert_eq!(qa_target, vec!["docs/qa"]);
                assert_eq!(labels, vec!["env=dev"]);
                assert!(dry_run);
                assert_eq!(output, OutputFormat::Json);
            }
            _ => panic!("expected workspace create command"),
        }
    }

    #[test]
    fn parse_agent_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "agent",
            "create",
            "qa-agent",
            "--template-qa",
            "echo qa",
            "--capability",
            "qa",
        ]);

        match cli.command {
            Commands::Agent(AgentCommands::Create {
                name,
                template_qa,
                capability,
                ..
            }) => {
                assert_eq!(name, "qa-agent");
                assert_eq!(template_qa, Some("echo qa".to_string()));
                assert_eq!(capability, vec!["qa"]);
            }
            _ => panic!("expected agent create command"),
        }
    }

    #[test]
    fn parse_task_worker_start_workers_flag() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "worker",
            "start",
            "--poll-ms",
            "250",
            "--workers",
            "6",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start {
                poll_ms,
                workers,
            })) => {
                assert_eq!(poll_ms, 250);
                assert_eq!(workers, 6);
            }
            _ => panic!("expected task worker start command"),
        }
    }

    #[test]
    fn parse_task_worker_start_workers_default() {
        let cli = Cli::parse_from(["orchestrator", "task", "worker", "start"]);

        match cli.command {
            Commands::Task(TaskCommands::Worker(TaskWorkerCommands::Start {
                poll_ms,
                workers,
            })) => {
                assert_eq!(poll_ms, 1000);
                assert_eq!(workers, 1);
            }
            _ => panic!("expected task worker start command"),
        }
    }

    #[test]
    fn parse_workflow_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "workflow",
            "create",
            "qa-flow",
            "--step",
            "qa",
            "--step",
            "fix",
            "--loop-mode",
            "infinite",
            "--max-cycles",
            "5",
        ]);

        match cli.command {
            Commands::Workflow(WorkflowCommands::Create {
                name,
                step,
                loop_mode,
                max_cycles,
                ..
            }) => {
                assert_eq!(name, "qa-flow");
                assert_eq!(step, vec!["qa", "fix"]);
                assert_eq!(loop_mode, "infinite");
                assert_eq!(max_cycles, Some(5));
            }
            _ => panic!("expected workflow create command"),
        }
    }

    #[test]
    fn parse_init_command() {
        let cli = Cli::parse_from(["orchestrator", "init"]);

        match cli.command {
            Commands::Init { root, force } => {
                assert_eq!(root, None);
                assert!(!force);
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn parse_init_command_with_options() {
        let cli = Cli::parse_from(["orchestrator", "init", "--root", "/tmp/test", "--force"]);

        match cli.command {
            Commands::Init { root, force } => {
                assert_eq!(root, Some("/tmp/test".to_string()));
                assert!(force);
            }
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn parse_get_command() {
        let cli = Cli::parse_from(["orchestrator", "get", "workspace/default"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "workspace/default");
                assert_eq!(output, OutputFormat::Table);
                assert_eq!(selector, None);
            }
            _ => panic!("expected get command"),
        }
    }

    #[test]
    fn parse_get_command_yaml() {
        let cli = Cli::parse_from(["orchestrator", "get", "agent/echo", "-o", "yaml"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "agent/echo");
                assert_eq!(output, OutputFormat::Yaml);
                assert_eq!(selector, None);
            }
            _ => panic!("expected get command"),
        }
    }

    #[test]
    fn parse_get_list_with_selector() {
        let cli = Cli::parse_from(["orchestrator", "get", "workspaces", "-l", "env=prod"]);

        match cli.command {
            Commands::Get {
                resource,
                output,
                selector,
            } => {
                assert_eq!(resource, "workspaces");
                assert_eq!(output, OutputFormat::Table);
                assert_eq!(selector, Some("env=prod".to_string()));
            }
            _ => panic!("expected get command"),
        }
    }

    #[test]
    fn parse_describe_command() {
        let cli = Cli::parse_from(["orchestrator", "describe", "workflow/basic"]);

        match cli.command {
            Commands::Describe { resource, output } => {
                assert_eq!(resource, "workflow/basic");
                assert_eq!(output, OutputFormat::Yaml);
            }
            _ => panic!("expected describe command"),
        }
    }

    #[test]
    fn parse_delete_command() {
        let cli = Cli::parse_from(["orchestrator", "delete", "workspace/old-ws"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "workspace/old-ws");
                assert!(!force);
            }
            _ => panic!("expected delete command"),
        }
    }

    #[test]
    fn parse_delete_force() {
        let cli = Cli::parse_from(["orchestrator", "delete", "agent/old", "--force"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "agent/old");
                assert!(force);
            }
            _ => panic!("expected delete command"),
        }
    }

    #[test]
    fn parse_delete_alias_rm() {
        let cli = Cli::parse_from(["orchestrator", "rm", "workflow/old-wf", "-f"]);

        match cli.command {
            Commands::Delete { resource, force } => {
                assert_eq!(resource, "workflow/old-wf");
                assert!(force);
            }
            _ => panic!("expected delete command via rm alias"),
        }
    }

    #[test]
    fn parse_db_command() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(!force);
                assert!(!include_history);
                assert!(!include_config);
            }
            _ => panic!("expected db reset command"),
        }
    }

    #[test]
    fn parse_db_reset_force() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset", "--force"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(!include_history);
                assert!(!include_config);
            }
            _ => panic!("expected db reset command"),
        }
    }

    #[test]
    fn parse_db_reset_include_history() {
        let cli = Cli::parse_from([
            "orchestrator",
            "db",
            "reset",
            "--force",
            "--include-history",
        ]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(include_history);
                assert!(!include_config);
            }
            _ => panic!("expected db reset command"),
        }
    }

    #[test]
    fn parse_db_reset_include_config() {
        let cli = Cli::parse_from(["orchestrator", "db", "reset", "--force", "--include-config"]);

        match cli.command {
            Commands::Db(DbCommands::Reset {
                force,
                include_history,
                include_config,
            }) => {
                assert!(force);
                assert!(!include_history);
                assert!(include_config);
            }
            _ => panic!("expected db reset command"),
        }
    }

    #[test]
    fn parse_completion_command() {
        let cli = Cli::parse_from(["orchestrator", "completion", "bash"]);

        match cli.command {
            Commands::Completion(CompletionCommands::Bash) => {}
            _ => panic!("expected completion bash command"),
        }
    }

    #[test]
    fn parse_qa_project_create_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "qa",
            "project",
            "create",
            "qa-run-1",
            "--workspace",
            "ws-a",
            "--workflow",
            "qa_only",
            "--force",
        ]);

        match cli.command {
            Commands::Qa(QaCommands::Project(QaProjectCommands::Create {
                project_id,
                workspace,
                workflow,
                force,
                ..
            })) => {
                assert_eq!(project_id, "qa-run-1");
                assert_eq!(workspace, Some("ws-a".to_string()));
                assert_eq!(workflow, Some("qa_only".to_string()));
                assert!(force);
            }
            _ => panic!("expected qa project create command"),
        }
    }

    #[test]
    fn parse_qa_project_reset_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "qa",
            "project",
            "reset",
            "qa-run-1",
            "--keep-config",
            "--force",
        ]);

        match cli.command {
            Commands::Qa(QaCommands::Project(QaProjectCommands::Reset {
                project_id,
                keep_config,
                force,
            })) => {
                assert_eq!(project_id, "qa-run-1");
                assert!(keep_config);
                assert!(force);
            }
            _ => panic!("expected qa project reset command"),
        }
    }

    #[test]
    fn parse_qa_doctor_command() {
        let cli = Cli::parse_from(["orchestrator", "qa", "doctor", "-o", "json"]);

        match cli.command {
            Commands::Qa(QaCommands::Doctor { output }) => {
                assert_eq!(output, OutputFormat::Json);
            }
            _ => panic!("expected qa doctor command"),
        }
    }

    #[test]
    fn parse_task_info_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "info", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Info { task_id, output }) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(output, OutputFormat::Table);
            }
            _ => panic!("expected task info command"),
        }
    }

    #[test]
    fn parse_task_start_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "start", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Start {
                task_id, latest, ..
            }) => {
                assert_eq!(task_id, Some("task-123".to_string()));
                assert!(!latest);
            }
            _ => panic!("expected task start command"),
        }
    }

    #[test]
    fn parse_task_start_latest() {
        let cli = Cli::parse_from(["orchestrator", "task", "start", "--latest"]);

        match cli.command {
            Commands::Task(TaskCommands::Start {
                task_id, latest, ..
            }) => {
                assert_eq!(task_id, None);
                assert!(latest);
            }
            _ => panic!("expected task start command"),
        }
    }

    #[test]
    fn parse_task_create_with_project_flag() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "create",
            "--project",
            "default",
            "--name",
            "test",
            "--goal",
            "goal",
            "--no-start",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Create {
                project,
                name,
                goal,
                no_start,
                ..
            }) => {
                assert_eq!(project, Some("default".to_string()));
                assert_eq!(name, Some("test".to_string()));
                assert_eq!(goal, Some("goal".to_string()));
                assert!(no_start);
            }
            _ => panic!("expected task create command"),
        }
    }

    #[test]
    fn parse_task_list_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "list"]);

        match cli.command {
            Commands::Task(TaskCommands::List {
                status,
                output,
                verbose,
            }) => {
                assert_eq!(status, None);
                assert_eq!(output, OutputFormat::Table);
                assert!(!verbose);
            }
            _ => panic!("expected task list command"),
        }
    }

    #[test]
    fn parse_task_list_with_options() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "list",
            "--status",
            "running",
            "-o",
            "json",
            "-v",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::List {
                status,
                output,
                verbose,
            }) => {
                assert_eq!(status, Some("running".to_string()));
                assert_eq!(output, OutputFormat::Json);
                assert!(verbose);
            }
            _ => panic!("expected task list command"),
        }
    }

    #[test]
    fn parse_task_delete_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "delete", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Delete { task_id, force }) => {
                assert_eq!(task_id, "task-123");
                assert!(!force);
            }
            _ => panic!("expected task delete command"),
        }
    }

    #[test]
    fn parse_task_delete_force() {
        let cli = Cli::parse_from(["orchestrator", "task", "delete", "task-123", "--force"]);

        match cli.command {
            Commands::Task(TaskCommands::Delete { task_id, force }) => {
                assert_eq!(task_id, "task-123");
                assert!(force);
            }
            _ => panic!("expected task delete command"),
        }
    }

    #[test]
    fn parse_task_retry_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "retry", "item-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Retry { task_item_id, .. }) => {
                assert_eq!(task_item_id, "item-123");
            }
            _ => panic!("expected task retry command"),
        }
    }

    #[test]
    fn parse_task_pause_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "pause", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Pause { task_id }) => {
                assert_eq!(task_id, "task-123");
            }
            _ => panic!("expected task pause command"),
        }
    }

    #[test]
    fn parse_task_resume_command() {
        let cli = Cli::parse_from(["orchestrator", "task", "resume", "task-123"]);

        match cli.command {
            Commands::Task(TaskCommands::Resume { task_id, .. }) => {
                assert_eq!(task_id, "task-123");
            }
            _ => panic!("expected task resume command"),
        }
    }

    #[test]
    fn parse_task_edit_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "task",
            "edit",
            "task-123",
            "--insert-before",
            "qa",
            "--step",
            "plan",
            "--tty",
        ]);

        match cli.command {
            Commands::Task(TaskCommands::Edit {
                task_id,
                insert_before,
                step,
                tty,
                repeatable,
                ..
            }) => {
                assert_eq!(task_id, "task-123");
                assert_eq!(insert_before, "qa");
                assert_eq!(step, "plan");
                assert!(tty);
                assert!(!repeatable);
            }
            _ => panic!("expected task edit command"),
        }
    }

    #[test]
    fn parse_exec_interactive_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "exec",
            "-it",
            "task/task-123/step/plan-1",
            "--",
            "echo",
            "hello",
        ]);

        match cli.command {
            Commands::Exec {
                stdin,
                tty,
                target,
                command,
            } => {
                assert!(stdin);
                assert!(tty);
                assert_eq!(target, "task/task-123/step/plan-1");
                assert_eq!(command, vec!["echo".to_string(), "hello".to_string()]);
            }
            _ => panic!("expected exec command"),
        }
    }

    #[test]
    fn parse_manifest_export_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "manifest",
            "export",
            "-o",
            "json",
            "-f",
            "/tmp/out.json",
        ]);

        match cli.command {
            Commands::Manifest(ManifestCommands::Export { output, file }) => {
                assert_eq!(output, OutputFormat::Json);
                assert_eq!(file, Some("/tmp/out.json".to_string()));
            }
            _ => panic!("expected manifest export command"),
        }
    }

    #[test]
    fn parse_manifest_validate_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "manifest",
            "validate",
            "-f",
            "/tmp/input.yaml",
        ]);

        match cli.command {
            Commands::Manifest(ManifestCommands::Validate { file }) => {
                assert_eq!(file, "/tmp/input.yaml");
            }
            _ => panic!("expected manifest validate command"),
        }
    }
}

pub fn run_cli_mode(state: Arc<InnerState>, cli: Cli) -> Result<()> {
    let handler = CliHandler::new(state);
    let exit_code = handler.execute(&cli)?;
    std::process::exit(exit_code);
}
