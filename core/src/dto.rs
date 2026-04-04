use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Payload accepted by task-creation APIs.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CreateTaskPayload {
    /// Optional human-readable task name.
    pub name: Option<String>,
    /// Optional task goal shown in task summaries and prompts.
    pub goal: Option<String>,
    /// Project identifier used to scope task execution.
    pub project_id: Option<String>,
    /// Workspace identifier used to resolve files and resources.
    pub workspace_id: Option<String>,
    /// Workflow identifier to execute for the task.
    pub workflow_id: Option<String>,
    /// Explicit target files to associate with the task.
    pub target_files: Option<Vec<String>>,
    /// Parent task identifier when the task was spawned from another task.
    pub parent_task_id: Option<String>,
    /// Human-readable reason for task spawning.
    pub spawn_reason: Option<String>,
    /// Step IDs to execute (empty/None = all steps).
    pub step_filter: Option<Vec<String>>,
    /// Ad-hoc pipeline variables injected at task start.
    pub initial_vars: Option<HashMap<String, String>>,
}

/// Snapshot of the active orchestrator configuration exposed by read APIs.
#[derive(Debug, Serialize)]
pub struct ConfigOverview {
    /// Fully materialized configuration object.
    pub config: crate::config::OrchestratorConfig,
    /// YAML serialization of [`Self::config`].
    pub yaml: String,
    /// Monotonic configuration version stored in persistence.
    pub version: i64,
    /// Timestamp when the configuration was last updated.
    pub updated_at: String,
}

/// Summary view returned by task listing APIs.
#[derive(Debug, Serialize)]
pub struct TaskSummary {
    /// Stable task identifier.
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Current task status.
    pub status: String,
    /// Timestamp when execution started.
    pub started_at: Option<String>,
    /// Timestamp when execution completed.
    pub completed_at: Option<String>,
    /// Goal or mission statement associated with the task.
    pub goal: String,
    /// Project scope for the task.
    pub project_id: String,
    /// Workspace scope for the task.
    pub workspace_id: String,
    /// Workflow chosen for execution.
    pub workflow_id: String,
    /// Files targeted by the task.
    pub target_files: Vec<String>,
    /// Total number of task items.
    pub total_items: i64,
    /// Number of finished task items.
    pub finished_items: i64,
    /// Number of failed task items.
    pub failed_items: i64,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Parent task identifier when spawned from another task.
    pub parent_task_id: Option<String>,
    /// Spawn reason inherited from the parent task relationship.
    pub spawn_reason: Option<String>,
    /// Spawn depth used to prevent runaway recursive task creation.
    pub spawn_depth: i64,
}

/// Payload for the Phase 3 direct-assembly `RunStep` RPC.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CreateRunStepPayload {
    /// Project identifier.
    pub project_id: Option<String>,
    /// Workspace identifier.
    pub workspace_id: Option<String>,
    /// StepTemplate name to execute.
    pub template: String,
    /// Required agent capability for the step.
    pub agent_capability: String,
    /// Optional execution profile override.
    pub execution_profile: Option<String>,
    /// Ad-hoc pipeline variables.
    pub initial_vars: Option<HashMap<String, String>>,
    /// Explicit target files.
    pub target_files: Option<Vec<String>>,
}

/// Detailed task-item record returned by task detail APIs.
#[derive(Debug, Serialize)]
pub struct TaskItemDto {
    /// Stable task-item identifier.
    pub id: String,
    /// Owning task identifier.
    pub task_id: String,
    /// Execution order within the task.
    pub order_no: i64,
    /// QA document path associated with the item.
    pub qa_file_path: String,
    /// Current item status.
    pub status: String,
    /// Relative paths to generated ticket files.
    pub ticket_files: Vec<String>,
    /// Parsed ticket payloads for convenience in API consumers.
    pub ticket_content: Vec<Value>,
    /// Whether the item still requires a fix phase.
    pub fix_required: bool,
    /// Whether the latest fix phase marked the item as fixed.
    pub fixed: bool,
    /// Last recorded error message for the item.
    pub last_error: String,
    /// Timestamp when item execution started.
    pub started_at: Option<String>,
    /// Timestamp when item execution completed.
    pub completed_at: Option<String>,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Persisted command-run record returned by task detail APIs.
#[derive(Debug, Serialize)]
pub struct CommandRunDto {
    /// Stable command-run identifier.
    pub id: String,
    /// Task-item identifier that owns the run.
    pub task_item_id: String,
    /// Pipeline phase that produced the run.
    pub phase: String,
    /// Command string that was executed.
    pub command: String,
    /// Pre-rendered command template before variable substitution.
    pub command_template: Option<String>,
    /// Working directory used for execution.
    pub cwd: String,
    /// Workspace identifier resolved for the run.
    pub workspace_id: String,
    /// Agent identifier assigned to the run.
    pub agent_id: String,
    /// Exit code when the process terminated normally.
    pub exit_code: Option<i64>,
    /// Relative path to captured stdout.
    pub stdout_path: String,
    /// Relative path to captured stderr.
    pub stderr_path: String,
    /// Timestamp when execution started.
    pub started_at: String,
    /// Timestamp when execution ended.
    pub ended_at: Option<String>,
    /// Whether the run was interrupted before completion.
    pub interrupted: bool,
}

/// Event row returned by task event queries.
#[derive(Debug, Serialize)]
pub struct EventDto {
    /// Database identifier for the event row.
    pub id: i64,
    /// Owning task identifier.
    pub task_id: String,
    /// Optional task-item identifier associated with the event.
    pub task_item_id: Option<String>,
    /// Event type label.
    pub event_type: String,
    /// Structured event payload.
    pub payload: Value,
    /// Event creation timestamp.
    pub created_at: String,
}

/// Expanded task detail payload returned by `task get` style APIs.
#[derive(Debug, Serialize)]
pub struct TaskDetail {
    /// Top-level task summary.
    pub task: TaskSummary,
    /// Task items associated with the task.
    pub items: Vec<TaskItemDto>,
    /// Command runs recorded for the task.
    pub runs: Vec<CommandRunDto>,
    /// Events recorded for the task.
    pub events: Vec<EventDto>,
    /// Graph-debug snapshots captured for dynamic orchestration.
    pub graph_debug: Vec<TaskGraphDebugBundle>,
}

/// Debug bundle capturing one graph-planning attempt and its snapshots.
#[derive(Debug, Clone, Serialize)]
pub struct TaskGraphDebugBundle {
    /// Graph run identifier.
    pub graph_run_id: String,
    /// Workflow cycle that produced the graph run.
    pub cycle: i64,
    /// Planner source used for the run.
    pub source: String,
    /// Final status for the graph run.
    pub status: String,
    /// Effective fallback mode when planning degraded.
    pub fallback_mode: Option<String>,
    /// Planner failure classification when planning failed.
    pub planner_failure_class: Option<String>,
    /// Planner failure message when available.
    pub planner_failure_message: Option<String>,
    /// Effective graph serialized as JSON.
    pub effective_graph_json: String,
    /// Raw planner output serialized as JSON.
    pub planner_raw_output_json: Option<String>,
    /// Normalized plan payload serialized as JSON.
    pub normalized_plan_json: Option<String>,
    /// Execution replay payload serialized as JSON.
    pub execution_replay_json: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Log-chunk payload used by log streaming or tailing APIs.
#[derive(Debug, Serialize)]
pub struct LogChunk {
    /// Command-run identifier.
    pub run_id: String,
    /// Phase that produced the log output.
    pub phase: String,
    /// Collected log content.
    pub content: String,
    /// Relative stdout path for the run.
    pub stdout_path: String,
    /// Relative stderr path for the run.
    pub stderr_path: String,
    /// Timestamp when the run started.
    pub started_at: Option<String>,
}

/// Lightweight task-item row used by repository and scheduler internals.
#[derive(Debug, Clone)]
pub struct TaskItemRow {
    /// Task-item identifier.
    pub id: String,
    /// QA document path associated with the item.
    pub qa_file_path: String,
    /// JSON-encoded dynamic variables attached to the item.
    pub dynamic_vars_json: Option<String>,
    /// Optional human-readable label for the item.
    pub label: Option<String>,
    /// Origin of the item, such as static workflow or dynamic generation.
    pub source: String,
    /// Current status of the item (e.g. `pending`, `qa_passed`, `skipped`).
    pub status: String,
}

/// Preview metadata extracted from a ticket document.
#[derive(Debug, Clone)]
pub struct TicketPreviewData {
    /// Ticket status parsed from the Markdown preamble.
    pub status: String,
    /// QA document path referenced by the ticket.
    pub qa_document: String,
}

/// Placeholder QA path used for tickets that are not tied to a concrete file.
pub const UNASSIGNED_QA_FILE_PATH: &str = "__UNASSIGNED__";

/// Result of running a command phase in the orchestrator pipeline.
#[derive(Debug)]
pub struct RunResult {
    /// Whether the command run was considered successful by the runner.
    pub success: bool,
    /// Numeric process exit code.
    pub exit_code: i64,
    /// Relative path to captured stdout.
    pub stdout_path: String,
    /// Relative path to captured stderr.
    pub stderr_path: String,
    /// Whether execution timed out.
    pub timed_out: bool,
    /// Measured duration in milliseconds when available.
    pub duration_ms: Option<u64>,
    /// Structured agent output parsed from the run.
    pub output: Option<crate::collab::AgentOutput>,
    /// Validation status assigned after output validation.
    pub validation_status: String,
    /// Agent identifier that performed the run.
    pub agent_id: String,
    /// Command-run identifier.
    pub run_id: String,
    /// Execution profile label chosen for the run.
    pub execution_profile: String,
    /// Effective execution mode label.
    pub execution_mode: String,
    /// Whether sandbox enforcement denied execution.
    pub sandbox_denied: bool,
    /// Human-readable sandbox denial reason when available.
    pub sandbox_denial_reason: Option<String>,
    /// Sandbox violation category when available.
    pub sandbox_violation_kind: Option<String>,
    /// Sandbox resource kind associated with the violation.
    pub sandbox_resource_kind: Option<String>,
    /// Sandbox network target associated with the violation.
    pub sandbox_network_target: Option<String>,
}

impl RunResult {
    /// Returns `true` when the run completed without timeout and exited with code `0`.
    pub fn is_success(&self) -> bool {
        self.success && !self.timed_out && self.exit_code == 0
    }
}
