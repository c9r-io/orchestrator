/// Mutable command-run payload persisted by repository write operations.
#[derive(Clone)]
pub struct NewCommandRun {
    /// Command-run identifier.
    pub id: String,
    /// Task-item identifier that owns the run.
    pub task_item_id: String,
    /// Logical phase name associated with the command.
    pub phase: String,
    /// Rendered command string that was executed.
    pub command: String,
    /// Pre-rendered command template containing unexpanded variable placeholders.
    pub command_template: Option<String>,
    /// Working directory used for the command.
    pub cwd: String,
    /// Workspace identifier for the run.
    pub workspace_id: String,
    /// Agent identifier selected for the run.
    pub agent_id: String,
    /// Exit code reported by the process.
    pub exit_code: i64,
    /// Path to the captured stdout log.
    pub stdout_path: String,
    /// Path to the captured stderr log.
    pub stderr_path: String,
    /// Start timestamp serialized for storage.
    pub started_at: String,
    /// End timestamp serialized for storage.
    pub ended_at: String,
    /// Non-zero when the run was interrupted.
    pub interrupted: i64,
    /// Structured machine output serialized as JSON.
    pub output_json: String,
    /// Structured artifact list serialized as JSON.
    pub artifacts_json: String,
    /// Optional confidence score emitted by the agent.
    pub confidence: Option<f32>,
    /// Optional quality score emitted by the agent.
    pub quality_score: Option<f32>,
    /// Validation status assigned after output checking.
    pub validation_status: String,
    /// Optional interactive session identifier.
    pub session_id: Option<String>,
    /// Origin of the structured output payload.
    pub machine_output_source: String,
    /// Optional path to a large structured output spill file.
    pub output_json_path: Option<String>,
}
