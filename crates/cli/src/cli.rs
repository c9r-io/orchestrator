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
        prune: bool,

        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Get resource(s)
    #[command(alias = "g")]
    Get {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(value_name = "NAME")]
        name: Option<String>,

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

        #[arg(value_name = "NAME")]
        name: Option<String>,

        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,

        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Delete a resource
    #[command(alias = "rm")]
    Delete {
        #[arg(value_name = "RESOURCE")]
        resource: String,

        #[arg(value_name = "NAME")]
        name: Option<String>,

        #[arg(short, long)]
        force: bool,

        #[arg(long)]
        dry_run: bool,

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
        #[arg(long)]
        component: Option<String>,

        #[command(subcommand)]
        command: Option<DebugCommands>,
    },

    /// Preflight check
    #[command(alias = "ck")]
    Check {
        #[arg(long)]
        workflow: Option<String>,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,

        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Initialize orchestrator runtime
    Init { root: Option<String> },

    /// Secret key management
    #[command(subcommand)]
    Secret(SecretCommands),

    /// Database operations
    #[command(subcommand)]
    Db(DbCommands),

    /// Manifest operations
    #[command(subcommand)]
    Manifest(ManifestCommands),

    /// Show version
    Version {
        #[arg(long)]
        json: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, DbCommands, DbMigrationCommands, SecretCommands, SecretKeyCommands};
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
}

#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommands {
    /// Run a local sandbox probe without contacting the daemon
    SandboxProbe {
        #[command(subcommand)]
        probe: SandboxProbeCommands,
    },

    #[command(hide = true)]
    ChildIdle {
        #[arg(long, default_value = "60")]
        sleep_secs: u64,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum SandboxProbeCommands {
    WriteFile {
        #[arg(long)]
        path: String,

        #[arg(long, default_value = "probe")]
        contents: String,
    },
    OpenFiles {
        #[arg(long, default_value = "256")]
        count: usize,
    },
    CpuBurn,
    AllocMemory {
        #[arg(long, default_value = "8")]
        chunk_mb: usize,

        #[arg(long, default_value = "256")]
        total_mb: usize,
    },
    SpawnChildren {
        #[arg(long, default_value = "64")]
        count: usize,

        #[arg(long, default_value = "60")]
        sleep_secs: u64,
    },
    DnsResolve {
        #[arg(long, default_value = "example.com")]
        host: String,

        #[arg(long, default_value = "443")]
        port: u16,
    },
    TcpConnect {
        #[arg(long)]
        host: String,

        #[arg(long)]
        port: u16,

        #[arg(long, default_value = "3")]
        timeout_secs: u64,
    },
    #[command(hide = true)]
    TcpServe {
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,

        #[arg(long)]
        port: u16,

        #[arg(long)]
        ready_file: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManifestCommands {
    /// Validate a manifest file
    Validate {
        #[arg(short = 'f', long = "file")]
        file: String,

        #[arg(short = 'p', long)]
        project: Option<String>,
    },

    /// Export all resources as manifest documents
    Export {
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DbCommands {
    /// Show schema status for the local database
    Status {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    /// Database migration operations
    #[command(subcommand)]
    Migrations(DbMigrationCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub enum DbMigrationCommands {
    /// List registered migrations and their applied state
    #[command(alias = "ls")]
    List {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TaskCommands {
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
    },

    #[command(alias = "get")]
    Info {
        task_id: String,

        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },

    Start {
        task_id: Option<String>,

        #[arg(long, short)]
        latest: bool,
    },

    Pause {
        task_id: String,
    },

    Resume {
        task_id: String,
    },

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

    #[command(alias = "rm")]
    Delete {
        task_id: String,

        #[arg(short, long)]
        force: bool,
    },

    Retry {
        task_item_id: String,

        #[arg(short, long)]
        force: bool,
    },

    Watch {
        task_id: String,

        #[arg(long, default_value = "2")]
        interval: u64,
    },

    Trace {
        task_id: String,

        #[arg(long)]
        verbose: bool,

        #[arg(long)]
        json: bool,
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
pub enum SecretCommands {
    /// Secret key operations
    #[command(subcommand)]
    Key(SecretKeyCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub enum SecretKeyCommands {
    /// Show active key status
    Status {
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// List all keys
    #[command(alias = "ls")]
    List {
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
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}
