//! Streaming agent runner.
//!
//! Replaces the one-shot shell-text agent contract with a structured,
//! tool-calling contract: the agent CLI is launched in `stream-json` mode and
//! given orchestrator-owned typed tools over MCP. Coordination that previously
//! had to live in YAML/CEL (captures, post-actions) can instead be expressed as
//! tools the agent calls during the step.
//!
//! This is the first cut: a single step delivers its prompt via the agent
//! command's existing `{prompt}` argument and the agent may call one
//! orchestrator-owned MCP tool. The process is still spawned through the same
//! shell/sandbox/env path as [`super::spawn::ShellRunnerExecutor`]; only the
//! command string (augmented with `stream-json` + MCP flags) differs. See
//! `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`.

use super::spawn::{RunnerExecutor, SpawnParams, spawn_command_via_shell};
use anyhow::Result;

/// Runner that executes an agent CLI as a structured, tool-calling process
/// rather than a one-shot shell-text black box.
#[derive(Debug, Default)]
pub struct StreamingAgentRunner;

impl RunnerExecutor for StreamingAgentRunner {
    fn spawn(&self, params: SpawnParams<'_>) -> Result<tokio::process::Child> {
        let SpawnParams {
            runner,
            command,
            cwd,
            stdio_mode,
            extra_env,
            pipe_stdin,
            execution_profile,
            provider_session_token,
        } = params;

        let streaming_command = crate::driver::prepare_legacy_claude_streaming_command(
            command,
            provider_session_token,
        )?;

        spawn_command_via_shell(
            runner,
            &streaming_command,
            cwd,
            stdio_mode,
            extra_env,
            pipe_stdin,
            execution_profile,
        )
    }
}
