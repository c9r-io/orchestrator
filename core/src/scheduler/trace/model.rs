use serde::Serialize;
use std::collections::HashMap;

use crate::anomaly::Anomaly;

#[derive(Debug, Serialize)]
pub struct BuildVersion {
    pub version: String,
    pub git_hash: String,
    pub build_timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct TaskTrace {
    pub task_id: String,
    pub status: String,
    pub cycles: Vec<CycleTrace>,
    pub anomalies: Vec<Anomaly>,
    pub summary: TraceSummary,
    pub build_version: Option<BuildVersion>,
}

#[derive(Debug, Serialize)]
pub struct CycleTrace {
    pub cycle: u32,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub steps: Vec<StepTrace>,
}

#[derive(Debug, Serialize)]
pub struct StepTrace {
    pub step_id: String,
    pub scope: String,
    pub item_id: Option<String>,
    pub anchor_item_id: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
    pub agent_id: Option<String>,
    pub duration_secs: Option<f64>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TraceSummary {
    pub total_cycles: u32,
    pub total_steps: u32,
    pub total_commands: u32,
    pub failed_commands: u32,
    pub anomaly_counts: HashMap<String, u32>,
    pub wall_time_secs: Option<f64>,
}

pub struct TraceTaskMeta<'a> {
    pub task_id: &'a str,
    pub status: &'a str,
    pub created_at: &'a str,
    pub started_at: Option<&'a str>,
    pub completed_at: Option<&'a str>,
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
