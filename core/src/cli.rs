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

    /// Optional seed config path for bootstrap-compatible workflows
    #[arg(short, long, global = true)]
    pub config: Option<String>,

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

    #[command(alias = "cfg", alias = "c", subcommand)]
    Config(ConfigCommands),

    #[command(alias = "e", subcommand)]
    Edit(EditCommands),

    #[command(subcommand)]
    Db(DbCommands),

    #[command(alias = "comp", subcommand)]
    Completion(CompletionCommands),

    #[command(alias = "dbg")]
    Debug {
        #[arg(long)]
        component: Option<String>,
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
        #[arg(short, long, default_value = "100")]
        tail: usize,

        /// Include timestamps
        #[arg(short, long)]
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
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkspaceCommands {
    #[command(alias = "ls")]
    List {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    #[command(alias = "get")]
    Info {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,

        #[arg(short, long, default_value = "table")]
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
pub enum ConfigCommands {
    #[command(alias = "get")]
    View {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },

    Set {
        config_file: String,
    },

    Bootstrap {
        #[arg(long = "from")]
        from_file: String,

        #[arg(short, long)]
        force: bool,
    },

    Export {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        #[arg(short = 'f', long = "file")]
        file: Option<String>,
    },

    Validate {
        config_file: String,
    },

    #[command(alias = "lw", alias = "list-wf")]
    ListWorkflows {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    #[command(alias = "la", alias = "list-agent")]
    ListAgents {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
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
            Commands::Get { resource, output } => {
                assert_eq!(resource, "workspace/default");
                assert_eq!(output, OutputFormat::Table);
            }
            _ => panic!("expected get command"),
        }
    }

    #[test]
    fn parse_get_command_yaml() {
        let cli = Cli::parse_from(["orchestrator", "get", "agent/echo", "-o", "yaml"]);

        match cli.command {
            Commands::Get { resource, output } => {
                assert_eq!(resource, "agent/echo");
                assert_eq!(output, OutputFormat::Yaml);
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
            }) => {
                assert!(!force);
                assert!(!include_history);
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
            }) => {
                assert!(force);
                assert!(!include_history);
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
            }) => {
                assert!(force);
                assert!(include_history);
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
            Commands::Task(TaskCommands::Start { task_id, latest }) => {
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
            Commands::Task(TaskCommands::Start { task_id, latest }) => {
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
            Commands::Task(TaskCommands::Retry { task_item_id }) => {
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
            Commands::Task(TaskCommands::Resume { task_id }) => {
                assert_eq!(task_id, "task-123");
            }
            _ => panic!("expected task resume command"),
        }
    }

    #[test]
    fn parse_config_view_command() {
        let cli = Cli::parse_from(["orchestrator", "config", "view"]);

        match cli.command {
            Commands::Config(ConfigCommands::View { output }) => {
                assert_eq!(output, OutputFormat::Yaml);
            }
            _ => panic!("expected config view command"),
        }
    }

    #[test]
    fn parse_config_view_json() {
        let cli = Cli::parse_from(["orchestrator", "config", "view", "-o", "json"]);

        match cli.command {
            Commands::Config(ConfigCommands::View { output }) => {
                assert_eq!(output, OutputFormat::Json);
            }
            _ => panic!("expected config view command"),
        }
    }

    #[test]
    fn parse_config_validate_command() {
        let cli = Cli::parse_from(["orchestrator", "config", "validate", "/path/to/config.yaml"]);

        match cli.command {
            Commands::Config(ConfigCommands::Validate { config_file }) => {
                assert_eq!(config_file, "/path/to/config.yaml");
            }
            _ => panic!("expected config validate command"),
        }
    }

    #[test]
    fn parse_config_bootstrap_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "config",
            "bootstrap",
            "--from",
            "/tmp/config.yaml",
            "--force",
        ]);

        match cli.command {
            Commands::Config(ConfigCommands::Bootstrap { from_file, force }) => {
                assert_eq!(from_file, "/tmp/config.yaml");
                assert!(force);
            }
            _ => panic!("expected config bootstrap command"),
        }
    }

    #[test]
    fn parse_config_export_command() {
        let cli = Cli::parse_from([
            "orchestrator",
            "config",
            "export",
            "-o",
            "json",
            "-f",
            "/tmp/out.json",
        ]);

        match cli.command {
            Commands::Config(ConfigCommands::Export { output, file }) => {
                assert_eq!(output, OutputFormat::Json);
                assert_eq!(file, Some("/tmp/out.json".to_string()));
            }
            _ => panic!("expected config export command"),
        }
    }

    #[test]
    fn parse_config_list_workflows_command() {
        let cli = Cli::parse_from(["orchestrator", "config", "list-workflows"]);

        match cli.command {
            Commands::Config(ConfigCommands::ListWorkflows { output }) => {
                assert_eq!(output, OutputFormat::Table);
            }
            _ => panic!("expected config list-workflows command"),
        }
    }

    #[test]
    fn parse_config_list_agents_command() {
        let cli = Cli::parse_from(["orchestrator", "config", "list-agents", "-o", "json"]);

        match cli.command {
            Commands::Config(ConfigCommands::ListAgents { output }) => {
                assert_eq!(output, OutputFormat::Json);
            }
            _ => panic!("expected config list-agents command"),
        }
    }
}

pub fn run_cli_mode(state: Arc<InnerState>, cli: Cli) -> Result<()> {
    let handler = CliHandler::new(state);
    let exit_code = handler.execute(&cli)?;
    std::process::exit(exit_code);
}
