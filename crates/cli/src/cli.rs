use clap::{Parser, Subcommand, ValueEnum};

/// Agent Orchestrator CLI — lightweight gRPC client
#[derive(Parser, Debug)]
#[command(
    name = "orchestrator",
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("BUILD_GIT_HASH"), ")"),
    about = "Agent Orchestrator — workflow automation CLI"
)]
pub struct Cli {
    /// Override the control-plane client config file
    #[arg(long, global = true, env = "ORCHESTRATOR_CONTROL_PLANE_CONFIG")]
    pub control_plane_config: Option<String>,

    /// Subcommand selected for this invocation.
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// Top-level subcommands supported by the `orchestrator` CLI.
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Apply resource manifests
    #[command(alias = "ap")]
    Apply {
        /// Manifest file path.
        #[arg(short = 'f', long = "file")]
        file: String,

        /// Validate and render without persisting.
        #[arg(long)]
        dry_run: bool,

        /// Delete previously managed resources not present in the manifest.
        #[arg(long)]
        prune: bool,

        /// Project override for project-scoped resources.
        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Get resource(s)
    #[command(alias = "g")]
    Get {
        /// Resource kind selector.
        #[arg(value_name = "RESOURCE")]
        resource: String,

        /// Optional resource name.
        #[arg(value_name = "NAME")]
        name: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Label selector expression.
        #[arg(short = 'l', long = "selector")]
        selector: Option<String>,

        /// Project override for project-scoped resources.
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Describe a resource
    #[command(alias = "desc")]
    Describe {
        /// Resource kind selector.
        #[arg(value_name = "RESOURCE")]
        resource: String,

        /// Optional resource name.
        #[arg(value_name = "NAME")]
        name: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        /// Project override for project-scoped resources.
        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Delete a resource
    #[command(alias = "rm")]
    Delete {
        /// Resource kind selector.
        #[arg(value_name = "RESOURCE")]
        resource: String,

        /// Optional resource name.
        #[arg(value_name = "NAME")]
        name: Option<String>,

        /// Skip interactive confirmation.
        #[arg(short, long)]
        force: bool,

        /// Validate and render without deleting.
        #[arg(long)]
        dry_run: bool,

        /// Project override for project-scoped resources.
        #[arg(short = 'p', long)]
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
        /// Optional component filter.
        #[arg(long)]
        component: Option<String>,

        /// Optional nested debug command.
        #[command(subcommand)]
        command: Option<DebugCommands>,
    },

    /// Preflight check
    #[command(alias = "ck")]
    Check {
        /// Optional workflow filter.
        #[arg(long)]
        workflow: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Project override.
        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Initialize orchestrator runtime
    Init {
        /// Optional runtime root path to initialize.
        root: Option<String>,
    },

    /// Secret key management
    #[command(subcommand)]
    Secret(SecretCommands),

    /// Database operations
    #[command(subcommand)]
    Db(DbCommands),

    /// Manifest operations
    #[command(subcommand)]
    Manifest(ManifestCommands),

    /// Agent lifecycle operations (cordon, drain, uncordon)
    #[command(alias = "ag", subcommand)]
    Agent(AgentCommands),

    /// Event lifecycle operations (cleanup, stats)
    #[command(alias = "ev", subcommand)]
    Event(EventCommands),

    /// Trigger lifecycle operations (suspend, resume, fire)
    #[command(alias = "tg", subcommand)]
    Trigger(TriggerCommands),

    /// Daemon lifecycle operations (stop, status)
    #[command(subcommand)]
    Daemon(DaemonCommands),

    /// Show version
    Version {
        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
}

/// Daemon lifecycle commands.
#[derive(Subcommand, Debug, Clone)]
pub enum DaemonCommands {
    /// Stop the running daemon by sending SIGTERM
    Stop,
    /// Show whether the daemon is running and its PID
    Status,
    /// Enable or disable maintenance mode (blocks new task creation)
    Maintenance {
        /// Enable maintenance mode
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// Disable maintenance mode
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, DbCommands, DbMigrationCommands, EventCommands};
    use clap::Parser;

    #[test]
    fn version_subcommand_accepts_json_flag() {
        let cli = Cli::try_parse_from(["orchestrator", "version", "--json"])
            .expect("version --json should parse");
        assert!(matches!(cli.command, Commands::Version { json: true }));
    }

    #[test]
    fn db_status_subcommand_accepts_json_flag() {
        let cli = Cli::try_parse_from(["orchestrator", "db", "status", "--output", "json"])
            .expect("db status should parse");
        assert!(matches!(
            cli.command,
            Commands::Db(DbCommands::Status { .. })
        ));
    }

    #[test]
    fn db_migrations_list_subcommand_parses() {
        let cli = Cli::try_parse_from(["orchestrator", "db", "migrations", "list"])
            .expect("db migrations list should parse");
        assert!(matches!(
            cli.command,
            Commands::Db(DbCommands::Migrations(DbMigrationCommands::List { .. }))
        ));
    }

    #[test]
    fn event_cleanup_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "orchestrator",
            "event",
            "cleanup",
            "--older-than",
            "7",
            "--dry-run",
        ])
        .expect("event cleanup should parse");
        assert!(matches!(
            cli.command,
            Commands::Event(EventCommands::Cleanup {
                older_than_days: 7,
                dry_run: true,
                archive: false,
            })
        ));
    }

    #[test]
    fn event_stats_subcommand_parses() {
        let cli = Cli::try_parse_from(["orchestrator", "event", "stats"])
            .expect("event stats should parse");
        assert!(matches!(cli.command, Commands::Event(EventCommands::Stats)));
    }
}

/// Local-only debugging commands that do not require daemon connectivity.
#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommands {
    /// Run a local sandbox probe without contacting the daemon
    SandboxProbe {
        /// Sandbox probe primitive to execute locally.
        #[command(subcommand)]
        probe: SandboxProbeCommands,
    },

    #[command(hide = true)]
    /// Run a child process that idles for a fixed duration.
    ChildIdle {
        /// Number of seconds to sleep before exiting.
        #[arg(long, default_value = "60")]
        sleep_secs: u64,
    },
}

/// Sandbox probe primitives used to validate resource and network limits.
#[derive(Subcommand, Debug, Clone)]
pub enum SandboxProbeCommands {
    /// Write a file to a target path.
    WriteFile {
        /// Path to write.
        #[arg(long)]
        path: String,

        /// File contents to write.
        #[arg(long, default_value = "probe")]
        contents: String,
    },
    /// Attempt to open many files at once.
    OpenFiles {
        /// Number of files to open.
        #[arg(long, default_value = "256")]
        count: usize,
    },
    /// Burn CPU in a tight loop.
    CpuBurn,
    /// Allocate memory until the target size is reached.
    AllocMemory {
        /// Chunk size per allocation in MiB.
        #[arg(long, default_value = "8")]
        chunk_mb: usize,

        /// Total target allocation in MiB.
        #[arg(long, default_value = "256")]
        total_mb: usize,
    },
    /// Spawn many child processes.
    SpawnChildren {
        /// Number of children to spawn.
        #[arg(long, default_value = "64")]
        count: usize,

        /// Seconds each child should sleep.
        #[arg(long, default_value = "60")]
        sleep_secs: u64,
    },
    /// Resolve a hostname through DNS.
    DnsResolve {
        /// Hostname to resolve.
        #[arg(long, default_value = "example.com")]
        host: String,

        /// Port number to pair with resolved addresses.
        #[arg(long, default_value = "443")]
        port: u16,
    },
    /// Open a TCP connection to a remote endpoint.
    TcpConnect {
        /// Host to connect to.
        #[arg(long)]
        host: String,

        /// Port to connect to.
        #[arg(long)]
        port: u16,

        /// Connection timeout in seconds.
        #[arg(long, default_value = "3")]
        timeout_secs: u64,
    },
    #[command(hide = true)]
    /// Run a local TCP server for sandbox experiments.
    TcpServe {
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        /// Port to bind.
        #[arg(long)]
        port: u16,

        /// Optional file written when the listener is ready.
        #[arg(long)]
        ready_file: Option<String>,
    },
}

/// Manifest-specific utility commands.
#[derive(Subcommand, Debug, Clone)]
pub enum ManifestCommands {
    /// Validate a manifest file
    Validate {
        /// Manifest file path.
        #[arg(short = 'f', long = "file")]
        file: String,

        /// Project override.
        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Export all resources as manifest documents
    Export {
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

/// Database lifecycle and migration commands.
#[derive(Subcommand, Debug, Clone)]
pub enum DbCommands {
    /// Show schema status for the local database
    Status {
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Database migration operations
    #[command(subcommand)]
    Migrations(DbMigrationCommands),
}

/// Subcommands for inspecting database migration state.
#[derive(Subcommand, Debug, Clone)]
pub enum DbMigrationCommands {
    /// List registered migrations and their applied state
    #[command(alias = "ls")]
    /// List tasks with optional filters.
    List {
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

/// Task management commands exposed by the CLI.
#[derive(Subcommand, Debug, Clone)]
pub enum TaskCommands {
    #[command(alias = "ls")]
    /// List tasks with optional filters.
    List {
        /// Optional status filter.
        #[arg(short, long)]
        status: Option<String>,

        /// Optional project filter.
        #[arg(short, long)]
        project: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        /// Include more detail in the listing output.
        #[arg(short, long)]
        verbose: bool,
    },

    #[command(alias = "new")]
    /// Create a new task.
    Create {
        /// Optional task name.
        #[arg(short, long)]
        name: Option<String>,

        /// Optional task goal description.
        #[arg(short, long)]
        goal: Option<String>,

        /// Optional project identifier.
        #[arg(short, long)]
        project: Option<String>,

        /// Optional workspace identifier.
        #[arg(short, long)]
        workspace: Option<String>,

        /// Optional workflow identifier.
        #[arg(short = 'W', long)]
        workflow: Option<String>,

        /// Explicit target files for the task.
        #[arg(short, long)]
        target_file: Vec<String>,

        /// Create the task without starting it.
        #[arg(long)]
        no_start: bool,
    },

    #[command(alias = "get")]
    /// Show detailed information for one task.
    Info {
        /// Task identifier.
        task_id: String,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Start a task by id or resume the latest task.
    Start {
        /// Optional task identifier.
        task_id: Option<String>,

        /// Start the latest resumable task.
        #[arg(long, short)]
        latest: bool,
    },

    /// Pause a running task.
    Pause {
        /// Task identifier.
        task_id: String,
    },

    /// Resume a paused task.
    Resume {
        /// Task identifier.
        task_id: String,
        /// Reset blocked items back to unresolved before resuming.
        #[arg(long)]
        reset_blocked: bool,
    },

    #[command(alias = "log")]
    /// Show task logs.
    Logs {
        /// Task identifier.
        task_id: String,

        /// Follow the log stream.
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to tail.
        #[arg(short = 'n', long, default_value = "100")]
        tail: usize,

        /// Include timestamps in the output.
        #[arg(long)]
        timestamps: bool,
    },

    #[command(alias = "rm")]
    /// Delete one or more tasks.
    Delete {
        /// Task identifier(s).
        #[arg(required_unless_present = "all")]
        task_ids: Vec<String>,

        /// Delete all tasks (optionally filtered by --status and/or --project).
        #[arg(long)]
        all: bool,

        /// Only delete tasks matching this status (used with --all).
        #[arg(long)]
        status: Option<String>,

        /// Only delete tasks in this project (used with --all).
        #[arg(long, short = 'p')]
        project: Option<String>,

        /// Skip interactive confirmation.
        #[arg(short, long)]
        force: bool,
    },

    /// Retry a failed task item.
    Retry {
        /// Task-item identifier.
        task_item_id: String,

        /// Skip interactive confirmation.
        #[arg(short, long)]
        force: bool,
    },

    /// Recover orphaned running items for a task.
    Recover {
        /// Task identifier.
        task_id: String,
    },

    /// Watch task status continuously.
    Watch {
        /// Task identifier.
        task_id: String,

        /// Refresh interval in seconds.
        #[arg(long, default_value = "2")]
        interval: u64,

        /// Exit after this many seconds (0 = no timeout).
        #[arg(long, default_value = "0")]
        timeout: u64,
    },

    /// Render the structured task trace.
    Trace {
        /// Task identifier.
        task_id: String,

        /// Include verbose trace entries.
        #[arg(long)]
        verbose: bool,

        /// Emit JSON instead of terminal rendering.
        #[arg(long)]
        json: bool,
    },
}

/// Commands for interacting with workflow stores.
#[derive(Subcommand, Debug, Clone)]
pub enum StoreCommands {
    /// Read one workflow store entry.
    Get {
        /// Workflow store name.
        store: String,
        /// Store key.
        key: String,
        /// Project identifier.
        #[arg(short, long, default_value = "")]
        project: String,
    },
    /// Write one workflow store entry.
    Put {
        /// Workflow store name.
        store: String,
        /// Store key.
        key: String,
        /// JSON or string value to persist.
        value: String,
        /// Project identifier.
        #[arg(short, long, default_value = "")]
        project: String,
        /// Task identifier used for audit metadata.
        #[arg(short, long, default_value = "")]
        task_id: String,
    },
    /// Delete one workflow store entry.
    Delete {
        /// Workflow store name.
        store: String,
        /// Store key.
        key: String,
        /// Project identifier.
        #[arg(short, long, default_value = "")]
        project: String,
    },
    #[command(alias = "ls")]
    /// List workflow store entries.
    List {
        /// Workflow store name.
        store: String,
        /// Project identifier.
        #[arg(short, long, default_value = "")]
        project: String,
        /// Maximum number of rows to return.
        #[arg(short, long, default_value = "100")]
        limit: u64,
        /// Row offset for pagination.
        #[arg(long, default_value = "0")]
        offset: u64,
        /// Output encoding.
        #[arg(short = 'o', long, default_value = "table")]
        output: OutputFormat,
    },
    /// Prune workflow store entries according to retention rules.
    Prune {
        /// Workflow store name.
        store: String,
        /// Project identifier.
        #[arg(short, long, default_value = "")]
        project: String,
    },
}

/// Secret-management commands available to operators.
#[derive(Subcommand, Debug, Clone)]
pub enum SecretCommands {
    /// Secret key operations
    #[command(subcommand)]
    Key(SecretKeyCommands),
}

/// Secret key lifecycle commands.
#[derive(Subcommand, Debug, Clone)]
pub enum SecretKeyCommands {
    /// Show active key status
    Status {
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// List all keys
    #[command(alias = "ls")]
    List {
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Rotate the active key
    Rotate {
        /// Resume an incomplete rotation
        #[arg(long)]
        resume: bool,
    },
    /// Revoke a key
    Revoke {
        /// Key ID to revoke
        key_id: String,
        /// Force revocation of the active key
        #[arg(long)]
        force: bool,
    },
    /// Show key audit history
    History {
        /// Maximum events to show
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,
        /// Filter by key ID
        #[arg(long)]
        key_id: Option<String>,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

/// Agent lifecycle commands for scheduling control.
#[derive(Subcommand, Debug, Clone)]
pub enum AgentCommands {
    /// List agents and their lifecycle state
    #[command(alias = "ls")]
    List {
        /// Optional project filter.
        #[arg(short, long)]
        project: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Mark an agent as unschedulable (no new work dispatched)
    Cordon {
        /// Agent name.
        agent_name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Mark a cordoned agent as schedulable again
    Uncordon {
        /// Agent name.
        agent_name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Drain an agent: cordon + wait for in-flight work to complete
    Drain {
        /// Agent name.
        agent_name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,

        /// Timeout in seconds; force-drain after this duration
        #[arg(long)]
        timeout: Option<u64>,
    },
}

/// Event lifecycle commands for cleanup and statistics.
#[derive(Subcommand, Debug, Clone)]
pub enum EventCommands {
    /// Clean up old events from terminated tasks
    Cleanup {
        /// Delete events older than this many days (default 30).
        #[arg(long = "older-than", default_value_t = 30)]
        older_than_days: u32,

        /// Preview how many events would be deleted without deleting.
        #[arg(long)]
        dry_run: bool,

        /// Archive events to JSONL before deleting.
        #[arg(long)]
        archive: bool,
    },

    /// Show event table statistics
    Stats,
}

/// Trigger lifecycle commands for suspend, resume, and manual fire.
#[derive(Subcommand, Debug, Clone)]
pub enum TriggerCommands {
    /// Suspend a trigger so it stops firing
    Suspend {
        /// Trigger name.
        name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Resume a suspended trigger
    Resume {
        /// Trigger name.
        name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Manually fire a trigger once, creating a task
    Fire {
        /// Trigger name.
        name: String,

        /// Optional project override.
        #[arg(short, long)]
        project: Option<String>,
    },
}

/// Supported human-readable and machine-readable output encodings.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OutputFormat {
    /// Human-readable table output.
    Table,
    /// JSON output.
    Json,
    /// YAML output.
    Yaml,
}
