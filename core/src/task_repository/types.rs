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
}

pub struct TaskLogRunRow {
    pub run_id: String,
    pub phase: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: Option<String>,
}
