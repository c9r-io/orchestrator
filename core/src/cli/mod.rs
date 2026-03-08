mod task;
mod resource;
mod system;
mod qa;
mod store;
mod tests;

pub use task::{TaskCommands, TaskWorkerCommands, TaskSessionCommands};
pub use resource::{WorkspaceCommands, AgentCommands, WorkflowCommands, EditCommands, ManifestCommands};
pub use system::{DbCommands, CompletionCommands, VerifyCommands, ConfigLifecycleCommands};
pub use qa::{QaCommands, QaProjectCommands};
pub use store::StoreCommands;

use crate::cli_handler::CliHandler;
use crate::state::InnerState;
use agent_orchestrator::config::{LogLevel, LoggingFormat};
use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::sync::Arc;

/// Agent Orchestrator CLI - kubectl-like command-line interface
#[derive(Parser, Debug)]
#[command(
    name = "orchestrator",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("BUILD_GIT_HASH"), ")"),
    about = "Agent Orchestrator - workflow automation CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Override structured log level
    #[arg(long, global = true)]
    pub log_level: Option<CliLogLevel>,

    /// Override structured console log format
    #[arg(long, global = true)]
    pub log_format: Option<CliLogFormat>,

    /// Bypass all --force gates and override runner policy to Unsafe (power-user escape hatch)
    #[arg(long = "unsafe", global = true)]
    pub unsafe_mode: bool,
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

        /// Deploy Agent/Workflow/Workspace resources into a project scope
        #[arg(long)]
        project: Option<String>,
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

        /// Target selector: task/<task_id>/step/<step_id> or session/<session_id>
        target: String,

        /// Command to execute in the selected step context
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },

    #[command(subcommand)]
    Verify(VerifyCommands),

    /// Config lifecycle operations (heal-log, backfill)
    #[command(alias = "cfg", subcommand)]
    Config(ConfigLifecycleCommands),

    /// Preflight validation: cross-reference checks on config, agents, workflows
    #[command(alias = "ck")]
    Check {
        /// Only check this workflow
        #[arg(long)]
        workflow: Option<String>,
        /// Output format
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Persistent store operations (cross-task workflow memory)
    #[command(subcommand)]
    Store(StoreCommands),

    /// Show detailed build version information
    Version {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<CliLogLevel> for LogLevel {
    fn from(value: CliLogLevel) -> Self {
        match value {
            CliLogLevel::Error => LogLevel::Error,
            CliLogLevel::Warn => LogLevel::Warn,
            CliLogLevel::Info => LogLevel::Info,
            CliLogLevel::Debug => LogLevel::Debug,
            CliLogLevel::Trace => LogLevel::Trace,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CliLogFormat {
    Pretty,
    Json,
}

impl From<CliLogFormat> for LoggingFormat {
    fn from(value: CliLogFormat) -> Self {
        match value {
            CliLogFormat::Pretty => LoggingFormat::Pretty,
            CliLogFormat::Json => LoggingFormat::Json,
        }
    }
}

pub fn generate_completion(shell: Shell) {
    let mut app = Cli::command();
    clap_complete::generate(shell, &mut app, "orchestrator", &mut std::io::stdout());
}

pub fn run_cli_mode(state: Arc<InnerState>, cli: Cli) -> Result<()> {
    let handler = CliHandler::new(state);
    let exit_code = handler.execute(&cli)?;
    std::process::exit(exit_code);
}
