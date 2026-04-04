//! `orchestrator guide` — self-describing CLI reference for AI agents and users.
//!
//! Each command domain provides its own [`GuideEntry`] list.  The guide
//! subcommand collects, filters, and renders them as Markdown or JSON.

use std::fmt;

use anyhow::Result;
use serde::Serialize;

use crate::GuideFormat;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// Functional category for grouping commands in the guide output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum GuideCategory {
    /// apply, get, describe, delete
    ResourceManagement,
    /// task *
    TaskLifecycle,
    /// run
    WorkflowExecution,
    /// agent *
    AgentManagement,
    /// store *
    StoreOperations,
    /// daemon, db, debug, check, init, version, qa
    SystemAdmin,
    /// secret *
    Security,
    /// event *
    Observability,
    /// trigger *
    Trigger,
    /// manifest *
    WorkflowAuthoring,
    /// tool *
    BuiltinTools,
}

impl fmt::Display for GuideCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceManagement => write!(f, "Resource Management"),
            Self::TaskLifecycle => write!(f, "Task Lifecycle"),
            Self::WorkflowExecution => write!(f, "Workflow Execution"),
            Self::AgentManagement => write!(f, "Agent Management"),
            Self::StoreOperations => write!(f, "Store Operations"),
            Self::SystemAdmin => write!(f, "System Administration"),
            Self::Security => write!(f, "Security"),
            Self::Observability => write!(f, "Observability"),
            Self::Trigger => write!(f, "Trigger Management"),
            Self::WorkflowAuthoring => write!(f, "Workflow Authoring"),
            Self::BuiltinTools => write!(f, "Built-in Tools"),
        }
    }
}

impl GuideCategory {
    /// Canonical ordering for rendering.
    fn sort_key(self) -> u8 {
        match self {
            Self::ResourceManagement => 0,
            Self::TaskLifecycle => 1,
            Self::WorkflowExecution => 2,
            Self::AgentManagement => 3,
            Self::StoreOperations => 4,
            Self::WorkflowAuthoring => 5,
            Self::Trigger => 6,
            Self::Observability => 7,
            Self::Security => 8,
            Self::SystemAdmin => 9,
            Self::BuiltinTools => 10,
        }
    }

    /// Match a user-supplied filter string against this category (case-insensitive prefix).
    fn matches(&self, filter: &str) -> bool {
        let lower = filter.to_ascii_lowercase();
        let name = format!("{self}").to_ascii_lowercase();
        name.starts_with(&lower) || self.short_name().starts_with(&lower)
    }

    fn short_name(&self) -> &'static str {
        match self {
            Self::ResourceManagement => "resource",
            Self::TaskLifecycle => "task",
            Self::WorkflowExecution => "workflow",
            Self::AgentManagement => "agent",
            Self::StoreOperations => "store",
            Self::SystemAdmin => "system",
            Self::Security => "security",
            Self::Observability => "observability",
            Self::Trigger => "trigger",
            Self::WorkflowAuthoring => "authoring",
            Self::BuiltinTools => "tools",
        }
    }
}

/// A single command entry in the guide.
#[derive(Debug, Serialize)]
pub struct GuideEntry {
    /// Command path, e.g. `"task create"` or `"apply"`.
    pub command: &'static str,
    /// Short alias, e.g. `"t new"` or `"ap"`.
    pub alias: Option<&'static str>,
    /// Category for grouping.
    pub category: GuideCategory,
    /// One-line description.
    pub summary: &'static str,
    /// Longer description with context for AI agents.
    pub description: &'static str,
    /// Usage examples as `(command_line, explanation)` pairs.
    pub examples: &'static [(&'static str, &'static str)],
}

// ---------------------------------------------------------------------------
// Per-domain entry builders
// ---------------------------------------------------------------------------

fn resource_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "apply",
            alias: Some("ap"),
            category: GuideCategory::ResourceManagement,
            summary: "Apply resource manifests",
            description: "Submit YAML manifests to the daemon. Supports dry-run, pruning of \
                          removed resources, and project scoping. Config changes are \
                          hot-reloaded — no daemon restart needed.",
            examples: &[
                (
                    "orchestrator apply -f manifest.yaml",
                    "Apply a manifest file",
                ),
                (
                    "orchestrator apply -f manifest.yaml --dry-run",
                    "Validate without persisting",
                ),
                (
                    "orchestrator apply -f manifest.yaml --prune",
                    "Delete resources not in manifest",
                ),
                (
                    "orchestrator apply -f manifest.yaml --project my-project",
                    "Apply to a specific project",
                ),
                (
                    "cat manifest.yaml | orchestrator apply -f -",
                    "Apply from stdin",
                ),
            ],
        },
        GuideEntry {
            command: "get",
            alias: Some("g"),
            category: GuideCategory::ResourceManagement,
            summary: "Get resource(s)",
            description: "List or retrieve resources by kind. Supports table/JSON/YAML output, \
                          label selectors, and project filtering.",
            examples: &[
                ("orchestrator get workspaces", "List all workspaces"),
                ("orchestrator get agents -o json", "List agents as JSON"),
                (
                    "orchestrator get workflows -o yaml",
                    "List workflows as YAML",
                ),
                (
                    "orchestrator get executionprofiles",
                    "List execution profiles",
                ),
                (
                    "orchestrator get workspaces -l env=dev",
                    "Filter by label selector",
                ),
            ],
        },
        GuideEntry {
            command: "describe",
            alias: Some("desc"),
            category: GuideCategory::ResourceManagement,
            summary: "Describe a resource",
            description: "Show the full specification of a single resource. Default output is YAML.",
            examples: &[
                (
                    "orchestrator describe workspace default",
                    "Describe the default workspace",
                ),
                (
                    "orchestrator describe executionprofile sandbox_write",
                    "Describe an execution profile",
                ),
                (
                    "orchestrator describe workflow sdlc -o json",
                    "Describe as JSON",
                ),
            ],
        },
        GuideEntry {
            command: "delete",
            alias: Some("rm"),
            category: GuideCategory::ResourceManagement,
            summary: "Delete a resource",
            description: "Remove a resource by kind and name. Supports --force to skip \
                          confirmation and --dry-run to preview.",
            examples: &[
                (
                    "orchestrator delete agent old-agent",
                    "Delete an agent (with confirmation)",
                ),
                (
                    "orchestrator delete agent old-agent --force",
                    "Delete without confirmation",
                ),
                (
                    "orchestrator delete agent old-agent --dry-run",
                    "Preview without deleting",
                ),
            ],
        },
    ]
}

fn task_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "task list",
            alias: Some("t ls"),
            category: GuideCategory::TaskLifecycle,
            summary: "List tasks with optional filters",
            description: "Show all tasks. Filter by status or project. Supports table/JSON output \
                          and verbose mode for extra detail.",
            examples: &[
                ("orchestrator task list", "List all tasks"),
                ("orchestrator task list -s running", "List running tasks"),
                ("orchestrator task list -o json", "List tasks as JSON"),
                (
                    "orchestrator task list -p my-project",
                    "List tasks in a project",
                ),
                ("orchestrator task list -v", "List with verbose detail"),
            ],
        },
        GuideEntry {
            command: "task create",
            alias: Some("t new"),
            category: GuideCategory::TaskLifecycle,
            summary: "Create a new task",
            description: "Create and optionally start a task. Enqueues work for daemon workers. \
                          Supports step filtering (--step) and pipeline variable injection (--set).",
            examples: &[
                (
                    "orchestrator task create --name X --goal Y --workflow Z --project P",
                    "Create and auto-start a task",
                ),
                (
                    "orchestrator task create --workflow sdlc --step fix --set ticket_paths=docs/ticket/T-0042.md",
                    "Run only the fix step with a variable",
                ),
                (
                    "orchestrator task create --workflow sdlc --step plan --step implement",
                    "Run multiple steps in workflow order",
                ),
                (
                    "orchestrator task create --name X --goal Y --workflow Z --no-start",
                    "Create without starting",
                ),
            ],
        },
        GuideEntry {
            command: "task items",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "List task items and their status",
            description: "Show work items within a task and their individual status.",
            examples: &[
                (
                    "orchestrator task items <task_id>",
                    "List all items for a task",
                ),
                (
                    "orchestrator task items <task_id> -s failed",
                    "Show only failed items",
                ),
            ],
        },
        GuideEntry {
            command: "task info",
            alias: Some("t get"),
            category: GuideCategory::TaskLifecycle,
            summary: "Show detailed information for one task",
            description: "Display full task metadata including status, workflow, steps, and timing.",
            examples: &[
                (
                    "orchestrator task info <task_id>",
                    "Show task details (table)",
                ),
                (
                    "orchestrator task info <task_id> -o yaml",
                    "Show task details as YAML",
                ),
            ],
        },
        GuideEntry {
            command: "task start",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Start a task",
            description: "Start a previously created task, or resume the latest resumable task.",
            examples: &[
                ("orchestrator task start <task_id>", "Start a specific task"),
                (
                    "orchestrator task start --latest",
                    "Start the most recent resumable task",
                ),
            ],
        },
        GuideEntry {
            command: "task pause",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Pause a running task",
            description: "Suspend a running task. The task can be resumed later.",
            examples: &[("orchestrator task pause <task_id>", "Pause a running task")],
        },
        GuideEntry {
            command: "task resume",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Resume a paused task",
            description: "Continue execution of a paused task. Use --reset-blocked to clear \
                          blocked items before resuming.",
            examples: &[
                ("orchestrator task resume <task_id>", "Resume a paused task"),
                (
                    "orchestrator task resume <task_id> --reset-blocked",
                    "Reset blocked items and resume",
                ),
            ],
        },
        GuideEntry {
            command: "task logs",
            alias: Some("t log"),
            category: GuideCategory::TaskLifecycle,
            summary: "Show task logs",
            description: "Display execution logs for a task. Supports following and tailing.",
            examples: &[
                ("orchestrator task logs <task_id>", "Show recent logs"),
                (
                    "orchestrator task logs <task_id> -f",
                    "Follow the log stream",
                ),
                (
                    "orchestrator task logs <task_id> -n 200",
                    "Tail last 200 lines",
                ),
                (
                    "orchestrator task logs <task_id> --timestamps",
                    "Show timestamps",
                ),
            ],
        },
        GuideEntry {
            command: "task delete",
            alias: Some("t rm"),
            category: GuideCategory::TaskLifecycle,
            summary: "Delete one or more tasks",
            description: "Remove tasks by ID or delete all tasks with optional status/project filters.",
            examples: &[
                ("orchestrator task delete <task_id>", "Delete a single task"),
                (
                    "orchestrator task delete --all --status completed",
                    "Delete all completed tasks",
                ),
                (
                    "orchestrator task delete --all --project P --force",
                    "Force delete all tasks in a project",
                ),
            ],
        },
        GuideEntry {
            command: "task retry",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Retry a failed task item",
            description: "Re-run a specific failed task item.",
            examples: &[
                (
                    "orchestrator task retry <task_item_id>",
                    "Retry with confirmation",
                ),
                (
                    "orchestrator task retry <task_item_id> --force",
                    "Retry without confirmation",
                ),
            ],
        },
        GuideEntry {
            command: "task recover",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Recover orphaned running items",
            description: "Mark orphaned running items (from crashed workers) as retryable.",
            examples: &[(
                "orchestrator task recover <task_id>",
                "Recover orphaned items",
            )],
        },
        GuideEntry {
            command: "task watch",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Watch task status continuously",
            description: "Auto-refreshing status panel. Useful for monitoring long-running tasks.",
            examples: &[
                (
                    "orchestrator task watch <task_id>",
                    "Watch with 2s refresh (default)",
                ),
                (
                    "orchestrator task watch <task_id> --interval 5",
                    "Watch with 5s refresh",
                ),
                (
                    "orchestrator task watch <task_id> --timeout 300",
                    "Stop watching after 5 minutes",
                ),
            ],
        },
        GuideEntry {
            command: "task trace",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Render the structured task trace",
            description: "Show execution timeline with step durations and anomaly detection.",
            examples: &[
                ("orchestrator task trace <task_id>", "Show execution trace"),
                (
                    "orchestrator task trace <task_id> --verbose",
                    "Include verbose entries",
                ),
                ("orchestrator task trace <task_id> --json", "Output as JSON"),
            ],
        },
    ]
}

fn run_entries() -> Vec<GuideEntry> {
    vec![GuideEntry {
        command: "run",
        alias: None,
        category: GuideCategory::WorkflowExecution,
        summary: "Execute workflow step(s) synchronously",
        description: "Lightweight execution mode. Follows logs until completion and exits with \
                      the task status code. Supports --detach for background execution and \
                      direct assembly mode (--template + --agent-capability) without a \
                      pre-defined workflow.",
        examples: &[
            (
                "orchestrator run -W sdlc -S fix --set ticket_paths=docs/ticket/T-0042.md",
                "Run the fix step synchronously",
            ),
            (
                "orchestrator run -W sdlc -S fix --detach",
                "Run in background (equivalent to task create)",
            ),
            (
                "orchestrator run --template fix-ticket --agent-capability fix --set ticket_paths=docs/ticket/T-0042.md",
                "Direct assembly: StepTemplate + capability, no workflow needed",
            ),
            (
                "orchestrator run --template fix-ticket --agent-capability fix --profile host-unrestricted",
                "Direct assembly with execution profile override",
            ),
        ],
    }]
}

fn agent_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "agent list",
            alias: Some("ag ls"),
            category: GuideCategory::AgentManagement,
            summary: "List agents and their lifecycle state",
            description: "Show all registered agents with their state, capabilities, and cost.",
            examples: &[
                ("orchestrator agent list", "List all agents"),
                (
                    "orchestrator agent list -p my-project -o json",
                    "List project agents as JSON",
                ),
            ],
        },
        GuideEntry {
            command: "agent cordon",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Mark an agent as unschedulable",
            description: "Prevent new work from being dispatched to this agent. Existing \
                          in-flight work continues.",
            examples: &[("orchestrator agent cordon my-agent", "Cordon an agent")],
        },
        GuideEntry {
            command: "agent uncordon",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Mark a cordoned agent as schedulable again",
            description: "Resume scheduling new work to a previously cordoned agent.",
            examples: &[("orchestrator agent uncordon my-agent", "Uncordon an agent")],
        },
        GuideEntry {
            command: "agent drain",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Drain an agent",
            description: "Cordon the agent and wait for in-flight work to complete. Use \
                          --timeout to force-drain after a duration.",
            examples: &[
                ("orchestrator agent drain my-agent", "Drain gracefully"),
                (
                    "orchestrator agent drain my-agent --timeout 60",
                    "Force-drain after 60s",
                ),
            ],
        },
    ]
}

fn store_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "store get",
            alias: None,
            category: GuideCategory::StoreOperations,
            summary: "Read one workflow store entry",
            description: "Retrieve a value from a workflow store by key.",
            examples: &[(
                "orchestrator store get my-store build_hash",
                "Read a store entry",
            )],
        },
        GuideEntry {
            command: "store put",
            alias: None,
            category: GuideCategory::StoreOperations,
            summary: "Write one workflow store entry",
            description: "Persist a key-value pair to a workflow store.",
            examples: &[(
                "orchestrator store put my-store build_hash abc123 -t <task_id>",
                "Write a store entry with audit task ID",
            )],
        },
        GuideEntry {
            command: "store delete",
            alias: None,
            category: GuideCategory::StoreOperations,
            summary: "Delete one workflow store entry",
            description: "Remove a single key from a workflow store.",
            examples: &[(
                "orchestrator store delete my-store old_key",
                "Delete a store entry",
            )],
        },
        GuideEntry {
            command: "store list",
            alias: Some("store ls"),
            category: GuideCategory::StoreOperations,
            summary: "List workflow store entries",
            description: "Enumerate keys in a workflow store with pagination.",
            examples: &[
                (
                    "orchestrator store list my-store",
                    "List entries (default 100)",
                ),
                (
                    "orchestrator store list my-store -l 10 --offset 20",
                    "Paginated listing",
                ),
            ],
        },
        GuideEntry {
            command: "store prune",
            alias: None,
            category: GuideCategory::StoreOperations,
            summary: "Prune workflow store entries",
            description: "Remove entries according to the store's retention rules.",
            examples: &[(
                "orchestrator store prune my-store",
                "Prune by retention rules",
            )],
        },
    ]
}

fn manifest_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "manifest validate",
            alias: None,
            category: GuideCategory::WorkflowAuthoring,
            summary: "Validate a manifest file",
            description: "Check a YAML manifest for errors without applying. Reads from file or stdin.",
            examples: &[
                (
                    "orchestrator manifest validate -f manifest.yaml",
                    "Validate a file",
                ),
                (
                    "cat manifest.yaml | orchestrator manifest validate -f -",
                    "Validate from stdin",
                ),
            ],
        },
        GuideEntry {
            command: "manifest export",
            alias: None,
            category: GuideCategory::WorkflowAuthoring,
            summary: "Export all resources as manifest documents",
            description: "Dump all currently applied resources as YAML or JSON manifests.",
            examples: &[
                ("orchestrator manifest export", "Export as YAML (default)"),
                ("orchestrator manifest export -o json", "Export as JSON"),
            ],
        },
    ]
}

fn secret_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "secret key status",
            alias: None,
            category: GuideCategory::Security,
            summary: "Show active encryption key status",
            description: "Display the currently active encryption key and its metadata.",
            examples: &[("orchestrator secret key status", "Show active key info")],
        },
        GuideEntry {
            command: "secret key list",
            alias: Some("secret key ls"),
            category: GuideCategory::Security,
            summary: "List all encryption keys",
            description: "Show all keys with their state (active/retired/revoked).",
            examples: &[
                ("orchestrator secret key list", "List keys (table)"),
                ("orchestrator secret key list -o json", "List keys as JSON"),
            ],
        },
        GuideEntry {
            command: "secret key rotate",
            alias: None,
            category: GuideCategory::Security,
            summary: "Rotate the active encryption key",
            description: "Generate a new key and re-encrypt secrets. Use --resume if a prior \
                          rotation was interrupted.",
            examples: &[
                ("orchestrator secret key rotate", "Rotate to a new key"),
                (
                    "orchestrator secret key rotate --resume",
                    "Resume an interrupted rotation",
                ),
            ],
        },
        GuideEntry {
            command: "secret key revoke",
            alias: None,
            category: GuideCategory::Security,
            summary: "Revoke a specific encryption key",
            description: "Mark a key as revoked. Use --force to revoke the currently active key.",
            examples: &[
                ("orchestrator secret key revoke <key_id>", "Revoke a key"),
                (
                    "orchestrator secret key revoke <key_id> --force",
                    "Force-revoke the active key",
                ),
            ],
        },
        GuideEntry {
            command: "secret key bootstrap",
            alias: None,
            category: GuideCategory::Security,
            summary: "Bootstrap a new encryption key",
            description: "Emergency recovery: create a fresh primary key when all keys are in \
                          terminal state.",
            examples: &[("orchestrator secret key bootstrap", "Bootstrap a new key")],
        },
        GuideEntry {
            command: "secret key history",
            alias: None,
            category: GuideCategory::Security,
            summary: "Show key audit history",
            description: "Display the audit trail for encryption key lifecycle events.",
            examples: &[
                ("orchestrator secret key history", "Show last 50 events"),
                (
                    "orchestrator secret key history -n 100",
                    "Show last 100 events",
                ),
                (
                    "orchestrator secret key history --key-id <id>",
                    "Filter by key ID",
                ),
            ],
        },
    ]
}

fn db_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "db status",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Show database schema status",
            description: "Display database info including schema version and size.",
            examples: &[
                ("orchestrator db status", "Show DB status (table)"),
                ("orchestrator db status -o json", "Show DB status as JSON"),
            ],
        },
        GuideEntry {
            command: "db migrations list",
            alias: Some("db migrations ls"),
            category: GuideCategory::SystemAdmin,
            summary: "List database migrations",
            description: "Show registered migrations and their applied state.",
            examples: &[("orchestrator db migrations list", "List migrations")],
        },
        GuideEntry {
            command: "db vacuum",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Run VACUUM to reclaim disk space",
            description: "Compact the SQLite database file.",
            examples: &[("orchestrator db vacuum", "Vacuum the database")],
        },
        GuideEntry {
            command: "db cleanup",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Clean up old log files",
            description: "Delete task log files older than the specified number of days.",
            examples: &[
                (
                    "orchestrator db cleanup",
                    "Clean up logs older than 30 days (default)",
                ),
                (
                    "orchestrator db cleanup --older-than 7",
                    "Clean up logs older than 7 days",
                ),
            ],
        },
    ]
}

fn event_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "event cleanup",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Clean up old events",
            description: "Remove events from terminated tasks. Supports dry-run and archiving.",
            examples: &[
                (
                    "orchestrator event cleanup",
                    "Delete events older than 30 days",
                ),
                (
                    "orchestrator event cleanup --older-than 7 --dry-run",
                    "Preview cleanup",
                ),
                (
                    "orchestrator event cleanup --archive",
                    "Archive events before deleting",
                ),
            ],
        },
        GuideEntry {
            command: "event list",
            alias: Some("ev ls"),
            category: GuideCategory::Observability,
            summary: "List events for a task",
            description: "Show lifecycle events for a specific task with optional type filtering. \
                          Returns up to 50 events by default; use -l to adjust.",
            examples: &[
                (
                    "orchestrator event list --task <id>",
                    "List task events (default 50)",
                ),
                (
                    "orchestrator event list --task <id> --type step",
                    "Filter by event type prefix",
                ),
                (
                    "orchestrator event list --task <id> -l 100",
                    "List up to 100 events",
                ),
            ],
        },
        GuideEntry {
            command: "event stats",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Show event table statistics",
            description: "Display aggregate statistics about the event table.",
            examples: &[("orchestrator event stats", "Show event statistics")],
        },
    ]
}

fn trigger_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "trigger suspend",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Suspend a trigger",
            description: "Pause a trigger so it stops auto-firing.",
            examples: &[(
                "orchestrator trigger suspend nightly-qa",
                "Suspend a trigger",
            )],
        },
        GuideEntry {
            command: "trigger resume",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Resume a suspended trigger",
            description: "Re-enable a previously suspended trigger.",
            examples: &[("orchestrator trigger resume nightly-qa", "Resume a trigger")],
        },
        GuideEntry {
            command: "trigger fire",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Manually fire a trigger",
            description: "Create a task as if the trigger fired. Supports an optional JSON \
                          payload to simulate webhook bodies.",
            examples: &[
                ("orchestrator trigger fire nightly-qa", "Fire a trigger now"),
                (
                    "orchestrator trigger fire webhook-handler --payload '{\"event\":\"push\"}'",
                    "Fire with a simulated payload",
                ),
            ],
        },
    ]
}

fn daemon_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "daemon stop",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Stop the running daemon",
            description: "Send SIGTERM to the daemon for graceful shutdown.",
            examples: &[("orchestrator daemon stop", "Stop the daemon")],
        },
        GuideEntry {
            command: "daemon status",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Show daemon status",
            description: "Check whether the daemon is running and display its PID.",
            examples: &[("orchestrator daemon status", "Check daemon status")],
        },
        GuideEntry {
            command: "daemon maintenance",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Enable or disable maintenance mode",
            description: "Maintenance mode blocks new task creation while existing tasks continue.",
            examples: &[
                (
                    "orchestrator daemon maintenance --enable",
                    "Enable maintenance mode",
                ),
                (
                    "orchestrator daemon maintenance --disable",
                    "Disable maintenance mode",
                ),
            ],
        },
    ]
}

fn system_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "init",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Initialize orchestrator runtime",
            description: "Set up the runtime directory and initial configuration.",
            examples: &[
                ("orchestrator init", "Initialize in default location"),
                (
                    "orchestrator init /custom/path",
                    "Initialize in a custom path",
                ),
            ],
        },
        GuideEntry {
            command: "check",
            alias: Some("ck"),
            category: GuideCategory::SystemAdmin,
            summary: "Preflight check",
            description: "Validate configuration and connectivity. Optionally filter by workflow.",
            examples: &[
                ("orchestrator check", "Run preflight checks"),
                (
                    "orchestrator check --workflow my-wf",
                    "Check a specific workflow",
                ),
                (
                    "orchestrator check -p my-proj -o json",
                    "Check with project filter (JSON)",
                ),
            ],
        },
        GuideEntry {
            command: "debug",
            alias: Some("dbg"),
            category: GuideCategory::SystemAdmin,
            summary: "System debug info",
            description: "Display configuration and daemon debug information. \
                          Use --component daemon for daemon-specific status. \
                          See also: debug sandbox-probe for local sandbox testing.",
            examples: &[
                ("orchestrator debug", "Show system debug info"),
                (
                    "orchestrator debug --component daemon",
                    "Show daemon status",
                ),
            ],
        },
        GuideEntry {
            command: "debug sandbox-probe write-file",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Write a file to a target path (sandbox probe)",
            description: "Local sandbox probe: write a file to verify filesystem access under \
                          sandbox constraints. Does not contact the daemon.",
            examples: &[(
                "orchestrator debug sandbox-probe write-file --path /tmp/test.txt --contents hello",
                "Write a probe file",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe open-files",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Open many files at once (sandbox probe)",
            description: "Local sandbox probe: stress-test file descriptor limits.",
            examples: &[(
                "orchestrator debug sandbox-probe open-files --count 512",
                "Attempt to open 512 files",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe cpu-burn",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Burn CPU in a tight loop (sandbox probe)",
            description: "Local sandbox probe: verify CPU resource limits under sandbox.",
            examples: &[(
                "orchestrator debug sandbox-probe cpu-burn",
                "Run CPU burn test",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe alloc-memory",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Allocate memory (sandbox probe)",
            description: "Local sandbox probe: allocate memory in chunks to test memory limits.",
            examples: &[(
                "orchestrator debug sandbox-probe alloc-memory --chunk-mb 8 --total-mb 256",
                "Allocate 256 MiB in 8 MiB chunks",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe spawn-children",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Spawn many child processes (sandbox probe)",
            description: "Local sandbox probe: test process limit enforcement under sandbox.",
            examples: &[(
                "orchestrator debug sandbox-probe spawn-children --count 64 --sleep-secs 10",
                "Spawn 64 idle children for 10 seconds",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe dns-resolve",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Resolve a hostname through DNS (sandbox probe)",
            description: "Local sandbox probe: verify network/DNS access under sandbox.",
            examples: &[(
                "orchestrator debug sandbox-probe dns-resolve --host example.com --port 443",
                "Resolve example.com",
            )],
        },
        GuideEntry {
            command: "debug sandbox-probe tcp-connect",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Open a TCP connection (sandbox probe)",
            description: "Local sandbox probe: test outbound TCP connectivity under sandbox.",
            examples: &[(
                "orchestrator debug sandbox-probe tcp-connect --host example.com --port 443 --timeout-secs 3",
                "TCP connect to example.com:443",
            )],
        },
        GuideEntry {
            command: "version",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Show version",
            description: "Display CLI version, build hash, and control-plane compatibility.",
            examples: &[
                ("orchestrator version", "Show version info"),
                ("orchestrator version --json", "Show version as JSON"),
            ],
        },
        GuideEntry {
            command: "guide",
            alias: Some("gd"),
            category: GuideCategory::SystemAdmin,
            summary: "Show CLI command reference with examples",
            description: "Self-describing guide for all CLI commands. Filter by command name \
                          or category. Supports markdown and JSON output. AI agents should use \
                          this command as their primary CLI reference.",
            examples: &[
                ("orchestrator guide", "Show full categorized reference"),
                ("orchestrator guide task", "Filter by command name"),
                (
                    "orchestrator guide --category resource",
                    "Filter by category",
                ),
                (
                    "orchestrator guide --format json",
                    "Machine-readable JSON output",
                ),
            ],
        },
        GuideEntry {
            command: "qa doctor",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Show observability health metrics",
            description: "Display health metrics from task_execution_metrics for QA observability.",
            examples: &[
                ("orchestrator qa doctor", "Show health metrics"),
                (
                    "orchestrator qa doctor -o json",
                    "Show health metrics as JSON",
                ),
            ],
        },
    ]
}

fn tool_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "tool webhook-verify-hmac",
            alias: None,
            category: GuideCategory::BuiltinTools,
            summary: "Verify an HMAC signature",
            description: "Validate a webhook request body against an HMAC signature. Exits 0 \
                          if valid, 1 if invalid. Used in CRD plugin scripts.",
            examples: &[(
                "orchestrator tool webhook-verify-hmac --secret $SECRET --body \"$BODY\" --signature $SIG",
                "Verify a webhook HMAC signature",
            )],
        },
        GuideEntry {
            command: "tool payload-extract",
            alias: None,
            category: GuideCategory::BuiltinTools,
            summary: "Extract a value from JSON",
            description: "Read JSON from stdin and extract a value using a dot-separated path. \
                          Used in CRD plugin scripts.",
            examples: &[(
                "echo '{\"event\":{\"type\":\"push\"}}' | orchestrator tool payload-extract --path event.type",
                "Extract a nested JSON value",
            )],
        },
        GuideEntry {
            command: "tool secret-rotate",
            alias: None,
            category: GuideCategory::BuiltinTools,
            summary: "Rotate a key in a SecretStore",
            description: "Update the value of a key in a SecretStore. Requires a running daemon.",
            examples: &[(
                "orchestrator tool secret-rotate my-secrets api_key --value NEW_KEY",
                "Rotate a secret key value",
            )],
        },
    ]
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Collect all guide entries from every domain.
fn all_entries() -> Vec<GuideEntry> {
    let mut entries = Vec::with_capacity(64);
    entries.extend(resource_entries());
    entries.extend(task_entries());
    entries.extend(run_entries());
    entries.extend(agent_entries());
    entries.extend(store_entries());
    entries.extend(manifest_entries());
    entries.extend(secret_entries());
    entries.extend(db_entries());
    entries.extend(event_entries());
    entries.extend(trigger_entries());
    entries.extend(daemon_entries());
    entries.extend(system_entries());
    entries.extend(tool_entries());
    entries
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

fn filter_entries(
    entries: Vec<GuideEntry>,
    command_filter: Option<&str>,
    category_filter: Option<&str>,
) -> Vec<GuideEntry> {
    entries
        .into_iter()
        .filter(|e| {
            if let Some(cf) = command_filter {
                let lower = cf.to_ascii_lowercase();
                let cmd_match = e.command.to_ascii_lowercase().starts_with(&lower)
                    || e.command.to_ascii_lowercase().contains(&lower);
                let alias_match = e
                    .alias
                    .map(|a| {
                        a.to_ascii_lowercase().starts_with(&lower)
                            || a.to_ascii_lowercase().contains(&lower)
                    })
                    .unwrap_or(false);
                if !cmd_match && !alias_match {
                    return false;
                }
            }
            if let Some(cat) = category_filter {
                if !e.category.matches(cat) {
                    return false;
                }
            }
            true
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_markdown(entries: &[GuideEntry]) -> String {
    use std::collections::BTreeMap;

    // Group by category, sorted by canonical order.
    let mut groups: BTreeMap<u8, (GuideCategory, Vec<&GuideEntry>)> = BTreeMap::new();
    for entry in entries {
        groups
            .entry(entry.category.sort_key())
            .or_insert_with(|| (entry.category, Vec::new()))
            .1
            .push(entry);
    }

    let mut out = String::with_capacity(4096);
    out.push_str("# orchestrator CLI Guide\n\n");

    for (_key, (category, group)) in &groups {
        out.push_str(&format!("## {category}\n\n"));
        for entry in group {
            // Heading
            if let Some(alias) = entry.alias {
                out.push_str(&format!("### {} (alias: {})\n", entry.command, alias));
            } else {
                out.push_str(&format!("### {}\n", entry.command));
            }
            out.push_str(&format!("{}\n\n", entry.summary));

            // Description
            out.push_str(&format!("{}\n\n", entry.description));

            // Examples
            if !entry.examples.is_empty() {
                out.push_str("**Examples:**\n");
                for (cmd, explanation) in entry.examples {
                    out.push_str(&format!("```\n{cmd}\n```\n{explanation}\n\n"));
                }
            }
        }
    }

    out
}

fn render_json(entries: &[GuideEntry]) -> Result<String> {
    Ok(serde_json::to_string_pretty(entries)?)
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Entry point for `orchestrator guide`.
pub fn dispatch(
    command_filter: Option<String>,
    category: Option<String>,
    format: GuideFormat,
) -> Result<()> {
    let entries = all_entries();
    let filtered = filter_entries(entries, command_filter.as_deref(), category.as_deref());

    if filtered.is_empty() {
        let mut msg = String::from("No commands matched");
        if let Some(cf) = &command_filter {
            msg.push_str(&format!(" command filter '{cf}'"));
        }
        if let Some(cat) = &category {
            msg.push_str(&format!(" category filter '{cat}'"));
        }
        msg.push_str(". Run `orchestrator guide` to see all available commands.");
        println!("{msg}");
        return Ok(());
    }

    match format {
        GuideFormat::Markdown => print!("{}", render_markdown(&filtered)),
        GuideFormat::Json => println!("{}", render_json(&filtered)?),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Compile-time exhaustiveness guard
// ---------------------------------------------------------------------------

/// Compile-time guard: every [`Commands`] variant must be accounted for.
///
/// When a new variant is added to `Commands`, this match becomes non-exhaustive
/// and the build fails — pointing the developer to add guide entries.
#[cfg(test)]
fn _exhaustiveness_guard(cmd: crate::Commands) {
    match cmd {
        crate::Commands::Apply { .. } => {}
        crate::Commands::Get { .. } => {}
        crate::Commands::Describe { .. } => {}
        crate::Commands::Delete { .. } => {}
        crate::Commands::Task(_) => {}
        crate::Commands::Store(_) => {}
        crate::Commands::Debug { .. } => {}
        crate::Commands::Check { .. } => {}
        crate::Commands::Init { .. } => {}
        crate::Commands::Secret(_) => {}
        crate::Commands::Db(_) => {}
        crate::Commands::Manifest(_) => {}
        crate::Commands::Agent(_) => {}
        crate::Commands::Event(_) => {}
        crate::Commands::Trigger(_) => {}
        crate::Commands::Qa(_) => {}
        crate::Commands::Daemon(_) => {}
        crate::Commands::Tool(_) => {}
        crate::Commands::Version { .. } => {}
        crate::Commands::Guide { .. } => {}
        crate::Commands::Run { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_entries_covers_all_categories() {
        let entries = all_entries();
        let cats: HashSet<_> = entries.iter().map(|e| e.category).collect();
        // Every GuideCategory variant must have at least one entry.
        assert!(cats.contains(&GuideCategory::ResourceManagement));
        assert!(cats.contains(&GuideCategory::TaskLifecycle));
        assert!(cats.contains(&GuideCategory::WorkflowExecution));
        assert!(cats.contains(&GuideCategory::AgentManagement));
        assert!(cats.contains(&GuideCategory::StoreOperations));
        assert!(cats.contains(&GuideCategory::SystemAdmin));
        assert!(cats.contains(&GuideCategory::Security));
        assert!(cats.contains(&GuideCategory::Observability));
        assert!(cats.contains(&GuideCategory::Trigger));
        assert!(cats.contains(&GuideCategory::WorkflowAuthoring));
        assert!(cats.contains(&GuideCategory::BuiltinTools));
    }

    #[test]
    fn no_duplicate_commands() {
        let entries = all_entries();
        let mut seen = HashSet::new();
        for e in &entries {
            assert!(
                seen.insert(e.command),
                "Duplicate guide entry for command: {}",
                e.command
            );
        }
    }

    #[test]
    fn all_entries_have_examples() {
        for e in all_entries() {
            assert!(
                !e.examples.is_empty(),
                "Guide entry '{}' has no examples",
                e.command
            );
        }
    }

    #[test]
    fn filter_by_command_name() {
        let entries = all_entries();
        let filtered = filter_entries(entries, Some("task"), None);
        assert!(!filtered.is_empty());
        for e in &filtered {
            assert!(
                e.command.contains("task") || e.alias.map(|a| a.contains("task")).unwrap_or(false),
                "Entry '{}' should not match 'task'",
                e.command
            );
        }
    }

    #[test]
    fn filter_by_category() {
        let entries = all_entries();
        let filtered = filter_entries(entries, None, Some("resource"));
        assert!(!filtered.is_empty());
        for e in &filtered {
            assert_eq!(e.category, GuideCategory::ResourceManagement);
        }
    }

    #[test]
    fn render_markdown_contains_headings() {
        let entries = all_entries();
        let md = render_markdown(&entries);
        assert!(md.contains("# orchestrator CLI Guide"));
        assert!(md.contains("## Resource Management"));
        assert!(md.contains("### apply"));
    }

    #[test]
    fn render_json_is_valid() {
        let entries = all_entries();
        let json = render_json(&entries).expect("JSON render should succeed");
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&json).expect("JSON should parse");
        assert!(!parsed.is_empty());
    }

    #[test]
    fn guide_subcommand_parses() {
        use crate::Cli;
        use clap::Parser;
        let cli = Cli::try_parse_from(["orchestrator", "guide"]).expect("guide should parse");
        assert!(matches!(cli.command, crate::Commands::Guide { .. }));
    }

    #[test]
    fn guide_subcommand_with_filter() {
        use crate::Cli;
        use clap::Parser;
        let cli = Cli::try_parse_from(["orchestrator", "guide", "task"])
            .expect("guide task should parse");
        match cli.command {
            crate::Commands::Guide {
                command_filter,
                category,
                ..
            } => {
                assert_eq!(command_filter.as_deref(), Some("task"));
                assert!(category.is_none());
            }
            _ => panic!("expected Guide variant"),
        }
    }

    #[test]
    fn guide_subcommand_with_category() {
        use crate::Cli;
        use clap::Parser;
        let cli = Cli::try_parse_from(["orchestrator", "guide", "--category", "resource"])
            .expect("guide --category should parse");
        match cli.command {
            crate::Commands::Guide { category, .. } => {
                assert_eq!(category.as_deref(), Some("resource"));
            }
            _ => panic!("expected Guide variant"),
        }
    }
}
