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
    /// source connection *
    SourceIntegration,
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
            Self::SourceIntegration => write!(f, "Source Integrations"),
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
            Self::SourceIntegration => 7,
            Self::Observability => 8,
            Self::Security => 9,
            Self::SystemAdmin => 10,
            Self::BuiltinTools => 11,
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
            Self::SourceIntegration => "integration",
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
        GuideEntry {
            command: "task timeline",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Show the semantic process timeline",
            description: "Browse goal, execution, evidence, failure, and state transitions with stable pagination.",
            examples: &[
                (
                    "orchestrator task timeline <task_id>",
                    "Show the first timeline page",
                ),
                (
                    "orchestrator task timeline <task_id> --category failure --follow",
                    "Follow failure entries",
                ),
                (
                    "orchestrator task timeline <task_id> -o json",
                    "Output structured JSON",
                ),
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

fn attention_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "attention list",
            alias: Some("attn ls"),
            category: GuideCategory::TaskLifecycle,
            summary: "List cross-task decisions and blockers",
            description: "Show only workflow conditions that need human attention, ordered by severity and ownership. The full --kind vocabulary is the generated routing table in docs/guide/03-workflow-configuration.md (Where Failures Go).",
            examples: &[
                ("orchestrator attention list", "List the active inbox"),
                (
                    "orchestrator attention list --assignee me",
                    "Show items assigned to the current authenticated actor",
                ),
                (
                    "orchestrator attention list --state resolved -o json",
                    "Audit resolved decisions",
                ),
            ],
        },
        GuideEntry {
            command: "attention get",
            alias: Some("attn get"),
            category: GuideCategory::TaskLifecycle,
            summary: "Inspect one attention item",
            description: "Show the redacted condition, optimistic version, task context, and safe allowlisted actions.",
            examples: &[("orchestrator attention get <id>", "Inspect one item")],
        },
        GuideEntry {
            command: "attention claim",
            alias: Some("attn claim"),
            category: GuideCategory::TaskLifecycle,
            summary: "Claim an open attention item",
            description: "Take ownership of an open item through an authenticated, \
                          version-checked and idempotent queue mutation.",
            examples: &[
                (
                    "orchestrator attention claim <id> --expected-version 1",
                    "Claim an open item",
                ),
                (
                    "orchestrator attention claim <id> --expected-version 1 --idempotency-key claim-1 -o json",
                    "Claim with a retry-stable idempotency key",
                ),
            ],
        },
        GuideEntry {
            command: "attention snooze",
            alias: Some("attn snooze"),
            category: GuideCategory::TaskLifecycle,
            summary: "Snooze an open or claimed item",
            description: "Defer an item until an RFC3339 deadline through an authenticated, \
                          version-checked and idempotent queue mutation.",
            examples: &[(
                "orchestrator attention snooze <id> --expected-version 2 --until 2026-07-13T09:00:00Z",
                "Snooze until an RFC3339 deadline",
            )],
        },
        GuideEntry {
            command: "attention resolve",
            alias: Some("attn resolve"),
            category: GuideCategory::TaskLifecycle,
            summary: "Resolve an attention item",
            description: "Close an item with an audit reason through an authenticated, \
                          version-checked and idempotent queue mutation.",
            examples: &[(
                "orchestrator attention resolve <id> --expected-version 2 --reason reviewed",
                "Resolve with an audit reason",
            )],
        },
        GuideEntry {
            command: "attention action",
            alias: Some("attn action"),
            category: GuideCategory::TaskLifecycle,
            summary: "Execute an allowlisted recovery or decision action",
            description: "Reserve and execute only an action advertised by the item, such as retry_failed_item or resume_task.",
            examples: &[(
                "orchestrator attention action <id> resume_task --expected-version 1",
                "Resume and resolve a stalled task",
            )],
        },
        GuideEntry {
            command: "attention follow",
            alias: Some("attn follow"),
            category: GuideCategory::TaskLifecycle,
            summary: "Follow monotonic inbox changes",
            description: "Stream upsert and remove deltas from a durable change sequence for reconnect-safe clients.",
            examples: &[(
                "orchestrator attention follow --after 42",
                "Resume a queue stream",
            )],
        },
    ]
}

fn source_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "source list",
            alias: Some("src ls"),
            category: GuideCategory::Observability,
            summary: "List recent source events",
            description: "Query provider-neutral source events with routing state, task, and project filters without exposing raw provider payloads.",
            examples: &[
                (
                    "orchestrator source list --state failed",
                    "List replay candidates",
                ),
                (
                    "orchestrator source list --project demo --limit 20 -o json",
                    "List the newest project events as JSON",
                ),
            ],
        },
        GuideEntry {
            command: "source get",
            alias: Some("src get"),
            category: GuideCategory::Observability,
            summary: "Get one source event",
            description: "Inspect one normalized event's routing state, provenance, and the resolved process without exposing raw provider payloads.",
            examples: &[(
                "orchestrator source get <source-event-id>",
                "Inspect one normalized event",
            )],
        },
        GuideEntry {
            command: "source ingest",
            alias: Some("src ingest"),
            category: GuideCategory::Trigger,
            summary: "Ingest a provider-neutral source fixture",
            description: "Durably insert an authenticated normalized event for adapter development and non-Slack integration testing. Runtime source ingestion must be enabled.",
            examples: &[(
                "orchestrator source ingest --project demo --file event.json",
                "Ingest a normalized source event",
            )],
        },
        GuideEntry {
            command: "source bindings",
            alias: Some("src bindings"),
            category: GuideCategory::TaskLifecycle,
            summary: "List source bindings for one task",
            description: "Show the provider conversation coordinates correlated with an orchestrator task, including primary, related, and notification_target bindings.",
            examples: &[(
                "orchestrator source bindings <task-id>",
                "List task provenance bindings",
            )],
        },
        GuideEntry {
            command: "source bind",
            alias: Some("src bind"),
            category: GuideCategory::TaskLifecycle,
            summary: "Bind provider conversation coordinates to a task",
            description: "Correlate trusted provider conversation coordinates with an orchestrator task using primary, related, or notification_target bindings.",
            examples: &[(
                "orchestrator source bind --project demo --task <task-id> --provider fixture --installation install-1 --conversation C1 --thread T1 --source-event <event-id>",
                "Create a trusted binding",
            )],
        },
        GuideEntry {
            command: "source replay",
            alias: Some("src replay"),
            category: GuideCategory::SystemAdmin,
            summary: "Replay a failed or attention-blocked source route",
            description: "Admin-only recovery for generic source events. Events linked to a badge automation route must use source automation replay so route version, generation, and task fences remain authoritative.",
            examples: &[(
                "orchestrator source replay <source-event-id>",
                "Requeue one failed route",
            )],
        },
        GuideEntry {
            command: "source automation list",
            alias: None,
            category: GuideCategory::Observability,
            summary: "List badge automation routes",
            description: "Query safe route projections with bounded keyset pagination. Operational output omits Slack message coordinates, bodies, credentials, and permalinks.",
            examples: &[
                (
                    "orchestrator source automation list --project demo --state needs_attention -o json",
                    "List actionable routes",
                ),
                (
                    "orchestrator source automation list --page-size 20 --page-token <token>",
                    "Continue a paginated listing",
                ),
            ],
        },
        GuideEntry {
            command: "source automation get",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Inspect one badge automation route",
            description: "Show one durable route's safe projection and bounded attempt history without exposing Slack message coordinates, bodies, credentials, or permalinks.",
            examples: &[(
                "orchestrator source automation get <route-id> --attempt-limit 20",
                "Inspect one route and retry history",
            )],
        },
        GuideEntry {
            command: "source automation watch",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Follow badge automation route transitions",
            description: "Resume a monotonic route transition stream from a durable change sequence for reconnect-safe clients.",
            examples: &[(
                "orchestrator source automation watch --project demo --after 42",
                "Reconnect to route transitions",
            )],
        },
        GuideEntry {
            command: "source automation simulate",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Preview badge matching and task rendering",
            description: "Run the same matcher and renderer used by live routing against caller-supplied safe evidence. Simulation does not read credentials, call Slack, reserve a route, create Attention, or create a task.",
            examples: &[(
                "orchestrator source automation simulate --project demo --installation T1 --reaction agent-analyze --channel C1 --actor U1 --message-url https://acme.slack.com/archives/C1/p123 --target-id C1:1.23",
                "Preview the selected binding and rendered task",
            )],
        },
        GuideEntry {
            command: "source automation replay",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Replay an actionable automation route",
            description: "Audited operator control requiring a reason, optimistic route version, and idempotency key. Replay resumes from the durable checkpoint and keeps the pinned generation unless --adopt-current-config is explicitly requested.",
            examples: &[
                (
                    "orchestrator source automation replay <route-id> --expected-version 7 --reason 'credential rotated' --idempotency-key replay-20260717",
                    "Replay from the durable checkpoint",
                ),
                (
                    "orchestrator source automation replay <route-id> --expected-version 7 --reason 'config fixed' --idempotency-key replay-2 --adopt-current-config",
                    "Replay against the current configuration",
                ),
            ],
        },
        GuideEntry {
            command: "source automation ignore",
            alias: None,
            category: GuideCategory::SystemAdmin,
            summary: "Deliberately ignore an actionable automation route",
            description: "Audited operator control requiring a reason, optimistic route version, and idempotency key. Ignore closes the route without task creation and resolves the matching Attention item.",
            examples: &[(
                "orchestrator source automation ignore <route-id> --expected-version 8 --reason 'obsolete request' --idempotency-key ignore-20260717",
                "Close a route without task creation",
            )],
        },
        GuideEntry {
            command: "source automation status",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Report source automation worker health",
            description: "Show backlog, oldest age, active leases, retrying routes, Attention count, and low-cardinality failure families without exposing installation or message identifiers.",
            examples: &[(
                "orchestrator source automation status --project demo -o json",
                "Inspect route worker health",
            )],
        },
        GuideEntry {
            command: "source route",
            alias: Some("src route"),
            category: GuideCategory::Observability,
            summary: "Inspect one protected automation route",
            description: "Show the protected automation route resolved for a source event, including its Slack deep link.",
            examples: &[(
                "orchestrator source route <source-event-id>",
                "Inspect the route for one source event",
            )],
        },
        GuideEntry {
            command: "source template preview",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Preview a governed source-to-task template",
            description: "Render a side-effect-free sample using the daemon's active configuration. Preview never calls the provider or creates a task.",
            examples: &[(
                "orchestrator source template preview badge-default --provider slack --installation T1 --message-url https://acme.slack.com/archives/C1/p123",
                "Render a sample from a governed template",
            )],
        },
        GuideEntry {
            command: "source binding simulate",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Simulate deterministic binding matching",
            description: "Run governed source-to-task binding matching against caller-supplied evidence without side effects or provider API calls.",
            examples: &[(
                "orchestrator source binding simulate --project demo --installation T1 --reaction agent-analyze --channel C1 --actor U1",
                "Preview which binding matches an event",
            )],
        },
        GuideEntry {
            command: "source binding suspend",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Suspend a source binding immediately",
            description: "Stop a governed source-to-task binding from matching new events. The mutation is project-scoped and takes effect immediately.",
            examples: &[(
                "orchestrator source binding suspend badge-default --project demo",
                "Suspend a binding",
            )],
        },
        GuideEntry {
            command: "source binding resume",
            alias: None,
            category: GuideCategory::Trigger,
            summary: "Resume a suspended source binding",
            description: "Re-enable a suspended source-to-task binding after conflict validation against the currently active bindings.",
            examples: &[(
                "orchestrator source binding resume badge-default --project demo",
                "Resume a binding",
            )],
        },
    ]
}

fn source_connection_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "source connection list",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "List safe SourceConnection projections",
            description: "Show provider connections for a project without exposing credentials. Disconnected connections are hidden unless requested.",
            examples: &[
                (
                    "orchestrator source connection list -p demo",
                    "List active connections",
                ),
                (
                    "orchestrator source connection list -p demo --include-disconnected -o json",
                    "Include disconnected connections as JSON",
                ),
            ],
        },
        GuideEntry {
            command: "source connection get",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Get one SourceConnection",
            description: "Inspect one connection's safe projection, lifecycle state, and version without exposing credentials.",
            examples: &[(
                "orchestrator source connection get <connection-id> -p demo",
                "Inspect one connection",
            )],
        },
        GuideEntry {
            command: "source connection watch",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Follow monotonic SourceConnection changes",
            description: "Stream connection change deltas from a durable change sequence for reconnect-safe clients.",
            examples: &[(
                "orchestrator source connection watch -p demo --after 42",
                "Reconnect to connection changes",
            )],
        },
        GuideEntry {
            command: "source connection catalog",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Show provisioning capabilities",
            description: "Report which managed and manual provisioning modes the daemon supports for each provider.",
            examples: &[(
                "orchestrator source connection catalog",
                "Show managed/manual provisioning capabilities",
            )],
        },
        GuideEntry {
            command: "source connection connect",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Start the official Slack App OAuth flow",
            description: "Create an OAuth installation intent and open the authorization URL. Use --no-open to print the URL instead of launching a browser.",
            examples: &[(
                "orchestrator source connection connect -p demo --reason 'onboard workspace' --idempotency-key connect-1",
                "Start the OAuth flow",
            )],
        },
        GuideEntry {
            command: "source connection provision-dedicated",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Provision a workspace-owned private Slack App",
            description: "Validate and provision a dedicated App from a Slack configuration token read on stdin. The audited mutation requires a reason and idempotency key.",
            examples: &[(
                "orchestrator source connection provision-dedicated -p demo --config-token-stdin --reason 'private app' --idempotency-key prov-1",
                "Provision a dedicated App",
            )],
        },
        GuideEntry {
            command: "source connection dedicated-status",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Inspect a dedicated App provisioning checkpoint",
            description: "Show the durable checkpoint state of an in-progress dedicated App provisioning.",
            examples: &[(
                "orchestrator source connection dedicated-status <provisioning-id> -p demo",
                "Inspect a provisioning checkpoint",
            )],
        },
        GuideEntry {
            command: "source connection dedicated-resume",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Resume a dedicated App provisioning",
            description: "Resume credential handoff or approve a reviewed dedicated App preview from its durable checkpoint.",
            examples: &[(
                "orchestrator source connection dedicated-resume <provisioning-id> -p demo --reason 'approve preview' --idempotency-key resume-1",
                "Resume a provisioning checkpoint",
            )],
        },
        GuideEntry {
            command: "source connection dedicated-abandon",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Abandon a dedicated App provisioning",
            description: "Abandon a non-terminal dedicated App provisioning checkpoint through an audited mutation.",
            examples: &[(
                "orchestrator source connection dedicated-abandon <provisioning-id> -p demo --reason 'wrong workspace' --idempotency-key abandon-1",
                "Abandon a provisioning checkpoint",
            )],
        },
        GuideEntry {
            command: "source connection dedicated-upgrade",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Upgrade an existing dedicated App manifest",
            description: "Review and apply the fixed manifest to an existing dedicated App. Preview first, then re-run with --approve to apply.",
            examples: &[(
                "orchestrator source connection dedicated-upgrade <connection-id> -p demo --expected-version 3 --config-token-stdin --approve --reason 'apply manifest fix' --idempotency-key upgrade-1",
                "Apply the reviewed manifest upgrade",
            )],
        },
        GuideEntry {
            command: "source connection migrate-to-shared",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Migrate a dedicated App to the official App",
            description: "Start a reviewed dedicated-to-official App migration through an audited, version-checked mutation.",
            examples: &[(
                "orchestrator source connection migrate-to-shared <connection-id> -p demo --expected-version 3 --reason 'move to official app' --idempotency-key migrate-1",
                "Start a reviewed migration",
            )],
        },
        GuideEntry {
            command: "source connection dedicated-delete",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Delete a disconnected workspace-owned Slack App",
            description: "Permanently delete a disconnected dedicated App. Requires the App ID as confirmation plus an audited reason and idempotency key.",
            examples: &[(
                "orchestrator source connection dedicated-delete <connection-id> -p demo --expected-version 5 --app-id-confirmation A0123 --reason 'decommission' --idempotency-key delete-1",
                "Permanently delete a dedicated App",
            )],
        },
        GuideEntry {
            command: "source connection status",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Poll or resume one OAuth intent",
            description: "Check the state of a pending OAuth installation intent and resume it when possible.",
            examples: &[(
                "orchestrator source connection status <intent-id> -p demo",
                "Poll an OAuth intent",
            )],
        },
        GuideEntry {
            command: "source connection cancel",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Cancel a pending OAuth intent",
            description: "Cancel an unfinished OAuth installation intent through an audited mutation.",
            examples: &[(
                "orchestrator source connection cancel <intent-id> -p demo --reason 'abandoned flow' --idempotency-key cancel-1",
                "Cancel a pending intent",
            )],
        },
        GuideEntry {
            command: "source connection reauthorize",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Start OAuth again for an existing connection",
            description: "Re-run the OAuth flow for an existing connection, for example after a scope change or credential revocation.",
            examples: &[(
                "orchestrator source connection reauthorize <connection-id> -p demo --expected-version 2 --reason 'scope update' --idempotency-key reauth-1",
                "Reauthorize a connection",
            )],
        },
        GuideEntry {
            command: "source connection disconnect",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Disconnect and destroy managed credentials",
            description: "Disconnect a connection and destroy its managed credentials through an audited, version-checked mutation.",
            examples: &[(
                "orchestrator source connection disconnect <connection-id> -p demo --expected-version 2 --reason 'offboard workspace' --idempotency-key disc-1",
                "Disconnect a connection",
            )],
        },
        GuideEntry {
            command: "source connection transfer",
            alias: None,
            category: GuideCategory::SourceIntegration,
            summary: "Transfer exclusive ownership to another daemon",
            description: "Move exclusive connection ownership to a different daemon through an audited, version-checked mutation.",
            examples: &[(
                "orchestrator source connection transfer <connection-id> -p demo --expected-version 2 --target-daemon-id <daemon-id> --reason 'move to prod daemon' --idempotency-key transfer-1",
                "Transfer connection ownership",
            )],
        },
    ]
}

fn audit_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "audit list",
            alias: Some("audit ls"),
            category: GuideCategory::Observability,
            summary: "List canonical action audit evidence",
            description: "Query project-scoped mutation evidence by actor, target, action, status, or time without exposing request bodies or secrets.",
            examples: &[
                (
                    "orchestrator audit list --project demo --status failed",
                    "List failed mutations",
                ),
                (
                    "orchestrator audit list --project demo --target-type attention_item -o json",
                    "Filter evidence by target kind",
                ),
            ],
        },
        GuideEntry {
            command: "audit get",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Inspect one action by request ID",
            description: "Retrieve the canonical envelope used to correlate transport authorization, domain mutation, and event evidence.",
            examples: &[(
                "orchestrator audit get req-123 --project demo",
                "Inspect one canonical request",
            )],
        },
    ]
}

fn handoff_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "handoff generate",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Generate an immutable task handoff snapshot",
            description: "Capture an immutable snapshot of a task at the latest or a selected \
                          event cursor, for transferring context between agents or sessions.",
            examples: &[
                (
                    "orchestrator handoff generate <task_id>",
                    "Snapshot at the latest event cursor",
                ),
                (
                    "orchestrator handoff generate <task_id> --cursor 42 -o json",
                    "Snapshot at a selected event cursor",
                ),
            ],
        },
        GuideEntry {
            command: "handoff get",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Get a previously generated handoff snapshot",
            description: "Retrieve an immutable handoff snapshot by ID.",
            examples: &[(
                "orchestrator handoff get <handoff_id>",
                "Retrieve one snapshot",
            )],
        },
    ]
}

fn resume_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "resume boundaries",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "List logical resume boundaries",
            description: "Show a task's logical boundaries and their side-effect \
                          classifications, the starting point for a safe logical resume.",
            examples: &[
                (
                    "orchestrator resume boundaries <task_id>",
                    "List boundaries for a task",
                ),
                (
                    "orchestrator resume boundaries <task_id> -o json",
                    "List boundaries as JSON",
                ),
            ],
        },
        GuideEntry {
            command: "resume plan",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Persist an expiring resume consequence preview",
            description: "Create an expiring consequence preview for resuming at a boundary \
                          without changing task or workspace state.",
            examples: &[(
                "orchestrator resume plan <task_id> --boundary <boundary_id> --mode <mode>",
                "Preview the consequences of a resume",
            )],
        },
        GuideEntry {
            command: "resume execute",
            alias: None,
            category: GuideCategory::TaskLifecycle,
            summary: "Execute a reviewed resume plan",
            description: "Execute a previously reviewed plan with stale-state protection. \
                          Requires the expected state version, an audit reason, and an \
                          idempotency key; elevated plans need --elevated-confirmation.",
            examples: &[(
                "orchestrator resume execute <plan_id> --expected-state-version 3 --reason 'reviewed preview' --idempotency-key resume-1",
                "Execute a reviewed plan",
            )],
        },
    ]
}

fn metrics_entries() -> Vec<GuideEntry> {
    vec![
        GuideEntry {
            command: "metrics process",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Query one Process Console snapshot",
            description: "Show a project-scoped Process Console metrics snapshot over a \
                          time window with a configurable bucket size.",
            examples: &[
                (
                    "orchestrator metrics process -p demo",
                    "Snapshot over the default 24h window",
                ),
                (
                    "orchestrator metrics process -p demo --window 7d --bucket 1d -o json",
                    "Weekly snapshot with daily buckets",
                ),
            ],
        },
        GuideEntry {
            command: "metrics prune",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Prune old optional metrics",
            description: "Delete optional metrics older than the retention threshold.",
            examples: &[(
                "orchestrator metrics prune --retention-days 30",
                "Prune metrics older than 30 days",
            )],
        },
        GuideEntry {
            command: "metrics rebuild",
            alias: None,
            category: GuideCategory::Observability,
            summary: "Rebuild materialized metrics rollups",
            description: "Rebuild retained materialized rollups for one project.",
            examples: &[(
                "orchestrator metrics rebuild -p demo",
                "Rebuild rollups for a project",
            )],
        },
    ]
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
        GuideEntry {
            command: "agent session list",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "List observable interactive agent sessions",
            description: "Filter daemon-authoritative sessions without exposing transport paths or command text.",
            examples: &[(
                "orchestrator agent session list --state detached -o json",
                "List detached sessions as bounded JSON",
            )],
        },
        GuideEntry {
            command: "agent session get",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Inspect one interactive agent session",
            description: "Show public lifecycle, process, and writer lease metadata for a session ID.",
            examples: &[(
                "orchestrator agent session get SESSION_ID -o json",
                "Inspect one session",
            )],
        },
        GuideEntry {
            command: "agent session attach",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Attach a reader or request the fenced writer lease",
            description: "Reader attachment is read-only; writer attachment requires operator authority and an enabled session-control policy.",
            examples: &[(
                "orchestrator agent session attach SESSION_ID --mode writer --client-id terminal-a",
                "Request exclusive writer control",
            )],
        },
        GuideEntry {
            command: "agent session read",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Read transcript chunks from a client-owned offset",
            description: "Use structured chunk output to commit next_offset before reconnecting a transcript stream.",
            examples: &[(
                "orchestrator agent session read SESSION_ID --offset 0 --chunks-json",
                "Read chunks with reconnect offsets",
            )],
        },
        GuideEntry {
            command: "agent session heartbeat",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Renew the current writer lease",
            description: "Only the current unexpired client and fencing token can extend a writer lease.",
            examples: &[(
                "orchestrator agent session heartbeat SESSION_ID --client-id terminal-a --fencing-token 1",
                "Renew a writer lease",
            )],
        },
        GuideEntry {
            command: "agent session send-input",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Send retry-safe bounded input to a live session",
            description: "Input requires the current fencing token and a retry-stable idempotency key.",
            examples: &[(
                "orchestrator agent session send-input SESSION_ID --client-id terminal-a --fencing-token 1 --text hello --idempotency-key input-1",
                "Send one idempotent input payload",
            )],
        },
        GuideEntry {
            command: "agent session detach",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Detach a session reader or writer",
            description: "Writer detach requires the exact current fencing token; stale tokens cannot release a new owner.",
            examples: &[(
                "orchestrator agent session detach SESSION_ID --mode writer --client-id terminal-a --fencing-token 1",
                "Release writer control",
            )],
        },
        GuideEntry {
            command: "agent session close",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Close a fingerprint-verified session process",
            description: "Close is session-ID addressed, version-aware, audited, and never authorizes a mutation by PID alone.",
            examples: &[(
                "orchestrator agent session close SESSION_ID --reason done --expected-version 2 --idempotency-key close-1",
                "Request a governed close",
            )],
        },
        GuideEntry {
            command: "agent session resolve",
            alias: None,
            category: GuideCategory::AgentManagement,
            summary: "Resolve a diagnostic PID to session resources",
            description: "PID resolution is read-only and never creates mutation authority.",
            examples: &[(
                "orchestrator agent session resolve --pid 1234 -o json",
                "Find sessions carrying a diagnostic PID",
            )],
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
            description: "Pause a trigger through an audited mutation. Matching unleased source automation routes are suspended immediately at installation scope while in-flight leases finish their bounded transition.",
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
            description: "Re-enable a previously suspended trigger and requeue routes suspended by that installation scope.",
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
            summary: "Show daemon status, or wait until it can serve",
            description: "Without flags, reads the PID file and probes the process — no \
                          connection, so it answers even for a daemon that cannot serve. \
                          With --wait-ready, polls until migrations, keyring and workers all \
                          report ready. A bound socket is not a daemon that can serve: the \
                          socket accepts connections before the worker pool has registered.",
            examples: &[
                (
                    "orchestrator daemon status",
                    "Check whether the daemon is running",
                ),
                (
                    "orchestrator daemon status --wait-ready --timeout 30",
                    "Block until the daemon can serve, for scripts that start it",
                ),
            ],
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
            summary: "Report runtime paths, and optionally create one workspace root",
            description: "An RPC to a running daemon, so it is not a setup step: the daemon creates the data directory and applies every migration before it binds its socket. With no argument it creates nothing and prints the resolved data directory and database path. Given a root, it creates that one directory. To wait for a daemon that can serve, use `daemon status --wait-ready`.",
            examples: &[
                (
                    "orchestrator init",
                    "Print the data directory and database path in use",
                ),
                (
                    "orchestrator init /custom/path",
                    "Create one workspace root at a custom path",
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

/// Guide topics that are not clap commands (reference material reachable via
/// `orchestrator guide <topic>`).
fn topic_entries() -> Vec<GuideEntry> {
    vec![GuideEntry {
        command: "error-codes",
        alias: None,
        category: GuideCategory::SystemAdmin,
        summary: "Bracketed machine error codes reference",
        description: "Errors and warnings carry bracketed machine codes such as \
                      [driver_config_invalid], [secret_value_placeholder_rejected], and the \
                      driver requirement family ([driver_multi_turn_required], ...). \
                      docs/guide/error-codes.md is the glossary: each code's meaning, \
                      trigger, and remedy. The glossary's entry set is compared against \
                      the source-derived set in CI, so it cannot go stale.",
        examples: &[
            (
                "orchestrator guide error-codes",
                "Show where the error-code glossary lives",
            ),
            (
                "less docs/guide/error-codes.md",
                "Read the glossary in a repository checkout",
            ),
        ],
    }]
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

/// Collect the guide entries that mirror real clap commands.
///
/// The command set of this list is asserted equal to
/// `crate::surface::visible_invocable_paths()` in tests, so it cannot drift
/// from the clap tree.
fn command_entries() -> Vec<GuideEntry> {
    let mut entries = Vec::with_capacity(128);
    entries.extend(resource_entries());
    entries.extend(task_entries());
    entries.extend(attention_entries());
    entries.extend(source_entries());
    entries.extend(source_connection_entries());
    entries.extend(audit_entries());
    entries.extend(handoff_entries());
    entries.extend(resume_entries());
    entries.extend(metrics_entries());
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

/// Collect all guide entries: real commands plus non-command topics.
fn all_entries() -> Vec<GuideEntry> {
    let mut entries = command_entries();
    entries.extend(topic_entries());
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
            if let Some(cat) = category_filter
                && !e.category.matches(cat)
            {
                return false;
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

    for (category, group) in groups.values() {
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
        assert!(cats.contains(&GuideCategory::SourceIntegration));
        assert!(cats.contains(&GuideCategory::WorkflowAuthoring));
        assert!(cats.contains(&GuideCategory::BuiltinTools));
    }

    /// The guide's command set must equal the clap tree's visible invocable
    /// paths — bidirectionally. On failure, both one-sided diffs are printed
    /// so drift is diagnosable.
    #[test]
    fn guide_matches_clap_leaves() {
        let guide: HashSet<String> = command_entries()
            .iter()
            .map(|e| e.command.to_string())
            .collect();
        let clap: HashSet<String> = crate::surface::visible_invocable_paths()
            .into_iter()
            .collect();

        let mut missing_in_guide: Vec<&String> = clap.difference(&guide).collect();
        let mut unknown_in_guide: Vec<&String> = guide.difference(&clap).collect();
        missing_in_guide.sort();
        unknown_in_guide.sort();

        assert!(
            missing_in_guide.is_empty() && unknown_in_guide.is_empty(),
            "guide/clap drift:\n  missing in guide (add entries): {missing_in_guide:?}\n  \
             unknown in guide (not a clap path): {unknown_in_guide:?}"
        );
    }

    /// Topic pseudo-entries must never shadow a real command path.
    #[test]
    fn guide_topics_do_not_collide_with_commands() {
        let clap: HashSet<String> = crate::surface::visible_invocable_paths()
            .into_iter()
            .collect();
        for topic in topic_entries() {
            assert!(
                !clap.contains(topic.command),
                "guide topic '{}' collides with a clap command path",
                topic.command
            );
        }
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
