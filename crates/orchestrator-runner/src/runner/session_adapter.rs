//! Provider-neutral command adaptation for opaque provider session reuse.

use anyhow::{Result, bail};

/// Adapts a provider command for session reuse without exposing provider tokens to API clients.
pub trait RunnerSessionAdapter: Send + Sync {
    /// Stable provider identifier used in diagnostics.
    fn provider(&self) -> &'static str;

    /// Returns whether this adapter owns the supplied command shape.
    fn supports(&self, command: &str) -> bool;

    /// Returns a command configured to reuse the opaque provider session.
    fn prepare_resume_command(&self, command: &str, opaque_session_token: &str) -> Result<String>;
}

/// Initial adapter for Claude CLI commands executed by the streaming runner.
#[derive(Debug, Default)]
pub struct ClaudeStreamingSessionAdapter;

impl RunnerSessionAdapter for ClaudeStreamingSessionAdapter {
    fn provider(&self) -> &'static str {
        "claude_streaming"
    }

    fn supports(&self, command: &str) -> bool {
        command
            .split_whitespace()
            .next()
            .is_some_and(|binary| binary == "claude" || binary.ends_with("/claude"))
    }

    fn prepare_resume_command(&self, command: &str, opaque_session_token: &str) -> Result<String> {
        if !self.supports(command) {
            bail!(
                "provider session resume is unavailable for this runner; restart from the logical boundary in a new session"
            );
        }
        if opaque_session_token.is_empty()
            || opaque_session_token
                .chars()
                .any(|character| character.is_control())
        {
            bail!("provider session reference is invalid; restart in a new session");
        }
        Ok(format!(
            "{command} --resume {}",
            shell_single_quote(opaque_session_token)
        ))
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_adapter_keeps_token_opaque_and_shell_quotes_it() {
        let command = ClaudeStreamingSessionAdapter
            .prepare_resume_command("claude -p 'continue'", "session-'42")
            .expect("prepare");
        assert_eq!(command, r#"claude -p 'continue' --resume 'session-'\''42'"#);
    }

    #[test]
    fn unsupported_provider_has_explicit_new_session_fallback() {
        let error = ClaudeStreamingSessionAdapter
            .prepare_resume_command("codex exec", "session-42")
            .expect_err("unsupported provider");
        assert!(
            error
                .to_string()
                .contains("restart from the logical boundary")
        );
    }
}
