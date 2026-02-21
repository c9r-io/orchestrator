use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::{Deserialize, Serialize};

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

    /// Path to config file (default: ./config/default.yaml)
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Initialize orchestrator with a default configuration
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

    #[command(alias = "serve")]
    Daemon,

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

        /// Workspace ID to use
        #[arg(short, long)]
        workspace: Option<String>,

        /// Workflow ID to use
        #[arg(short, long)]
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

/// Legacy CLI options for backward compatibility
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct LegacyCliOptions {
    pub cli: bool,
    pub show_help: bool,
    pub no_auto_resume: bool,
    pub task_id: Option<String>,
    pub workspace_id: Option<String>,
    pub workflow_id: Option<String>,
    pub name: Option<String>,
    pub goal: Option<String>,
    pub target_files: Vec<String>,
}

impl From<&Cli> for LegacyCliOptions {
    fn from(cli: &Cli) -> Self {
        let mut target_files = Vec::new();

        if let Commands::Task(TaskCommands::Create { target_file, .. }) = &cli.command {
            target_files = target_file.clone();
        }

        LegacyCliOptions {
            cli: true,
            show_help: false,
            no_auto_resume: false,
            task_id: None,
            workspace_id: None,
            workflow_id: None,
            name: None,
            goal: None,
            target_files,
        }
    }
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
}
