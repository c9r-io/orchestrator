mod client;
mod commands;
mod output;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

/// Agent Orchestrator CLI — lightweight gRPC client
#[derive(Parser, Debug)]
#[command(
    name = "orchestrator",
    version,
    about = "Agent Orchestrator — workflow automation CLI"
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
    /// Apply resource manifests
    #[command(alias = "ap")]
    Apply {
        #[arg(short = 'f', long = "file")]
        file: String,

        #[arg(long)]
        dry_run: bool,

        #[arg(long)]
        project: Option<String>,
    },

    /// Get resource(s)
    #[command(alias = "g")]
    Get {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        #[arg(short = 'l', long = "selector")]
        selector: Option<String>,

        #[arg(short, long)]
        project: Option<String>,
    },

    /// Describe a resource
    #[command(alias = "desc")]
    Describe {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        #[arg(long)]
        project: Option<String>,
    },

    /// Delete a resource
    #[command(alias = "rm")]
    Delete {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(short, long)]
        force: bool,

        #[arg(long)]
        project: Option<String>,
    },

    /// Task operations
    #[command(alias = "t", subcommand)]
    Task(TaskCommands),

    /// Store operations
    #[command(subcommand)]
    Store(StoreCommands),

    /// System debug info
    #[command(alias = "dbg")]
    Debug {
        #[arg(long)]
        component: Option<String>,
    },

    /// Preflight check
    #[command(alias = "ck")]
    Check {
        #[arg(long)]
        workflow: Option<String>,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Initialize orchestrator runtime
    Init {
        /// Workspace root directory to create
        root: Option<String>,
    },

    /// Database operations
    #[command(subcommand)]
    Db(DbCommands),

    /// Manifest operations
    #[command(subcommand)]
    Manifest(ManifestCommands),

    /// Project management
    #[command(alias = "proj", subcommand)]
    Project(ProjectCommands),

    /// Show version
    Version,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DbCommands {
    /// Reset the database
    Reset {
        #[arg(long)]
        force: bool,

        #[arg(long)]
        include_history: bool,

        #[arg(long)]
        include_config: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManifestCommands {
    /// Validate a manifest file
    Validate {
        #[arg(short = 'f', long = "file")]
        file: String,
    },

    /// Export all resources as manifest documents
    Export {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TaskCommands {
    /// List tasks
    #[command(alias = "ls")]
    List {
        #[arg(short, long)]
        status: Option<String>,

        #[arg(short, long)]
        project: Option<String>,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        #[arg(short, long)]
        verbose: bool,
    },

    /// Create a new task
    #[command(alias = "new")]
    Create {
        #[arg(short, long)]
        name: Option<String>,

        #[arg(short, long)]
        goal: Option<String>,

        #[arg(short, long)]
        project: Option<String>,

        #[arg(short, long)]
        workspace: Option<String>,

        #[arg(short = 'W', long)]
        workflow: Option<String>,

        #[arg(short, long)]
        target_file: Vec<String>,

        #[arg(long)]
        no_start: bool,

        #[arg(long, default_value_t = true)]
        detach: bool,

        #[arg(long, conflicts_with = "detach")]
        attach: bool,
    },

    /// Get task details
    #[command(alias = "get")]
    Info {
        task_id: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Start a task
    Start {
        task_id: Option<String>,

        #[arg(long, short)]
        latest: bool,

        #[arg(long)]
        detach: bool,
    },

    /// Pause a running task
    Pause { task_id: String },

    /// Resume a paused task
    Resume {
        task_id: String,

        #[arg(long)]
        detach: bool,
    },

    /// View task logs
    #[command(alias = "log")]
    Logs {
        task_id: String,

        #[arg(short, long)]
        follow: bool,

        #[arg(short = 'n', long, default_value = "100")]
        tail: usize,

        #[arg(long)]
        timestamps: bool,
    },

    /// Delete a task
    #[command(alias = "rm")]
    Delete {
        task_id: String,

        #[arg(short, long)]
        force: bool,
    },

    /// Retry a failed task item
    Retry {
        task_item_id: String,

        #[arg(long)]
        detach: bool,

        #[arg(short, long)]
        force: bool,
    },

    /// Watch task status (streaming updates)
    Watch {
        task_id: String,

        /// Polling interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },

    /// Show task event trace
    Trace {
        task_id: String,

        /// Show all events (not just key lifecycle events)
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum StoreCommands {
    Get {
        store: String,
        key: String,
        #[arg(short, long, default_value = "")]
        project: String,
    },
    Put {
        store: String,
        key: String,
        value: String,
        #[arg(short, long, default_value = "")]
        project: String,
        #[arg(short, long, default_value = "")]
        task_id: String,
    },
    Delete {
        store: String,
        key: String,
        #[arg(short, long, default_value = "")]
        project: String,
    },
    #[command(alias = "ls")]
    List {
        store: String,
        #[arg(short, long, default_value = "")]
        project: String,
        #[arg(short, long, default_value = "100")]
        limit: u64,
        #[arg(long, default_value = "0")]
        offset: u64,
        #[arg(short = 'o', long, default_value = "table")]
        output: OutputFormat,
    },
    Prune {
        store: String,
        #[arg(short, long, default_value = "")]
        project: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProjectCommands {
    /// Reset a project's task data (tasks, items, runs, events)
    Reset {
        /// Project ID to reset
        project_id: String,

        /// Confirm the reset
        #[arg(short, long)]
        force: bool,

        /// Also remove project entry from configuration
        #[arg(long)]
        include_config: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    rt.block_on(async move {
        match cli.command {
            Commands::Version => commands::version::run().await,
            _ => {
                let mut client = client::connect().await?;
                commands::dispatch(&mut client, cli.command).await
            }
        }
    })
}
