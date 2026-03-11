use crate::db::open_conn;
use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

#[doc(hidden)]
pub enum TaskRepositorySource {
    Path(PathBuf),
}

pub type TaskRepositoryConn = Connection;

impl TaskRepositorySource {
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

pub struct TaskRuntimeRow {
    pub workspace_id: String,
    pub workflow_id: String,
    pub workspace_root_raw: String,
    pub ticket_dir: String,
    pub execution_plan_json: String,
    pub current_cycle: i64,
    pub init_done: i64,
    pub goal: String,
    pub project_id: String,
    pub pipeline_vars_json: Option<String>,
    pub spawn_depth: i64,
}

pub struct TaskLogRunRow {
    pub run_id: String,
    pub phase: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: Option<String>,
}

#[derive(Clone)]
pub struct DbEventRecord {
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub event_type: String,
    pub payload_json: String,
}

pub struct NewTaskGraphRun {
    pub graph_run_id: String,
    pub task_id: String,
    pub cycle: i64,
    pub mode: String,
    pub source: String,
    pub status: String,
    pub fallback_mode: Option<String>,
    pub planner_failure_class: Option<String>,
    pub planner_failure_message: Option<String>,
    pub entry_node_id: Option<String>,
    pub node_count: i64,
    pub edge_count: i64,
}

pub struct NewTaskGraphSnapshot {
    pub graph_run_id: String,
    pub task_id: String,
    pub snapshot_kind: String,
    pub payload_json: String,
}
