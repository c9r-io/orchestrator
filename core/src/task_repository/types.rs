use crate::database::Database;
use crate::db::open_conn;
use anyhow::Result;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;

#[doc(hidden)]
pub enum TaskRepositorySource {
    Database(Arc<Database>),
    Path(PathBuf),
}

pub enum TaskRepositoryConn {
    Pooled(PooledConnection<SqliteConnectionManager>),
    Direct(Connection),
}

impl std::ops::Deref for TaskRepositoryConn {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Pooled(conn) => conn,
            Self::Direct(conn) => conn,
        }
    }
}

impl std::ops::DerefMut for TaskRepositoryConn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Pooled(conn) => conn,
            Self::Direct(conn) => conn,
        }
    }
}

impl TaskRepositorySource {
    pub fn connection(&self) -> Result<TaskRepositoryConn> {
        match self {
            TaskRepositorySource::Database(database) => {
                Ok(TaskRepositoryConn::Pooled(database.connection()?))
            }
            TaskRepositorySource::Path(db_path) => {
                Ok(TaskRepositoryConn::Direct(open_conn(db_path)?))
            }
        }
    }
}

impl From<PathBuf> for TaskRepositorySource {
    fn from(value: PathBuf) -> Self {
        Self::Path(value)
    }
}

impl From<Arc<Database>> for TaskRepositorySource {
    fn from(value: Arc<Database>) -> Self {
        Self::Database(value)
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
}

pub struct TaskLogRunRow {
    pub run_id: String,
    pub phase: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: Option<String>,
}
