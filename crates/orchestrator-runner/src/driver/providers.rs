use super::process::{ProcessSession, piped_stdio};
use super::{
    AgentDriver, DriverCapabilities, DriverSession, DriverStartRequest, driver_capabilities,
};
use crate::runner::spawn_command_via_shell;
use anyhow::{Context, Result};
use async_trait::async_trait;
use orchestrator_config::config::{DriverPermissionMode, DriverProvider, ToolHosting};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(super) struct ShellCliDriver;
pub(super) struct ClaudeCliDriver;
pub(super) struct CodexCliDriver;

#[async_trait]
impl AgentDriver for ShellCliDriver {
    fn id(&self) -> &'static str {
        "shell/cli"
    }
    fn capabilities(&self) -> DriverCapabilities {
        capabilities(DriverProvider::Shell)
    }

    async fn start(&self, request: DriverStartRequest<'_>) -> Result<Box<dyn DriverSession>> {
        let child = spawn_command(&request, request.legacy_command, false)?;
        Ok(Box::new(
            ProcessSession::start(
                DriverProvider::Shell,
                child,
                request.stdout,
                request.stderr,
                request.redaction_patterns.to_vec(),
                None,
            )
            .await?,
        ))
    }
}

#[async_trait]
impl AgentDriver for ClaudeCliDriver {
    fn id(&self) -> &'static str {
        "claude/cli"
    }
    fn capabilities(&self) -> DriverCapabilities {
        capabilities(DriverProvider::Claude)
    }

    async fn start(&self, request: DriverStartRequest<'_>) -> Result<Box<dyn DriverSession>> {
        let mcp_config = if self.capabilities().tool_hosting == ToolHosting::Stdio {
            Some(write_per_run_mcp_config(request.artifacts_dir)?)
        } else {
            None
        };
        let command = build_claude_command(&request, mcp_config.as_deref());
        let initial_input = format!(
            "{}\n",
            json!({"type":"user","message":{"role":"user","content":request.prompt}})
        );
        let child = spawn_command(&request, &command, true)?;
        Ok(Box::new(
            ProcessSession::start(
                DriverProvider::Claude,
                child,
                request.stdout,
                request.stderr,
                request.redaction_patterns.to_vec(),
                Some(initial_input),
            )
            .await?,
        ))
    }
}

#[async_trait]
impl AgentDriver for CodexCliDriver {
    fn id(&self) -> &'static str {
        "codex/cli"
    }
    fn capabilities(&self) -> DriverCapabilities {
        capabilities(DriverProvider::Codex)
    }

    async fn start(&self, request: DriverStartRequest<'_>) -> Result<Box<dyn DriverSession>> {
        let command = build_codex_command(&request);
        let child = spawn_command(&request, &command, false)?;
        Ok(Box::new(
            ProcessSession::start(
                DriverProvider::Codex,
                child,
                request.stdout,
                request.stderr,
                request.redaction_patterns.to_vec(),
                None,
            )
            .await?,
        ))
    }
}

fn capabilities(provider: DriverProvider) -> DriverCapabilities {
    let mut config = orchestrator_config::config::AgentDriverConfig {
        provider,
        transport: orchestrator_config::config::DriverTransport::Cli,
        binary: None,
        options: Default::default(),
        claude: None,
        codex: None,
        shell: None,
        raw_args: Vec::new(),
        unsafe_raw_args: false,
    };
    if provider == DriverProvider::Shell {
        config.shell = Some(Default::default());
    }
    driver_capabilities(&config)
}

fn spawn_command(
    request: &DriverStartRequest<'_>,
    command: &str,
    pipe_stdin: bool,
) -> Result<tokio::process::Child> {
    let cwd = request.driver.options.cwd.as_deref().map_or_else(
        || request.cwd.to_path_buf(),
        |relative| request.cwd.join(relative),
    );
    let mut env = request.extra_env.clone();
    env.extend(request.driver.options.env.clone());
    if let Some(tokens) = request
        .driver
        .claude
        .as_ref()
        .and_then(|config| config.thinking_budget_tokens)
    {
        env.insert("MAX_THINKING_TOKENS".to_string(), tokens.to_string());
    }
    spawn_command_via_shell(
        request.runner,
        command,
        &cwd,
        piped_stdio(),
        &env,
        pipe_stdin,
        request.execution_profile,
    )
}

fn build_claude_command(request: &DriverStartRequest<'_>, mcp_config: Option<&Path>) -> String {
    let binary = request.driver.binary.as_deref().unwrap_or("claude");
    let mut args = vec![
        quote(binary),
        "--print".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];
    push_common_args(&mut args, request);
    if let Some(turns) = request.driver.options.max_turns {
        args.extend(["--max-turns".to_string(), turns.to_string()]);
    }
    if let Some(budget) = request.driver.options.budget_cap_usd {
        args.extend(["--max-budget-usd".to_string(), budget.to_string()]);
    }
    let permission = match request.driver.options.permission_mode {
        DriverPermissionMode::Governed | DriverPermissionMode::Ask => "default",
        DriverPermissionMode::Deny => "dontAsk",
    };
    args.extend(["--permission-mode".to_string(), permission.to_string()]);
    if let Some(config) = mcp_config {
        let allowed_tools = if request.driver.options.allowed_tools.is_empty() {
            vec!["mcp__orch".to_string()]
        } else {
            request.driver.options.allowed_tools.clone()
        };
        args.extend([
            "--mcp-config".to_string(),
            quote(&config.to_string_lossy()),
            "--strict-mcp-config".to_string(),
            "--allowedTools".to_string(),
        ]);
        args.extend(allowed_tools.into_iter().map(|tool| quote(&tool)));
    }
    if let Some(reference) = request.session_ref {
        args.extend(["--resume".to_string(), quote(reference.expose_secret())]);
    }
    args.extend(request.driver.raw_args.iter().map(|arg| quote(arg)));
    args.join(" ")
}

fn build_codex_command(request: &DriverStartRequest<'_>) -> String {
    let binary = request.driver.binary.as_deref().unwrap_or("codex");
    let mut args = vec![quote(binary), "exec".to_string()];
    if let Some(reference) = request.session_ref {
        args.extend(["resume".to_string(), quote(reference.expose_secret())]);
    }
    args.push("--json".to_string());
    push_common_args(&mut args, request);
    if let Some(codex) = &request.driver.codex {
        if let Some(effort) = &codex.reasoning_effort {
            args.extend([
                "--config".to_string(),
                quote(&format!("model_reasoning_effort={effort}")),
            ]);
        }
    }
    args.extend(request.driver.raw_args.iter().map(|arg| quote(arg)));
    args.push("--".to_string());
    args.push(quote(request.prompt));
    args.join(" ")
}

fn push_common_args(args: &mut Vec<String>, request: &DriverStartRequest<'_>) {
    if let Some(model) = &request.driver.options.model {
        args.extend(["--model".to_string(), quote(model)]);
    }
}

fn write_per_run_mcp_config(artifacts_dir: &Path) -> Result<PathBuf> {
    let directory = artifacts_dir.join("driver");
    std::fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating driver artifacts directory {}",
            directory.display()
        )
    })?;
    let path = directory.join("mcp.json");
    let binary = resolve_mcp_tools_bin()?;
    let value = json!({"mcpServers":{"orch":{"command":binary,"args":[]}}});
    std::fs::write(&path, value.to_string())
        .with_context(|| format!("writing per-run MCP config {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

/// Compatibility bridge for the deprecated global streaming executor.
/// Provider flags remain confined to the Claude driver module while existing
/// manifests migrate to per-Agent driver configuration.
pub(crate) fn prepare_legacy_claude_streaming_command(
    base_command: &str,
    session_ref: Option<&str>,
) -> Result<String> {
    let directory = std::env::temp_dir().join(format!("orch-streaming-{}", uuid::Uuid::new_v4()));
    let config_path = write_per_run_mcp_config(&directory)?;
    let mut command = base_command.to_string();
    if let Some(reference) = session_ref {
        let reference = super::SessionRef::from_provider(reference.to_string())?;
        command.push_str(" --resume ");
        command.push_str(&quote(reference.expose_secret()));
    }
    Ok(format!(
        "{command} --output-format stream-json --verbose --mcp-config {} \
         --strict-mcp-config --allowedTools mcp__orch --permission-mode bypassPermissions",
        quote(&config_path.to_string_lossy()),
    ))
}

fn resolve_mcp_tools_bin() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("ORCH_MCP_TOOLS_BIN") {
        if !value.is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .context("current executable has no parent")?;
    Ok(directory.join("orch-mcp-tools"))
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_config::config::{AgentDriverConfig, DriverOptions, DriverTransport};
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    use tempfile::tempdir;

    fn driver(provider: DriverProvider) -> AgentDriverConfig {
        AgentDriverConfig {
            provider,
            transport: DriverTransport::Cli,
            binary: None,
            options: DriverOptions::default(),
            claude: None,
            codex: None,
            shell: None,
            raw_args: Vec::new(),
            unsafe_raw_args: false,
        }
    }

    fn request<'a>(config: &'a AgentDriverConfig, cwd: &'a Path) -> DriverStartRequest<'a> {
        DriverStartRequest {
            driver: config,
            runner: Box::leak(Box::new(
                orchestrator_config::config::RunnerConfig::default(),
            )),
            legacy_command: "echo ok",
            prompt: "fix it's tests",
            cwd,
            stdout: tempfile::tempfile().unwrap(),
            stderr: tempfile::tempfile().unwrap(),
            redaction_patterns: &[],
            extra_env: Box::leak(Box::new(HashMap::new())),
            execution_profile: Box::leak(Box::new(crate::runner::ResolvedExecutionProfile::host())),
            artifacts_dir: cwd,
            session_ref: None,
        }
    }

    #[test]
    fn provider_flags_are_built_only_inside_driver_module() {
        let root = tempdir().unwrap();
        let mut claude = driver(DriverProvider::Claude);
        claude.options.model = Some("sonnet".to_string());
        let command = build_claude_command(
            &request(&claude, root.path()),
            Some(Path::new("/tmp/run/mcp.json")),
        );
        assert!(command.contains("--output-format stream-json"));
        assert!(command.contains("--model 'sonnet'"));
        assert!(command.contains("--mcp-config '/tmp/run/mcp.json'"));

        let codex = driver(DriverProvider::Codex);
        let command = build_codex_command(&request(&codex, root.path()));
        assert!(command.contains("exec --json"));
        assert!(command.ends_with(r"-- 'fix it'\''s tests'"));
    }

    #[test]
    fn per_run_mcp_configs_do_not_share_paths() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let first_path = write_per_run_mcp_config(first.path()).unwrap();
        let second_path = write_per_run_mcp_config(second.path()).unwrap();
        assert_ne!(first_path, second_path);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(first_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn legacy_streaming_compatibility_uses_unique_provider_owned_config() {
        let command = prepare_legacy_claude_streaming_command("claude -p 'do it'", None).unwrap();
        assert!(command.contains("--output-format stream-json"));
        assert!(command.contains("--strict-mcp-config"));
        assert!(command.contains("orch-streaming-"));
    }
}
