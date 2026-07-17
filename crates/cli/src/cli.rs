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

        /// Atomically remove SourceTaskBinding references (requires Admin authorization).
        #[arg(long, requires = "force")]
        force_references: bool,

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

    /// Cross-task human attention queue
    #[command(alias = "attn", subcommand)]
    Attention(AttentionCommands),

    /// External source events and process bindings
    #[command(alias = "src", subcommand)]
    Source(SourceCommands),

    /// Query canonical control-plane action audit evidence
    #[command(subcommand)]
    Audit(AuditCommands),

    /// Process Console operational metrics
    #[command(subcommand)]
    Metrics(MetricsCommands),

    /// Generate and inspect immutable task handoffs
    #[command(subcommand)]
    Handoff(HandoffCommands),

    /// Preview and execute safe logical resume operations
    #[command(subcommand)]
    Resume(ResumeCommands),

    /// Trigger lifecycle operations (suspend, resume, fire)
    #[command(alias = "tg", subcommand)]
    Trigger(TriggerCommands),

    /// QA observability tools
    #[command(subcommand)]
    Qa(QaCommands),

    /// Daemon lifecycle operations (stop, status)
    #[command(subcommand)]
    Daemon(DaemonCommands),

    /// Built-in tools for CRD plugin scripts
    #[command(subcommand)]
    Tool(ToolCommands),

    /// Show version
    Version {
        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },

    /// Show a guided reference for CLI commands with examples
    #[command(alias = "gd")]
    Guide {
        /// Filter by command name (e.g. "task", "apply", "store get")
        #[arg(value_name = "COMMAND")]
        command_filter: Option<String>,

        /// Filter by category (e.g. "resource", "task", "agent")
        #[arg(short = 'c', long)]
        category: Option<String>,

        /// Output format
        #[arg(short, long, default_value = "markdown")]
        format: GuideFormat,
    },

    /// Execute workflow step(s) synchronously.
    Run {
        /// Workflow identifier (required unless --template is specified).
        #[arg(short = 'W', long)]
        workflow: Option<String>,

        /// Execute only the specified step(s). Repeatable.
        #[arg(short = 'S', long = "step")]
        step: Vec<String>,

        /// Inject a pipeline variable (key=value). Repeatable.
        #[arg(long = "set", value_parser = parse_key_val)]
        set: Vec<(String, String)>,

        /// Optional project identifier.
        #[arg(short, long)]
        project: Option<String>,

        /// Optional workspace identifier.
        #[arg(short, long)]
        workspace: Option<String>,

        /// Explicit target files for the task.
        #[arg(short, long)]
        target_file: Vec<String>,

        /// Run in background (equivalent to task create).
        #[arg(long)]
        detach: bool,

        /// Step template name (Phase 3: direct assembly without workflow).
        #[arg(long)]
        template: Option<String>,

        /// Agent capability for direct assembly mode.
        #[arg(long)]
        agent_capability: Option<String>,

        /// Execution profile override for direct assembly mode.
        #[arg(long)]
        profile: Option<String>,
    },
}

/// Cross-task attention queue operations.
#[derive(Subcommand, Debug, Clone)]
pub enum AttentionCommands {
    /// List attention items with optional filters.
    #[command(alias = "ls")]
    List {
        /// Optional project filter.
        #[arg(short, long)]
        project: Option<String>,
        /// Optional lifecycle state filter.
        #[arg(long)]
        state: Option<String>,
        /// Optional policy kind filter.
        #[arg(long)]
        kind: Option<String>,
        /// Optional severity filter.
        #[arg(long)]
        severity: Option<String>,
        /// Optional assignee (`me`, `unassigned`, or actor ID).
        #[arg(long)]
        assignee: Option<String>,
        /// Optional task filter.
        #[arg(long)]
        task: Option<String>,
        /// Maximum number of results.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Get one attention item.
    Get {
        /// Attention item ID.
        id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
    /// Claim an open attention item.
    Claim {
        /// Attention item ID.
        id: String,
        /// Current optimistic concurrency version.
        #[arg(long)]
        expected_version: i64,
        /// Optional retry-safe idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Snooze an open or claimed item.
    Snooze {
        /// Attention item ID.
        id: String,
        /// Current optimistic concurrency version.
        #[arg(long)]
        expected_version: i64,
        /// RFC3339 wake-up time.
        #[arg(long)]
        until: String,
        /// Optional retry-safe idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Resolve an attention item.
    Resolve {
        /// Attention item ID.
        id: String,
        /// Current optimistic concurrency version.
        #[arg(long)]
        expected_version: i64,
        /// Short resolution reason.
        #[arg(long)]
        reason: String,
        /// Optional retry-safe idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Execute an allowlisted action.
    Action {
        /// Attention item ID.
        id: String,
        /// Allowlisted action ID.
        action_id: String,
        /// Current optimistic concurrency version.
        #[arg(long)]
        expected_version: i64,
        /// Bounded JSON object input.
        #[arg(long, default_value = "{}")]
        input: String,
        /// Optional retry-safe idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Follow monotonic attention queue changes.
    Follow {
        /// Resume after this change sequence.
        #[arg(long, default_value_t = 0)]
        after: i64,
        /// Optional project filter.
        #[arg(short, long)]
        project: Option<String>,
        /// Output encoding for each change.
        #[arg(short, long, default_value = "json")]
        output: OutputFormat,
    },
}

/// External source event and binding operations.
#[derive(Subcommand, Debug, Clone)]
pub enum SourceCommands {
    /// Manage and preview governed source-to-task templates.
    Template {
        /// Template operation.
        #[command(subcommand)]
        command: SourceTemplateCommands,
    },
    /// Match and control governed source-to-task bindings.
    Binding {
        /// Binding operation.
        #[command(subcommand)]
        command: SourceBindingCommands,
    },
    /// List recent source events.
    #[command(alias = "ls")]
    List {
        /// Optional project filter.
        #[arg(short, long)]
        project: Option<String>,
        /// Optional routed task filter.
        #[arg(long)]
        task: Option<String>,
        /// Optional routing state filter.
        #[arg(long)]
        state: Option<String>,
        /// Maximum number of events.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Get one source event.
    Get {
        /// Source event ID.
        id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
    /// Ingest one provider-neutral normalized event fixture.
    Ingest {
        /// Project selected by trusted adapter configuration.
        #[arg(short, long)]
        project: String,
        /// JSON file containing NormalizedSourceEvent, or `-` for stdin.
        #[arg(short, long)]
        file: String,
        /// Optional authenticated raw-payload digest.
        #[arg(long)]
        payload_hash: Option<String>,
    },
    /// List source bindings for one task.
    Bindings {
        /// Task/process ID.
        task_id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Bind provider conversation coordinates to a task.
    Bind {
        /// Project ID.
        #[arg(short, long)]
        project: String,
        /// Task/process ID.
        #[arg(long)]
        task: String,
        /// Provider name.
        #[arg(long)]
        provider: String,
        /// Installation ID.
        #[arg(long)]
        installation: String,
        /// Optional conversation ID.
        #[arg(long)]
        conversation: Option<String>,
        /// Optional thread/root ID.
        #[arg(long)]
        thread: Option<String>,
        /// Binding type.
        #[arg(long, default_value = "primary")]
        binding_type: String,
        /// Source event that authorized/created the binding.
        #[arg(long)]
        source_event: String,
    },
    /// Replay a failed or attention-blocked route.
    Replay {
        /// Source event ID.
        id: String,
    },
}

/// Governed SourceTaskBinding operations.
#[derive(Subcommand, Debug, Clone)]
pub enum SourceBindingCommands {
    /// Simulate deterministic matching without side effects or provider API calls.
    Simulate {
        /// Project namespace.
        #[arg(short, long, default_value = "default")]
        project: String,
        /// Source provider, currently `slack`.
        #[arg(long, default_value = "slack")]
        provider: String,
        /// Trusted installation identifier.
        #[arg(long)]
        installation: String,
        /// Normalized event kind.
        #[arg(long, default_value = "reaction_added")]
        event_kind: String,
        /// Exact normalized reaction name without colons.
        #[arg(long)]
        reaction: String,
        /// Normalized target kind.
        #[arg(long, default_value = "message")]
        target_kind: String,
        /// Source channel identifier.
        #[arg(long)]
        channel: String,
        /// Authenticated external actor identifier.
        #[arg(long)]
        actor: String,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
    /// Suspend a binding immediately.
    Suspend {
        /// SourceTaskBinding name.
        name: String,
        /// Project namespace.
        #[arg(short, long, default_value = "default")]
        project: String,
    },
    /// Resume a binding after conflict validation.
    Resume {
        /// SourceTaskBinding name.
        name: String,
        /// Project namespace.
        #[arg(short, long, default_value = "default")]
        project: String,
    },
}

/// Governed source-to-task template operations.
#[derive(Subcommand, Debug, Clone)]
pub enum SourceTemplateCommands {
    /// Render a side-effect-free sample using the daemon's active configuration.
    Preview {
        /// SourceTaskTemplate name.
        name: String,
        /// Project namespace.
        #[arg(short, long, default_value = "default")]
        project: String,
        /// Source provider, currently `slack`.
        #[arg(long)]
        provider: String,
        /// Trusted installation identifier used for the sample.
        #[arg(long)]
        installation: String,
        /// Canonical source message permalink.
        #[arg(long)]
        message_url: String,
        /// Optional provider event identifier.
        #[arg(long)]
        event_id: Option<String>,
        /// Optional reaction or badge value.
        #[arg(long)]
        reaction: Option<String>,
        /// Optional provider-neutral target identifier.
        #[arg(long)]
        target_id: Option<String>,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

/// Canonical control-plane action audit queries.
#[derive(Subcommand, Debug, Clone)]
pub enum AuditCommands {
    /// List recent action audit records within one project.
    #[command(alias = "ls")]
    List {
        /// Required project isolation scope.
        #[arg(short, long)]
        project: String,
        /// Optional trusted actor filter.
        #[arg(long)]
        actor: Option<String>,
        /// Optional target kind filter.
        #[arg(long)]
        target_type: Option<String>,
        /// Optional target identifier filter.
        #[arg(long)]
        target_id: Option<String>,
        /// Optional closed action filter.
        #[arg(long)]
        action: Option<String>,
        /// Optional terminal status filter.
        #[arg(long)]
        status: Option<String>,
        /// Inclusive RFC3339 lower timestamp bound.
        #[arg(long)]
        from: Option<String>,
        /// Exclusive RFC3339 upper timestamp bound.
        #[arg(long)]
        to: Option<String>,
        /// Maximum number of records.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Get one action audit record by request ID.
    Get {
        /// Canonical request identifier.
        request_id: String,
        /// Required project isolation scope.
        #[arg(short, long)]
        project: String,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

/// Immutable task handoff operations.
#[derive(Subcommand, Debug, Clone)]
pub enum HandoffCommands {
    /// Generate an immutable snapshot at the latest or selected event cursor.
    Generate {
        /// Source task ID.
        task_id: String,
        /// Optional deterministic event cursor.
        #[arg(long)]
        cursor: Option<i64>,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
    /// Get a previously generated snapshot.
    Get {
        /// Snapshot ID.
        id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

/// Two-stage safe logical resume operations.
#[derive(Subcommand, Debug, Clone)]
pub enum ResumeCommands {
    /// List logical boundaries and their side-effect classifications.
    Boundaries {
        /// Source task ID.
        task_id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Persist an expiring consequence preview without changing task/workspace state.
    Plan {
        /// Source task ID.
        task_id: String,
        /// Boundary ID returned by `resume boundaries`.
        #[arg(long)]
        boundary: String,
        /// continue_task, retry_item, restart_from_boundary, or resume_provider_session.
        #[arg(long)]
        mode: String,
        /// Optional Attention Inbox item to correlate.
        #[arg(long)]
        attention_item: Option<String>,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
    /// Execute a previously reviewed plan with stale-state protection.
    Execute {
        /// Resume plan ID.
        plan_id: String,
        /// State version returned by `resume plan`.
        #[arg(long)]
        expected_state_version: String,
        /// Required operator reason recorded in audit evidence.
        #[arg(long)]
        reason: String,
        /// Retry-safe idempotency key.
        #[arg(long)]
        idempotency_key: String,
        /// Explicit confirmation for policy-enabled non-idempotent replay.
        #[arg(long)]
        elevated_confirmation: bool,
        /// Output encoding.
        #[arg(short, long, default_value = "yaml")]
        output: OutputFormat,
    },
}

/// Parse a key=value pair for --set flags.
fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid key=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
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

/// Built-in tools callable from CRD plugin scripts.
#[derive(Subcommand, Debug, Clone)]
pub enum ToolCommands {
    /// Verify an HMAC signature (exit 0 = valid, exit 1 = invalid)
    #[command(name = "webhook-verify-hmac")]
    WebhookVerifyHmac {
        /// HMAC algorithm (sha256)
        #[arg(long, default_value = "sha256")]
        algo: String,
        /// Shared secret
        #[arg(long)]
        secret: String,
        /// Request body to verify
        #[arg(long)]
        body: String,
        /// Expected signature (hex, with optional sha256= prefix)
        #[arg(long)]
        signature: String,
    },
    /// Extract a value from JSON using a dot-separated path (reads stdin)
    #[command(name = "payload-extract")]
    PayloadExtract {
        /// Dot-separated JSON path (e.g. "event.type")
        #[arg(long)]
        path: String,
    },
    /// Rotate a key in a SecretStore (requires running daemon)
    #[command(name = "secret-rotate")]
    SecretRotate {
        /// SecretStore name
        store: String,
        /// Key to rotate
        key: String,
        /// New value
        #[arg(long)]
        value: String,
        /// Project scope
        #[arg(short, long)]
        project: Option<String>,
    },
}

/// QA observability commands.
#[derive(Subcommand, Debug, Clone)]
pub enum QaCommands {
    /// Show observability health metrics from task_execution_metrics
    Doctor {
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
}

/// Process Console operational metric commands.
#[derive(Subcommand, Debug, Clone)]
pub enum MetricsCommands {
    /// Query one project-scoped Process Console snapshot.
    Process {
        /// Required project isolation scope.
        #[arg(short, long)]
        project: String,
        /// Bounded lookback window such as 1h, 24h, or 7d.
        #[arg(long, default_value = "24h")]
        window: String,
        /// Materialized bucket such as 5m, 1h, or 1d.
        #[arg(long, default_value = "1h")]
        bucket: String,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Rebuild retained materialized rollups for one project.
    Rebuild {
        /// Required project isolation scope.
        #[arg(short, long)]
        project: String,
    },
    /// Delete optional metrics older than the retention threshold.
    Prune {
        /// Retention in days; zero uses RuntimePolicy.
        #[arg(long, default_value_t = 0)]
        retention_days: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        AgentCommands, AgentSessionCommands, AuditCommands, Cli, Commands, DbCommands,
        DbMigrationCommands, EventCommands, MetricsCommands, TaskCommands,
    };
    use clap::Parser;

    #[test]
    fn version_subcommand_accepts_json_flag() {
        let cli = Cli::try_parse_from(["orchestrator", "version", "--json"])
            .expect("version --json should parse");
        assert!(matches!(cli.command, Commands::Version { json: true }));
    }

    #[test]
    fn process_metrics_command_requires_project_and_accepts_window() {
        let cli = Cli::try_parse_from([
            "orchestrator",
            "metrics",
            "process",
            "--project",
            "default",
            "--window",
            "7d",
            "--bucket",
            "6h",
            "-o",
            "json",
        ])
        .expect("metrics process should parse");
        assert!(matches!(
            cli.command,
            Commands::Metrics(MetricsCommands::Process { project, window, .. })
                if project == "default" && window == "7d"
        ));
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

    #[test]
    fn task_timeline_subcommand_parses_follow_filters_and_output() {
        let cli = Cli::try_parse_from([
            "orchestrator",
            "task",
            "timeline",
            "task-1",
            "--category",
            "failure",
            "--follow",
            "--output",
            "json",
        ])
        .expect("task timeline should parse");
        assert!(matches!(
            cli.command,
            Commands::Task(TaskCommands::Timeline {
                task_id,
                categories,
                follow: true,
                output: super::OutputFormat::Json,
                ..
            }) if task_id == "task-1" && categories == ["failure"]
        ));
    }

    #[test]
    fn audit_list_requires_project_and_parses_filters() {
        let cli = Cli::try_parse_from([
            "orchestrator",
            "audit",
            "list",
            "--project",
            "demo",
            "--status",
            "failed",
            "--output",
            "json",
        ])
        .expect("audit list should parse");
        assert!(matches!(
            cli.command,
            Commands::Audit(AuditCommands::List {
                project,
                status: Some(status),
                output: super::OutputFormat::Json,
                ..
            }) if project == "demo" && status == "failed"
        ));
        assert!(Cli::try_parse_from(["orchestrator", "audit", "list"]).is_err());
    }

    #[test]
    fn agent_session_read_parses_committed_offset_output() {
        let cli = Cli::try_parse_from([
            "orchestrator",
            "agent",
            "session",
            "read",
            "session-1",
            "--offset",
            "42",
            "--chunks-json",
        ])
        .expect("session read should parse");
        assert!(matches!(
            cli.command,
            Commands::Agent(AgentCommands::Session(AgentSessionCommands::Read {
                session_id,
                offset: 42,
                chunks_json: true,
                ..
            })) if session_id == "session-1"
        ));
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

    /// Run VACUUM to reclaim disk space
    Vacuum,

    /// Clean up old log files from terminated tasks
    Cleanup {
        /// Delete logs older than this many days (default 30).
        #[arg(long = "older-than", default_value_t = 30)]
        older_than_days: u32,
    },
}

/// Subcommands for inspecting database migration state.
#[derive(Subcommand, Debug, Clone)]
pub enum DbMigrationCommands {
    /// List registered migrations and their applied state
    #[command(alias = "ls")]
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

        /// Execute only the specified step(s) from the workflow. Repeatable.
        #[arg(short = 'S', long = "step")]
        step: Vec<String>,

        /// Inject a pipeline variable (key=value). Repeatable.
        #[arg(long = "set", value_parser = parse_key_val)]
        set: Vec<(String, String)>,
    },

    /// List task items and their status.
    Items {
        /// Task identifier.
        task_id: String,

        /// Filter by item status.
        #[arg(short, long)]
        status: Option<String>,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
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

    /// Show the semantic process timeline for a task.
    Timeline {
        /// Task identifier.
        task_id: String,

        /// Continue after this opaque pagination cursor.
        #[arg(long)]
        cursor: Option<String>,

        /// Maximum entries to return per page.
        #[arg(short, long, default_value_t = 50)]
        limit: u32,

        /// Include only this category; repeat to select multiple categories.
        #[arg(long = "category")]
        categories: Vec<String>,

        /// Follow new timeline entries after the initial page.
        #[arg(short, long)]
        follow: bool,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
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
    /// Bootstrap a new key when all keys are in terminal state (emergency recovery)
    Bootstrap,
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
    /// Observe and control interactive agent sessions.
    #[command(subcommand)]
    Session(AgentSessionCommands),
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

/// Interactive agent session operations.
#[derive(Subcommand, Debug, Clone)]
pub enum AgentSessionCommands {
    /// List sessions.
    List {
        /// Optional task filter.
        #[arg(long)]
        task: Option<String>,
        /// Optional agent filter.
        #[arg(long)]
        agent: Option<String>,
        /// Optional canonical state filter.
        #[arg(long)]
        state: Option<String>,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Get one session.
    Get {
        /// Session identifier.
        session_id: String,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Attach as a reader or explicitly acquire the writer lease.
    Attach {
        /// Session identifier.
        session_id: String,
        /// Attachment mode.
        #[arg(long, default_value = "reader")]
        mode: String,
        /// Stable client instance identifier.
        #[arg(long, default_value = "cli")]
        client_id: String,
    },
    /// Follow or read transcript bytes from an offset.
    Read {
        /// Session identifier.
        session_id: String,
        /// Continue following appended bytes.
        #[arg(long)]
        follow: bool,
        /// Committed source byte offset.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// Emit one JSON object per chunk, including next_offset, instead of raw transcript text.
        #[arg(long)]
        chunks_json: bool,
    },
    /// Renew a writer lease.
    Heartbeat {
        /// Session identifier.
        session_id: String,
        /// Writer client identifier.
        #[arg(long)]
        client_id: String,
        /// Current fencing token.
        #[arg(long)]
        fencing_token: i64,
    },
    /// Send input with the current writer fencing token.
    SendInput {
        /// Session identifier.
        session_id: String,
        /// Text to send.
        #[arg(long)]
        text: String,
        /// Writer client identifier.
        #[arg(long)]
        client_id: String,
        /// Current fencing token.
        #[arg(long)]
        fencing_token: i64,
        /// Retry-stable idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Detach a reader or writer.
    Detach {
        /// Session identifier.
        session_id: String,
        /// Attachment mode.
        #[arg(long, default_value = "reader")]
        mode: String,
        /// Client identifier.
        #[arg(long, default_value = "cli")]
        client_id: String,
        /// Required for writer detach.
        #[arg(long)]
        fencing_token: Option<i64>,
        /// Audited detach reason.
        #[arg(long, default_value = "client detach")]
        reason: String,
    },
    /// Close the backing session process.
    Close {
        /// Session identifier.
        session_id: String,
        /// Audited close reason.
        #[arg(long)]
        reason: String,
        /// Optional optimistic state version.
        #[arg(long)]
        expected_version: Option<i64>,
        /// Retry-stable idempotency key.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Resolve a diagnostic PID to sessions.
    Resolve {
        /// Diagnostic process identifier.
        #[arg(long)]
        pid: i64,
        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
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

    /// List events for a task.
    #[command(alias = "ls")]
    List {
        /// Task identifier (required).
        #[arg(long)]
        task: String,

        /// Filter by event type (prefix match).
        #[arg(long = "type")]
        event_type: Option<String>,

        /// Maximum number of events to return.
        #[arg(short, long, default_value_t = 50)]
        limit: u32,

        /// Output encoding.
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
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

        /// Optional JSON payload (simulates a webhook body).
        #[arg(long)]
        payload: Option<String>,
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

/// Output format for the `guide` subcommand.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum GuideFormat {
    /// Markdown output (default, AI-agent friendly).
    Markdown,
    /// JSON output for programmatic consumption.
    Json,
}
