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
}

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub resumed_task_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NamedOption {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct CreateTaskDefaults {
    pub project_id: String,
    pub workspace_id: String,
    pub workflow_id: String,
}

#[derive(Debug, Serialize)]
pub struct CreateTaskOptions {
    pub defaults: CreateTaskDefaults,
    pub projects: Vec<NamedOption>,
    pub workspaces: Vec<NamedOption>,
    pub workflows: Vec<NamedOption>,
}

#[derive(Debug, Serialize)]
pub struct ConfigOverview {
    pub config: crate::config::OrchestratorConfig,
    pub yaml: String,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveConfigFormPayload {
    pub config: crate::config::OrchestratorConfig,
}

#[derive(Debug, Deserialize)]
pub struct SaveConfigYamlPayload {
    pub yaml: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct SimulatePrehookContextPayload {
    pub cycle: i64,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub qa_failed: bool,
    pub fix_required: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct SimulatePrehookPayload {
    pub expression: String,
    pub step: Option<String>,
    pub context: SimulatePrehookContextPayload,
}

#[derive(Debug, Serialize)]
pub struct SimulatePrehookResult {
    pub result: bool,
    pub expression: String,
}

#[derive(Debug, Serialize)]
pub struct ConfigVersionSummary {
    pub version: i64,
    pub created_at: String,
    pub author: String,
}

#[derive(Debug, Serialize)]
pub struct ConfigVersionDetail {
    pub version: i64,
    pub created_at: String,
    pub author: String,
    pub yaml: String,
}

#[derive(Debug, Serialize)]
pub struct ConfigValidationResult {
    pub valid: bool,
    pub normalized_yaml: String,
    pub errors: Vec<ValidationErrorDto>,
    pub warnings: Vec<ValidationWarningDto>,
    pub summary: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationErrorDto {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidationWarningDto {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub suggestion: Option<String>,
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
pub struct DeleteTaskResponse {
    pub task_id: String,
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
pub struct LogChunk {
    pub run_id: String,
    pub phase: String,
    pub content: String,
    pub stdout_path: String,
    pub stderr_path: String,
}

#[derive(Debug, Clone)]
pub struct TaskItemRow {
    pub id: String,
    pub qa_file_path: String,
}

#[derive(Debug, Clone)]
pub struct TicketPreviewData {
    pub path: String,
    pub title: String,
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
}

impl RunResult {
    pub fn is_success(&self) -> bool {
        self.success && !self.timed_out && self.exit_code == 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentHealthInfo {
    pub agent_id: String,
    pub healthy: bool,
    pub diseased_until: Option<String>,
    pub consecutive_errors: u32,
}
