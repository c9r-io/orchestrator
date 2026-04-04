use crate::db::open_conn;
use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

#[doc(hidden)]
pub enum TaskRepositorySource {
    /// Open repository connections from an on-disk SQLite database path.
    Path(PathBuf),
}

/// Concrete database connection type used by repository adapters.
pub type TaskRepositoryConn = Connection;

impl TaskRepositorySource {
    /// Opens a new repository connection from the configured source.
    pub fn connection(&self) -> Result<TaskRepositoryConn> {
        match self {
            TaskRepositorySource::Path(db_path) => open_conn(db_path),
        }
    }
}

impl From<PathBuf> for TaskRepositorySource {
    fn from(value: PathBuf) -> Self {
        Self::Path(value)
    }
}

/// Row needed to resume or continue execution for a task.
pub struct TaskRuntimeRow {
    /// Workspace identifier associated with the task.
    pub workspace_id: String,
    /// Workflow identifier associated with the task.
    pub workflow_id: String,
    /// Serialized workspace root path as stored in SQLite.
    pub workspace_root_raw: String,
    /// Ticket directory relative to the workspace.
    pub ticket_dir: String,
    /// Serialized execution plan JSON.
    pub execution_plan_json: String,
    /// Current cycle counter.
    pub current_cycle: i64,
    /// Non-zero when `init_once` has already been completed.
    pub init_done: i64,
    /// Task goal string.
    pub goal: String,
    /// Effective project identifier.
    pub project_id: String,
    /// Optional serialized pipeline-variable map.
    pub pipeline_vars_json: Option<String>,
    /// Current task spawn depth.
    pub spawn_depth: i64,
    /// FR-090: Serialized step filter (JSON array of step IDs).
    pub step_filter_json: Option<String>,
    /// FR-090: Serialized initial pipeline variables (JSON map).
    pub initial_vars_json: Option<String>,
}

/// Summary row for one command run returned by task-log queries.
pub struct TaskLogRunRow {
    /// Command-run identifier.
    pub run_id: String,
    /// Phase name for the run.
    pub phase: String,
    /// Path to captured stdout.
    pub stdout_path: String,
    /// Path to captured stderr.
    pub stderr_path: String,
    /// Optional run start time.
    pub started_at: Option<String>,
}

/// Event row written to the `events` table.
#[derive(Clone)]
pub struct DbEventRecord {
    /// Parent task identifier.
    pub task_id: String,
    /// Optional task-item identifier associated with the event.
    pub task_item_id: Option<String>,
    /// Event type name.
    pub event_type: String,
    /// JSON-serialized event payload.
    pub payload_json: String,
}

/// Insert payload for a task-graph planning run.
pub struct NewTaskGraphRun {
    /// Task-graph run identifier.
    pub graph_run_id: String,
    /// Parent task identifier.
    pub task_id: String,
    /// Cycle number that triggered the planning run.
    pub cycle: i64,
    /// Execution mode used by the planner.
    pub mode: String,
    /// Planner source identifier.
    pub source: String,
    /// Final status of the planning run.
    pub status: String,
    /// Optional fallback mode applied after planner degradation.
    pub fallback_mode: Option<String>,
    /// Optional normalized planner failure class.
    pub planner_failure_class: Option<String>,
    /// Optional planner failure message.
    pub planner_failure_message: Option<String>,
    /// Optional entry node selected for the graph.
    pub entry_node_id: Option<String>,
    /// Number of nodes in the graph snapshot.
    pub node_count: i64,
    /// Number of edges in the graph snapshot.
    pub edge_count: i64,
}

/// Insert payload for one persisted task-graph snapshot.
pub struct NewTaskGraphSnapshot {
    /// Task-graph run identifier that owns the snapshot.
    pub graph_run_id: String,
    /// Parent task identifier.
    pub task_id: String,
    /// Snapshot type such as `initial`, `adaptive`, or `final`.
    pub snapshot_kind: String,
    /// JSON payload of the snapshot.
    pub payload_json: String,
}
