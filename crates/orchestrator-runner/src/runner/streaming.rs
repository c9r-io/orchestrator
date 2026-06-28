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
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// MCP server key advertised to the agent CLI. Tool names the agent sees are
/// `mcp__<server_key>__<tool>`.
const MCP_SERVER_KEY: &str = "orch";

/// Environment variable overriding the path to the orchestrator MCP tools
/// binary (`orch-mcp-tools`). When unset, the binary is looked up next to the
/// current executable.
const MCP_TOOLS_BIN_ENV: &str = "ORCH_MCP_TOOLS_BIN";

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
        } = params;

        let bin = resolve_mcp_tools_bin()?;
        let config_path = write_mcp_config(&bin)?;
        let streaming_command = build_streaming_command(command, &config_path);

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

/// Allow-list pattern granting the agent every tool on the orchestrator MCP
/// server (`mcp__<server>`), so new tools work without changing the runner.
fn allowed_tools_pattern() -> String {
    format!("mcp__{}", MCP_SERVER_KEY)
}

/// Augments the agent command with the flags that turn it into a structured,
/// MCP-tool-calling, single-turn run.
fn build_streaming_command(base_command: &str, mcp_config_path: &Path) -> String {
    format!(
        "{base} --output-format stream-json --verbose \
         --mcp-config {config} --strict-mcp-config \
         --allowedTools {tool} --permission-mode bypassPermissions",
        base = base_command,
        config = shell_single_quote(&mcp_config_path.to_string_lossy()),
        tool = allowed_tools_pattern(),
    )
}

/// Builds the `--mcp-config` JSON registering the orchestrator-owned stdio MCP
/// server that hosts the typed tools.
fn mcp_config_json(bin: &Path) -> String {
    use serde_json::{Map, Value};

    let mut server = Map::new();
    server.insert(
        "command".to_string(),
        Value::String(bin.to_string_lossy().into_owned()),
    );
    server.insert("args".to_string(), Value::Array(Vec::new()));

    let mut servers = Map::new();
    servers.insert(MCP_SERVER_KEY.to_string(), Value::Object(server));

    let mut root = Map::new();
    root.insert("mcpServers".to_string(), Value::Object(servers));

    Value::Object(root).to_string()
}

/// Writes the MCP config to a stable temp path (atomically) and returns it.
///
/// The content depends only on the binary path, so concurrent steps sharing the
/// same binary write identical content; the atomic rename avoids torn reads.
fn write_mcp_config(bin: &Path) -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let path = dir.join("orch-streaming-mcp.json");
    let tmp = dir.join(format!(
        "orch-streaming-mcp.{}.json.tmp",
        std::process::id()
    ));
    std::fs::write(&tmp, mcp_config_json(bin))
        .with_context(|| format!("writing MCP config to {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("publishing MCP config to {}", path.display()))?;
    Ok(path)
}

/// Resolves the path to the `orch-mcp-tools` binary: `ORCH_MCP_TOOLS_BIN` if
/// set, otherwise a sibling of the current executable.
fn resolve_mcp_tools_bin() -> Result<PathBuf> {
    if let Ok(p) = std::env::var(MCP_TOOLS_BIN_ENV) {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let exe = std::env::current_exe().context("resolving current_exe for MCP tools binary")?;
    let dir = exe
        .parent()
        .context("current executable has no parent directory")?;
    let candidate = dir.join("orch-mcp-tools");
    if candidate.exists() {
        return Ok(candidate);
    }
    anyhow::bail!(
        "could not locate the orch-mcp-tools binary (set {} or place it next to the daemon)",
        MCP_TOOLS_BIN_ENV
    )
}

/// Wraps a string in single quotes for safe inclusion in a `/bin/sh -c` command,
/// escaping any embedded single quotes.
fn shell_single_quote(s: &str) -> String {
    let escaped = s.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_tools_pattern_grants_whole_server() {
        assert_eq!(allowed_tools_pattern(), "mcp__orch");
    }

    #[test]
    fn streaming_command_appends_expected_flags() {
        let cmd = build_streaming_command("claude -p \"do it\"", Path::new("/tmp/cfg.json"));
        assert!(cmd.starts_with("claude -p \"do it\""));
        assert!(cmd.contains("--output-format stream-json"));
        assert!(cmd.contains("--verbose"));
        assert!(cmd.contains("--mcp-config '/tmp/cfg.json'"));
        assert!(cmd.contains("--strict-mcp-config"));
        assert!(cmd.contains("--allowedTools mcp__orch"));
        assert!(cmd.contains("--permission-mode bypassPermissions"));
    }

    #[test]
    fn mcp_config_json_registers_server_and_bin() {
        let json = mcp_config_json(Path::new("/opt/bin/orch-mcp-tools"));
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("mcp config should be valid JSON");
        assert_eq!(
            parsed["mcpServers"]["orch"]["command"],
            serde_json::json!("/opt/bin/orch-mcp-tools")
        );
        assert_eq!(parsed["mcpServers"]["orch"]["args"], serde_json::json!([]));
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quote() {
        assert_eq!(shell_single_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_single_quote("it's"), r"'it'\''s'");
    }
}
