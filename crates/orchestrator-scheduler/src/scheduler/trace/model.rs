use serde::Serialize;
use std::collections::HashMap;

use agent_orchestrator::anomaly::Anomaly;

/// Build metadata embedded into exported trace payloads.
#[derive(Debug, Serialize)]
pub struct BuildVersion {
    /// Semantic version of the running binary.
    pub version: String,
    /// Git revision baked into the build.
    pub git_hash: String,
    /// Build timestamp baked into the binary.
    pub build_timestamp: String,
}

/// Full trace payload returned by task-trace APIs.
#[derive(Debug, Serialize)]
pub struct TaskTrace {
    /// Stable task identifier.
    pub task_id: String,
    /// Final task status.
    pub status: String,
    /// Cycle-level execution trace entries.
    pub cycles: Vec<CycleTrace>,
    /// Graph-planning trace entries, when dynamic orchestration was used.
    pub graph_runs: Vec<GraphTrace>,
    /// Scheduler anomalies detected while building the trace.
    pub anomalies: Vec<Anomaly>,
    /// Aggregate counts derived from the trace.
    pub summary: TraceSummary,
    /// Build metadata for the binary that generated the trace, when available.
    pub build_version: Option<BuildVersion>,
}

/// Trace for one workflow cycle.
#[derive(Debug, Serialize)]
pub struct CycleTrace {
    /// Zero-based or one-based cycle number stored by the scheduler.
    pub cycle: u32,
    /// Timestamp when the cycle started.
    pub started_at: Option<String>,
    /// Timestamp when the cycle finished.
    pub ended_at: Option<String>,
    /// Step executions recorded within the cycle.
    pub steps: Vec<StepTrace>,
}

/// Trace for one workflow step execution.
#[derive(Debug, Serialize)]
pub struct StepTrace {
    /// Step identifier.
    pub step_id: String,
    /// Step scope label such as `task` or `item`.
    pub scope: String,
    /// Task-item identifier for item-scoped steps.
    pub item_id: Option<String>,
    /// Anchor task-item identifier used to correlate related executions.
    pub anchor_item_id: Option<String>,
    /// Timestamp when the step started.
    pub started_at: Option<String>,
    /// Timestamp when the step finished.
    pub ended_at: Option<String>,
    /// Process exit code when a command was executed.
    pub exit_code: Option<i64>,
    /// Agent identifier selected for the step.
    pub agent_id: Option<String>,
    /// Wall-clock duration in seconds.
    pub duration_secs: Option<f64>,
    /// Whether the scheduler skipped the step.
    pub skipped: bool,
    /// Human-readable explanation for the skip decision.
    pub skip_reason: Option<String>,
}

/// Dynamic graph execution trace for one cycle.
#[derive(Debug, Serialize)]
pub struct GraphTrace {
    /// Workflow cycle that produced the graph run.
    pub cycle: u32,
    /// Planner source or fallback source label.
    pub source: Option<String>,
    /// Number of nodes in the materialized graph.
    pub node_count: u32,
    /// Number of edges in the materialized graph.
    pub edge_count: u32,
    /// Event stream recorded for the graph execution.
    pub events: Vec<GraphEventTrace>,
}

/// One event emitted while executing a dynamic graph.
#[derive(Debug, Serialize)]
pub struct GraphEventTrace {
    /// Event type label.
    pub event_type: String,
    /// Node associated with the event, when applicable.
    pub node_id: Option<String>,
    /// Source node for an edge traversal event.
    pub from: Option<String>,
    /// Destination node for an edge traversal event.
    pub to: Option<String>,
    /// Whether a conditional edge was taken.
    pub taken: Option<bool>,
    /// Timestamp when the event was recorded.
    pub created_at: String,
}

/// Aggregate counts derived from task and graph traces.
#[derive(Debug, Serialize)]
pub struct TraceSummary {
    /// Number of workflow cycles observed.
    pub total_cycles: u32,
    /// Number of step executions observed.
    pub total_steps: u32,
    /// Number of command executions observed.
    pub total_commands: u32,
    /// Number of failed command executions.
    pub failed_commands: u32,
    /// Per-anomaly counts keyed by anomaly type.
    pub anomaly_counts: HashMap<String, u32>,
    /// End-to-end wall-clock duration in seconds when derivable.
    pub wall_time_secs: Option<f64>,
}

/// Task metadata used while constructing a trace payload.
pub struct TraceTaskMeta<'a> {
    /// Stable task identifier.
    pub task_id: &'a str,
    /// Current or final task status.
    pub status: &'a str,
    /// Task creation timestamp.
    pub created_at: &'a str,
    /// Task start timestamp when available.
    pub started_at: Option<&'a str>,
    /// Task completion timestamp when available.
    pub completed_at: Option<&'a str>,
    /// Last update timestamp recorded for the task.
    pub updated_at: &'a str,
}

#[derive(Debug)]
pub(super) struct CycleBuilder {
    pub(super) cycle: u32,
    pub(super) started_at: Option<String>,
    pub(super) ended_at: Option<String>,
    pub(super) last_seen_at: Option<String>,
    pub(super) steps: Vec<StepTrace>,
}
