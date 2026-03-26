use agent_orchestrator::config::{PipelineVariables, PromptDelivery, RunnerConfig, StepScope};
use agent_orchestrator::runner::{ResolvedExecutionProfile, SandboxResourceKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::super::RunningTask;

pub(super) const DEFAULT_STEP_TIMEOUT_SECS: u64 = 1800;
pub(super) const HEARTBEAT_INTERVAL_SECS: u64 = 30;
pub(super) const LOW_OUTPUT_DELTA_THRESHOLD_BYTES: u64 = 32;
pub(super) const LOW_OUTPUT_MIN_ELAPSED_SECS: u64 = 90;
pub(super) const LOW_OUTPUT_CONSECUTIVE_HEARTBEATS: u32 = 3;
/// Auto-kill a step after this many consecutive stagnant heartbeats (30 × 30s = 900s).
pub(super) const STALL_AUTO_KILL_CONSECUTIVE_HEARTBEATS: u32 = 30;
pub(super) const VALIDATION_FAILED_EXIT_CODE: i64 = -6;
pub(super) const SANDBOX_STDERR_EXCERPT_MAX_BYTES: u64 = 1024;

pub(super) struct LimitedOutput {
    pub text: String,
    pub truncated_prefix_bytes: u64,
}

#[derive(Default)]
pub(super) struct HeartbeatProgress {
    pub last_stdout_bytes: u64,
    pub last_stderr_bytes: u64,
    pub stagnant_heartbeats: u32,
}

pub(super) struct HeartbeatSample {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_delta_bytes: u64,
    pub stderr_delta_bytes: u64,
    pub stagnant_heartbeats: u32,
    pub output_state: &'static str,
}

#[derive(Debug, Default, Clone)]
pub(super) struct SandboxViolationInfo {
    pub denied: bool,
    pub event_type: Option<&'static str>,
    pub reason_code: Option<&'static str>,
    pub reason: Option<String>,
    pub stderr_excerpt: Option<String>,
    pub resource_kind: Option<SandboxResourceKind>,
    pub network_target: Option<String>,
}

/// Intermediate data produced by `setup_phase_execution` and consumed by later stages.
pub(super) struct PhaseSetup {
    pub run_uuid: uuid::Uuid,
    pub run_id: String,
    pub now: String,
    pub runner: RunnerConfig,
    pub resolved_extra_env: HashMap<String, String>,
    pub redaction_patterns: Vec<String>,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub stdout_file: std::fs::File,
    pub stderr_file: std::fs::File,
    pub command: String,
    pub command_template: Option<String>,
    pub execution_profile: ResolvedExecutionProfile,
}

/// Intermediate data produced by `spawn_phase_process`.
pub(super) struct SpawnResult {
    pub session_id: Option<String>,
    pub child_pid: Option<u32>,
    pub output_capture: Option<agent_orchestrator::output_capture::OutputCaptureHandles>,
    /// If `true`, the TTY early-return path was taken and the caller should return immediately.
    pub tty_early_return: Option<agent_orchestrator::dto::RunResult>,
}

/// Intermediate data produced by `wait_for_process`.
pub(super) struct WaitResult {
    pub exit_code: i32,
    pub exit_signal: Option<i32>,
    pub timed_out: bool,
    pub duration: std::time::Duration,
}

/// Intermediate data produced by `validate_phase_output_stage`.
pub(super) struct ValidatedOutput {
    pub final_exit_code: i64,
    pub success: bool,
    pub validation_status: &'static str,
    pub validation_event_payload_json: Option<String>,
    pub redacted_output: agent_orchestrator::collab::AgentOutput,
    pub validation_error: Option<String>,
    pub sandbox_denied: bool,
    pub sandbox_event_type: Option<&'static str>,
    pub sandbox_reason_code: Option<&'static str>,
    pub sandbox_denial_reason: Option<String>,
    pub sandbox_denial_stderr_excerpt: Option<String>,
    pub sandbox_resource_kind: Option<SandboxResourceKind>,
    pub sandbox_network_target: Option<String>,
}

pub struct PhaseRunRequest<'a> {
    pub task_id: &'a str,
    pub item_id: &'a str,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub tty: bool,
    pub command: String,
    pub command_template: Option<String>,
    pub workspace_root: &'a Path,
    pub workspace_id: &'a str,
    pub agent_id: &'a str,
    pub runtime: &'a RunningTask,
    pub step_timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub step_scope: StepScope,
    /// How the prompt payload is delivered to the agent process.
    pub prompt_delivery: PromptDelivery,
    /// Rendered prompt for non-arg delivery modes (stdin, file, env).
    pub prompt_payload: Option<String>,
    /// Whether to pipe stdin to the child process.
    pub pipe_stdin: bool,
    /// Project ID for project-scoped agent env resolution (empty = non-project).
    pub project_id: &'a str,
    pub execution_profile: Option<&'a str>,
    /// Whether the workspace is self-referential (daemon PID protection enabled).
    pub self_referential: bool,
    /// Index of the matched agent command_rule (None = default command).
    #[allow(dead_code)]
    pub command_rule_index: Option<i32>,
}

pub struct RotatingPhaseRunRequest<'a> {
    pub task_id: &'a str,
    pub item_id: &'a str,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub tty: bool,
    pub capability: Option<&'a str>,
    pub rel_path: &'a str,
    pub ticket_paths: &'a [String],
    pub workspace_root: &'a Path,
    pub workspace_id: &'a str,
    pub cycle: u32,
    pub runtime: &'a RunningTask,
    pub pipeline_vars: Option<&'a PipelineVariables>,
    pub step_timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub step_scope: StepScope,
    /// Prompt from a resolved StepTemplate, injected into the agent command's {prompt} placeholder
    pub step_template_prompt: Option<&'a str>,
    /// Project ID for project-scoped agent selection (empty = non-project).
    pub project_id: &'a str,
    pub execution_profile: Option<&'a str>,
    /// Whether the workspace is self-referential (daemon PID protection enabled).
    pub self_referential: bool,
}

pub struct SelectedPhaseRunRequest<'a> {
    pub task_id: &'a str,
    pub item_id: &'a str,
    pub step_id: &'a str,
    pub phase: &'a str,
    pub tty: bool,
    pub agent_id: &'a str,
    pub command_template: &'a str,
    pub prompt_delivery: PromptDelivery,
    pub rel_path: &'a str,
    pub ticket_paths: &'a [String],
    pub workspace_root: &'a Path,
    pub workspace_id: &'a str,
    pub cycle: u32,
    pub runtime: &'a RunningTask,
    pub pipeline_vars: Option<&'a PipelineVariables>,
    pub step_timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub step_scope: StepScope,
    pub step_template_prompt: Option<&'a str>,
    /// Project ID for project-scoped agent env resolution (empty = non-project).
    pub project_id: &'a str,
    pub execution_profile: Option<&'a str>,
    /// Whether the workspace is self-referential (daemon PID protection enabled).
    pub self_referential: bool,
    /// Index of the matched agent command_rule (None = default command).
    pub command_rule_index: Option<i32>,
}
