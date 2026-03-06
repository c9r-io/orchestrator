use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct CreateTaskPayload {
    pub name: Option<String>,
    pub goal: Option<String>,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub workflow_id: Option<String>,
    pub target_files: Option<Vec<String>>,
    pub parent_task_id: Option<String>,
    pub spawn_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfigOverview {
    pub config: crate::config::OrchestratorConfig,
    pub yaml: String,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct TaskSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub goal: String,
    pub project_id: String,
    pub workspace_id: String,
    pub workflow_id: String,
    pub target_files: Vec<String>,
    pub total_items: i64,
    pub finished_items: i64,
    pub failed_items: i64,
    pub created_at: String,
    pub updated_at: String,
    pub parent_task_id: Option<String>,
    pub spawn_reason: Option<String>,
    pub spawn_depth: i64,
}

#[derive(Debug, Serialize)]
pub struct TaskItemDto {
    pub id: String,
    pub task_id: String,
    pub order_no: i64,
    pub qa_file_path: String,
    pub status: String,
    pub ticket_files: Vec<String>,
    pub ticket_content: Vec<Value>,
    pub fix_required: bool,
    pub fixed: bool,
    pub last_error: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct CommandRunDto {
    pub id: String,
    pub task_item_id: String,
    pub phase: String,
    pub command: String,
    pub cwd: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub exit_code: Option<i64>,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub interrupted: bool,
}

#[derive(Debug, Serialize)]
pub struct EventDto {
    pub id: i64,
    pub task_id: String,
    pub task_item_id: Option<String>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: TaskSummary,
    pub items: Vec<TaskItemDto>,
    pub runs: Vec<CommandRunDto>,
    pub events: Vec<EventDto>,
}

#[derive(Debug, Serialize)]
pub struct LogChunk {
    pub run_id: String,
    pub phase: String,
    pub content: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskItemRow {
    pub id: String,
    pub qa_file_path: String,
    pub dynamic_vars_json: Option<String>,
    pub label: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct TicketPreviewData {
    pub status: String,
    pub qa_document: String,
}

pub const UNASSIGNED_QA_FILE_PATH: &str = "__UNASSIGNED__";

#[derive(Debug)]
pub struct RunResult {
    pub success: bool,
    pub exit_code: i64,
    pub stdout_path: String,
    pub stderr_path: String,
    pub timed_out: bool,
    pub duration_ms: Option<u64>,
    pub output: Option<crate::collab::AgentOutput>,
    pub validation_status: String,
    pub agent_id: String,
    pub run_id: String,
}

impl RunResult {
    pub fn is_success(&self) -> bool {
        self.success && !self.timed_out && self.exit_code == 0
    }
}
