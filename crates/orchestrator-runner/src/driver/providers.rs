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
        let child = spawn_command(
            &request,
            request.shell_command,
            request.stdin_payload.is_some(),
        )?;
        Ok(Box::new(
            ProcessSession::start(
                DriverProvider::Shell,
                child,
                request.stdout,
                request.stderr,
                request.redaction_patterns.to_vec(),
                request.stdin_payload.map(str::to_string),
                true,
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
            Some(write_per_run_mcp_config(
                request.artifacts_dir,
                request.mcp_callback,
            )?)
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
                false,
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
                false,
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
    if let Some(codex) = &request.driver.codex
        && let Some(effort) = &codex.reasoning_effort
    {
        args.extend([
            "--config".to_string(),
            quote(&format!("model_reasoning_effort={effort}")),
        ]);
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

fn write_per_run_mcp_config(
    artifacts_dir: &Path,
    callback: Option<&super::McpCallbackConfig>,
) -> Result<PathBuf> {
    let directory = artifacts_dir.join("driver");
    std::fs::create_dir_all(&directory).with_context(|| {
        format!(
            "creating driver artifacts directory {}",
            directory.display()
        )
    })?;
    let path = directory.join("mcp.json");
    let binary = resolve_mcp_tools_bin()?;
    let mut server = json!({"command":binary,"args":[]});
    if let Some(callback) = callback {
        server["env"] = json!({
            "ORCH_MCP_CALLBACK_URL": callback.url(),
            "ORCH_MCP_CALLBACK_TOKEN": callback.expose_token(),
        });
    }
    let value = json!({"mcpServers":{"orch":server}});
    std::fs::write(&path, value.to_string())
        .with_context(|| format!("writing per-run MCP config {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

fn resolve_mcp_tools_bin() -> Result<PathBuf> {
    if let Ok(value) = std::env::var("ORCH_MCP_TOOLS_BIN")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
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
    use crate::driver::{DriverEvent, DriverOutcome, SessionRef};
    use orchestrator_config::config::{AgentDriverConfig, DriverOptions, DriverTransport};
    use std::collections::HashMap;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    use tempfile::tempdir;
    use tokio_stream::StreamExt as _;

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
            shell_command: "echo ok",
            stdin_payload: None,
            prompt: "fix it's tests",
            cwd,
            stdout: tempfile::tempfile().unwrap(),
            stderr: tempfile::tempfile().unwrap(),
            redaction_patterns: &[],
            extra_env: Box::leak(Box::new(HashMap::new())),
            execution_profile: Box::leak(Box::new(crate::runner::ResolvedExecutionProfile::host())),
            artifacts_dir: cwd,
            session_ref: None,
            mcp_callback: None,
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

    #[tokio::test]
    async fn shell_driver_delivers_stdin_payload_and_closes_stdin() {
        let root = tempdir().unwrap();
        let stdout_path = root.path().join("stdout.log");
        let stderr_path = root.path().join("stderr.log");
        let config = driver(DriverProvider::Shell);
        let runner = orchestrator_config::config::RunnerConfig {
            policy: orchestrator_config::config::RunnerPolicy::Unsafe,
            ..Default::default()
        };
        let extra_env = HashMap::new();
        let profile = crate::runner::ResolvedExecutionProfile::host();
        let mut session = ShellCliDriver
            .start(DriverStartRequest {
                driver: &config,
                runner: &runner,
                shell_command: "cat",
                stdin_payload: Some("prompt over stdin"),
                prompt: "prompt over stdin",
                cwd: root.path(),
                stdout: std::fs::File::create(&stdout_path).unwrap(),
                stderr: std::fs::File::create(&stderr_path).unwrap(),
                redaction_patterns: &[],
                extra_env: &extra_env,
                execution_profile: &profile,
                artifacts_dir: root.path(),
                session_ref: None,
                mcp_callback: None,
            })
            .await
            .unwrap();

        let mut events = session.take_events().unwrap();
        while let Some(event) = events.next().await {
            if matches!(event.unwrap(), DriverEvent::Finished { .. }) {
                break;
            }
        }
        assert_eq!(
            std::fs::read_to_string(stdout_path).unwrap(),
            "prompt over stdin\n"
        );
    }

    /// The assertion whose absence let the CPU-limit classifier sit dead.
    ///
    /// `detect_resource_exceeded` keys the CPU case on `exit_signal`, and it had
    /// a green unit test the whole time — one that hand-built a `WaitResult`
    /// carrying a signal no production path could produce. The gap was between
    /// the process and that struct, which is where this test looks: a real child
    /// killed by a real signal must report the signal.
    #[cfg(unix)]
    #[tokio::test]
    async fn shell_driver_reports_the_signal_that_killed_the_child() {
        let (outcome, exit_code, exit_signal) = run_shell_to_completion("kill -s XCPU $$").await;

        assert_eq!(outcome, DriverOutcome::Failed);
        assert_eq!(
            exit_signal,
            Some(libc::SIGXCPU),
            "a signal-killed child must report the signal that killed it"
        );
        assert_eq!(
            exit_code, -1,
            "a signal-killed child has no exit code; -1 is what the non-driver wait path reports"
        );
    }

    /// The other direction, and the one the repair could break: an ordinary
    /// non-zero exit must not acquire a signal. Without this, mapping "no exit
    /// code" onto a signal-shaped value would pass the test above while
    /// classifying every failed step as a resource kill.
    #[cfg(unix)]
    #[tokio::test]
    async fn shell_driver_reports_no_signal_for_an_ordinary_failure() {
        let (outcome, exit_code, exit_signal) = run_shell_to_completion("exit 3").await;

        assert_eq!(outcome, DriverOutcome::Failed);
        assert_eq!(exit_code, 3);
        assert_eq!(
            exit_signal, None,
            "a process that exited on its own was not killed by anything"
        );
    }

    #[cfg(unix)]
    async fn run_shell_to_completion(shell_command: &str) -> (DriverOutcome, i32, Option<i32>) {
        let root = tempdir().unwrap();
        let config = driver(DriverProvider::Shell);
        let runner = orchestrator_config::config::RunnerConfig {
            policy: orchestrator_config::config::RunnerPolicy::Unsafe,
            ..Default::default()
        };
        let extra_env = HashMap::new();
        let profile = crate::runner::ResolvedExecutionProfile::host();
        let mut session = ShellCliDriver
            .start(DriverStartRequest {
                driver: &config,
                runner: &runner,
                shell_command,
                stdin_payload: None,
                prompt: "",
                cwd: root.path(),
                stdout: std::fs::File::create(root.path().join("stdout.log")).unwrap(),
                stderr: std::fs::File::create(root.path().join("stderr.log")).unwrap(),
                redaction_patterns: &[],
                extra_env: &extra_env,
                execution_profile: &profile,
                artifacts_dir: root.path(),
                session_ref: None,
                mcp_callback: None,
            })
            .await
            .unwrap();

        let mut events = session.take_events().unwrap();
        while let Some(event) = events.next().await {
            if let DriverEvent::Finished {
                outcome,
                exit_code,
                exit_signal,
            } = event.unwrap()
            {
                return (outcome, exit_code, exit_signal);
            }
        }
        panic!("driver stream ended without a terminal event");
    }

    #[test]
    fn codex_resume_command_matches_certified_cli_grammar() {
        let root = tempdir().unwrap();
        let codex = driver(DriverProvider::Codex);
        let session =
            SessionRef::from_provider("01900000-0000-7000-8000-000000000001".to_string()).unwrap();
        let mut resume = request(&codex, root.path());
        resume.session_ref = Some(&session);

        assert_eq!(
            build_codex_command(&resume),
            "'codex' exec resume '01900000-0000-7000-8000-000000000001' --json -- \
             'fix it'\\''s tests'"
        );
    }

    #[test]
    fn per_run_mcp_configs_do_not_share_paths() {
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let first_path = write_per_run_mcp_config(first.path(), None).unwrap();
        let second_path = write_per_run_mcp_config(second.path(), None).unwrap();
        assert_ne!(first_path, second_path);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(first_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn private_mcp_config_carries_run_scoped_callback() {
        let directory = tempdir().unwrap();
        let callback = super::super::McpCallbackConfig::new(
            "http://127.0.0.1:19118/mcp".to_string(),
            "secret-118".to_string(),
        )
        .unwrap();
        let path = write_per_run_mcp_config(directory.path(), Some(&callback)).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            value["mcpServers"]["orch"]["env"]["ORCH_MCP_CALLBACK_URL"],
            callback.url()
        );
        assert_eq!(
            value["mcpServers"]["orch"]["env"]["ORCH_MCP_CALLBACK_TOKEN"],
            callback.expose_token()
        );
    }
}
