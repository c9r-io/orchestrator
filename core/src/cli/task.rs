use super::OutputFormat;
use clap::Subcommand;

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

        /// Enqueue task for background worker instead of running inline (default)
        #[arg(long, default_value_t = true)]
        detach: bool,

        /// Run task inline (blocking) instead of detaching to background worker
        #[arg(long, conflicts_with = "detach")]
        attach: bool,
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

    /// Watch task execution in real-time (auto-refreshing status panel)
    Watch {
        /// Task ID
        task_id: String,

        /// Refresh interval in seconds
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },

    /// Show execution trace with anomaly detection
    Trace {
        /// Task ID
        task_id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Show all events (verbose)
        #[arg(long, short)]
        verbose: bool,
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

        /// Force retry without confirmation (resets execution state)
        #[arg(short, long)]
        force: bool,
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

    /// Session control commands
    #[command(subcommand)]
    Session(TaskSessionCommands),
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
pub enum TaskSessionCommands {
    /// List sessions for a task
    List {
        /// Task ID
        task_id: String,
        /// Output format
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Show a specific session
    Info {
        /// Session ID
        session_id: String,
        /// Output format
        #[arg(short, long, default_value = "table")]
        output: OutputFormat,
    },
    /// Close a running session
    Close {
        /// Session ID
        session_id: String,
        /// Force kill the backing process
        #[arg(long)]
        force: bool,
    },
}
