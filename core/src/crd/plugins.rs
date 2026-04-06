use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::crd::types::CrdPlugin;
use orchestrator_config::config::RunnerConfig;
use orchestrator_config::plugin_policy::{PluginPolicy, PluginPolicyVerdict};

/// Plugin type: interceptor (gates request processing).
pub const PLUGIN_TYPE_INTERCEPTOR: &str = "interceptor";
/// Plugin type: transformer (modifies payload data).
pub const PLUGIN_TYPE_TRANSFORMER: &str = "transformer";
/// Plugin type: cron (periodic maintenance task).
pub const PLUGIN_TYPE_CRON: &str = "cron";

/// Phase: webhook authentication (runs before signature verification).
pub const PHASE_WEBHOOK_AUTHENTICATE: &str = "webhook.authenticate";
/// Phase: webhook transformation (normalizes payload before trigger matching).
pub const PHASE_WEBHOOK_TRANSFORM: &str = "webhook.transform";

/// Runtime context for plugin execution, carrying sandbox and policy state.
pub struct PluginExecutionContext<'a> {
    /// Runner configuration (shell, shell_arg, policy).
    pub runner: &'a RunnerConfig,
    /// Plugin security policy for runtime re-check and env sanitization.
    pub plugin_policy: &'a PluginPolicy,
    /// SQLite database path for audit logging.
    pub db_path: Option<&'a Path>,
}

/// Execute an interceptor plugin (e.g. custom signature verification).
///
/// The plugin receives context via environment variables:
/// - `PLUGIN_NAME`, `PLUGIN_TYPE`, `CRD_KIND`: plugin identity
/// - `WEBHOOK_BODY`: raw request body
/// - `WEBHOOK_HEADER_<NAME>`: one variable per HTTP header (uppercased, hyphens→underscores)
///
/// Returns Ok(()) if the plugin exits 0 (accept), or Err if non-zero (reject).
pub async fn execute_interceptor(
    plugin: &CrdPlugin,
    crd_kind: &str,
    headers: &HashMap<String, String>,
    body: &str,
    ctx: &PluginExecutionContext<'_>,
) -> Result<()> {
    // Runtime policy re-check (closes TOCTOU gap between CRD apply and execute)
    let verdict = check_runtime_policy(ctx.plugin_policy, plugin)?;
    let (resolved_profile, profile_name) = resolve_plugin_profile(plugin, ctx);

    audit_plugin_execution(
        ctx.db_path,
        "plugin_execute",
        crd_kind,
        plugin,
        &profile_name,
        &verdict,
    );
    let timeout = Duration::from_secs(plugin.effective_timeout());

    let mut cmd = build_plugin_command(ctx, plugin, &resolved_profile)?;

    // Plugin-specific environment
    cmd.env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_INTERCEPTOR)
        .env("CRD_KIND", crd_kind)
        .env("WEBHOOK_BODY", body);

    for (key, value) in headers {
        let env_key = format!("WEBHOOK_HEADER_{}", key.to_uppercase().replace('-', "_"));
        cmd.env(env_key, value);
    }

    let output = run_plugin_with_timeout(&mut cmd, None, timeout)
        .await
        .map_err(|e| {
            audit_plugin_timeout(ctx.db_path, crd_kind, plugin);
            anyhow!(
                "interceptor plugin '{}' for CRD '{}' failed: {}",
                plugin.name,
                crd_kind,
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "interceptor plugin '{}' for CRD '{}' rejected request (exit {}): {}",
            plugin.name,
            crd_kind,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Execute a transformer plugin (e.g. payload normalization).
///
/// The plugin receives:
/// - stdin: the original JSON payload
/// - env: `PLUGIN_NAME`, `PLUGIN_TYPE`, `CRD_KIND`
///
/// Returns the transformed JSON from stdout.
pub async fn execute_transformer(
    plugin: &CrdPlugin,
    crd_kind: &str,
    payload: &serde_json::Value,
    ctx: &PluginExecutionContext<'_>,
) -> Result<serde_json::Value> {
    let verdict = check_runtime_policy(ctx.plugin_policy, plugin)?;
    let (resolved_profile, profile_name) = resolve_plugin_profile(plugin, ctx);

    audit_plugin_execution(
        ctx.db_path,
        "plugin_execute",
        crd_kind,
        plugin,
        &profile_name,
        &verdict,
    );
    let timeout = Duration::from_secs(plugin.effective_timeout());
    let input = serde_json::to_string(payload)
        .map_err(|e| anyhow!("failed to serialize payload for transformer: {}", e))?;

    let mut cmd = build_plugin_command(ctx, plugin, &resolved_profile)?;

    cmd.env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_TRANSFORMER)
        .env("CRD_KIND", crd_kind);

    let output = run_plugin_with_timeout(&mut cmd, Some(input.as_bytes()), timeout)
        .await
        .map_err(|e| {
            audit_plugin_timeout(ctx.db_path, crd_kind, plugin);
            anyhow!(
                "transformer plugin '{}' for CRD '{}' failed: {}",
                plugin.name,
                crd_kind,
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "transformer plugin '{}' for CRD '{}' failed (exit {}): {}",
            plugin.name,
            crd_kind,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| {
        anyhow!(
            "transformer plugin '{}' for CRD '{}' returned invalid JSON: {}",
            plugin.name,
            crd_kind,
            e
        )
    })
}

/// Execute a cron plugin (periodic maintenance task).
///
/// The plugin receives env: `PLUGIN_NAME`, `PLUGIN_TYPE`, `CRD_KIND`.
/// Returns Ok(()) on success, Err on failure (caller should log, not abort).
pub async fn execute_cron_plugin(
    plugin: &CrdPlugin,
    crd_kind: &str,
    ctx: &PluginExecutionContext<'_>,
) -> Result<()> {
    let verdict = check_runtime_policy(ctx.plugin_policy, plugin)?;
    let (resolved_profile, profile_name) = resolve_plugin_profile(plugin, ctx);

    audit_plugin_execution(
        ctx.db_path,
        "plugin_execute",
        crd_kind,
        plugin,
        &profile_name,
        &verdict,
    );
    let timeout = Duration::from_secs(plugin.effective_timeout());

    let mut cmd = build_plugin_command(ctx, plugin, &resolved_profile)?;

    cmd.env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_CRON)
        .env("CRD_KIND", crd_kind);

    let output = run_plugin_with_timeout(&mut cmd, None, timeout)
        .await
        .map_err(|e| {
            audit_plugin_timeout(ctx.db_path, crd_kind, plugin);
            anyhow!(
                "cron plugin '{}' for CRD '{}' failed: {}",
                plugin.name,
                crd_kind,
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "cron plugin '{}' for CRD '{}' failed (exit {}): {}",
            plugin.name,
            crd_kind,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Collect plugins of a given phase from a CRD's plugin list.
pub fn plugins_for_phase<'a>(plugins: &'a [CrdPlugin], phase: &str) -> Vec<&'a CrdPlugin> {
    plugins
        .iter()
        .filter(|p| p.phase.as_deref() == Some(phase))
        .collect()
}

/// Collect cron-type plugins from a CRD's plugin list.
pub fn cron_plugins(plugins: &[CrdPlugin]) -> Vec<&CrdPlugin> {
    plugins
        .iter()
        .filter(|p| p.plugin_type == PLUGIN_TYPE_CRON)
        .collect()
}

// --- internal helpers ---

/// Re-check plugin policy at runtime before execution (TOCTOU defense).
fn check_runtime_policy(policy: &PluginPolicy, plugin: &CrdPlugin) -> Result<PluginPolicyVerdict> {
    let verdict = policy.evaluate_command(&plugin.command);
    if verdict.is_denied() {
        return Err(anyhow!(
            "plugin '{}' command denied at runtime by plugin policy: {}",
            plugin.name,
            verdict.reason().unwrap_or("unknown")
        ));
    }
    if let PluginPolicyVerdict::AuditWarning { ref reason } = verdict {
        tracing::warn!(
            plugin = plugin.name.as_str(),
            reason = reason.as_str(),
            "plugin policy audit warning at runtime"
        );
    }
    Ok(verdict)
}

/// Resolve the effective execution profile for a plugin.
///
/// Priority: per-plugin override > policy default > Host (no sandbox).
fn resolve_plugin_profile(
    plugin: &CrdPlugin,
    ctx: &PluginExecutionContext<'_>,
) -> (crate::runner::ResolvedExecutionProfile, String) {
    let ep_config = plugin
        .execution_profile
        .as_ref()
        .cloned()
        .unwrap_or_else(|| ctx.plugin_policy.effective_execution_profile());

    let name = format!("plugin:{}", plugin.name);
    let resolved = crate::runner::ResolvedExecutionProfile::from_config(
        &name,
        &ep_config,
        std::path::Path::new("/"),
        &[],
    );
    (resolved, name)
}

/// Build the tokio Command for a plugin, applying sandbox wrapping and
/// environment sanitization.
fn build_plugin_command(
    ctx: &PluginExecutionContext<'_>,
    plugin: &CrdPlugin,
    profile: &crate::runner::ResolvedExecutionProfile,
) -> Result<Command> {
    let cwd = std::path::Path::new("/");
    let mut cmd =
        crate::runner::build_command_for_profile(ctx.runner, &plugin.command, cwd, profile)?;

    // Sanitize environment: strip sensitive variable prefixes
    for (key, _) in std::env::vars() {
        let should_deny = ctx
            .plugin_policy
            .effective_env_deny_prefixes()
            .iter()
            .any(|prefix| key.starts_with(prefix));
        if should_deny {
            cmd.env_remove(&key);
        }
    }

    Ok(cmd)
}

// --- audit helpers ---

fn audit_plugin_execution(
    db_path: Option<&Path>,
    action: &str,
    crd_kind: &str,
    plugin: &CrdPlugin,
    sandbox_profile: &str,
    verdict: &PluginPolicyVerdict,
) {
    if let Some(path) = db_path {
        let verdict_str = match verdict {
            PluginPolicyVerdict::Allowed => "allowed",
            PluginPolicyVerdict::Denied { .. } => "denied",
            PluginPolicyVerdict::AuditWarning { .. } => "audit_warning",
        };
        let _ = crate::db::insert_plugin_audit(
            path,
            &crate::db::PluginAuditRecord {
                action: action.into(),
                crd_kind: crd_kind.into(),
                plugin_name: Some(plugin.name.clone()),
                plugin_type: Some(plugin.plugin_type.clone()),
                command: plugin.command.clone(),
                applied_by: None,
                transport: None,
                peer_pid: None,
                result: verdict_str.into(),
                policy_mode: None,
                sandbox_profile: Some(sandbox_profile.into()),
                policy_verdict: Some(verdict_str.into()),
            },
        );
    }
}

/// Spawn a plugin process with process-group isolation and async timeout.
///
/// - Sets `process_group(0)` so the child becomes its own PGID leader (Unix).
/// - Sets `kill_on_drop(true)` as a safety net.
/// - On timeout, kills the entire process group (child + all descendants)
///   via `SIGKILL` to `-pid`, not just the direct child.
/// - Uses `tokio::time::timeout` instead of busy-wait polling.
async fn run_plugin_with_timeout(
    cmd: &mut Command,
    stdin_data: Option<&[u8]>,
    timeout: Duration,
) -> Result<std::process::Output> {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    cmd.kill_on_drop(true);

    if stdin_data.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Apply resource limits from the execution profile (already built into the
    // Command via build_command_for_profile, but process-group and kill_on_drop
    // are plugin-specific additions).

    let mut child = cmd.spawn().map_err(|e| anyhow!("spawn failed: {}", e))?;

    // Write stdin data and close the handle before waiting.
    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(data).await;
            drop(stdin);
        }
    }

    // Take stdout/stderr pipes before waiting so we retain `&mut child` for kill.
    let mut child_stdout = child.stdout.take();
    let mut child_stderr = child.stderr.take();

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            use tokio::io::AsyncReadExt;
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(ref mut p) = child_stdout {
                let _ = p.read_to_end(&mut stdout).await;
            }
            if let Some(ref mut p) = child_stderr {
                let _ = p.read_to_end(&mut stderr).await;
            }
            Ok(std::process::Output {
                status,
                stdout,
                stderr,
            })
        }
        Ok(Err(e)) => Err(anyhow!("wait failed: {}", e)),
        Err(_elapsed) => {
            // Timeout — kill the entire process group, not just the direct child.
            crate::runner::kill_child_process_group(&mut child).await;
            Err(anyhow!("timed out after {}s", timeout.as_secs()))
        }
    }
}

fn audit_plugin_timeout(db_path: Option<&Path>, crd_kind: &str, plugin: &CrdPlugin) {
    if let Some(path) = db_path {
        let _ = crate::db::insert_plugin_audit(
            path,
            &crate::db::PluginAuditRecord {
                action: "plugin_timeout_kill".into(),
                crd_kind: crd_kind.into(),
                plugin_name: Some(plugin.name.clone()),
                plugin_type: Some(plugin.plugin_type.clone()),
                command: plugin.command.clone(),
                applied_by: None,
                transport: None,
                peer_pid: None,
                result: format!("killed_after_{}s", plugin.effective_timeout()),
                policy_mode: None,
                sandbox_profile: None,
                policy_verdict: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::types::CrdPlugin;

    fn make_plugin(name: &str, plugin_type: &str, phase: Option<&str>, command: &str) -> CrdPlugin {
        CrdPlugin {
            name: name.to_string(),
            plugin_type: plugin_type.to_string(),
            phase: phase.map(|s| s.to_string()),
            command: command.to_string(),
            timeout: Some(5),
            schedule: None,
            timezone: None,
            execution_profile: None,
        }
    }

    fn test_ctx() -> (RunnerConfig, PluginPolicy) {
        let runner = RunnerConfig::default();
        let policy = PluginPolicy {
            mode: orchestrator_config::plugin_policy::PluginPolicyMode::Audit,
            ..Default::default()
        };
        (runner, policy)
    }

    #[tokio::test]
    async fn interceptor_accepts_on_exit_zero() {
        let plugin = make_plugin("test", "interceptor", Some("webhook.authenticate"), "true");
        let headers = HashMap::new();
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        assert!(
            execute_interceptor(&plugin, "Foo", &headers, "{}", &ctx)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn interceptor_rejects_on_exit_nonzero() {
        let plugin = make_plugin(
            "test",
            "interceptor",
            Some("webhook.authenticate"),
            "exit 1",
        );
        let headers = HashMap::new();
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("rejected request"));
    }

    #[tokio::test]
    async fn interceptor_passes_headers_and_body() {
        let plugin = make_plugin(
            "check-env",
            "interceptor",
            Some("webhook.authenticate"),
            r#"test "$WEBHOOK_BODY" = '{"ok":true}' && test "$WEBHOOK_HEADER_X_SIG" = "abc""#,
        );
        let mut headers = HashMap::new();
        headers.insert("X-Sig".to_string(), "abc".to_string());
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        assert!(
            execute_interceptor(&plugin, "Foo", &headers, r#"{"ok":true}"#, &ctx)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn transformer_returns_modified_json() {
        // Transformer that wraps input in {"wrapped": <input>}
        let plugin = make_plugin(
            "wrap",
            "transformer",
            Some("webhook.transform"),
            r#"read input; echo "{\"wrapped\":$input}""#,
        );
        let payload = serde_json::json!({"a": 1});
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        let result = execute_transformer(&plugin, "Foo", &payload, &ctx)
            .await
            .unwrap();
        assert!(result.get("wrapped").is_some());
    }

    #[tokio::test]
    async fn transformer_rejects_invalid_json_output() {
        let plugin = make_plugin(
            "bad",
            "transformer",
            Some("webhook.transform"),
            "echo 'not json'",
        );
        let payload = serde_json::json!({});
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        assert!(
            execute_transformer(&plugin, "Foo", &payload, &ctx)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cron_plugin_success() {
        let plugin = make_plugin("daily", "cron", None, "true");
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        assert!(execute_cron_plugin(&plugin, "Foo", &ctx).await.is_ok());
    }

    #[tokio::test]
    async fn cron_plugin_failure() {
        let plugin = make_plugin("daily", "cron", None, "exit 42");
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        assert!(execute_cron_plugin(&plugin, "Foo", &ctx).await.is_err());
    }

    #[test]
    fn plugins_for_phase_filters_correctly() {
        let plugins = vec![
            make_plugin("a", "interceptor", Some("webhook.authenticate"), "true"),
            make_plugin("b", "transformer", Some("webhook.transform"), "cat"),
            make_plugin("c", "interceptor", Some("webhook.authenticate"), "true"),
            make_plugin("d", "cron", None, "true"),
        ];
        let auth = plugins_for_phase(&plugins, PHASE_WEBHOOK_AUTHENTICATE);
        assert_eq!(auth.len(), 2);
        let transform = plugins_for_phase(&plugins, PHASE_WEBHOOK_TRANSFORM);
        assert_eq!(transform.len(), 1);
    }

    #[test]
    fn cron_plugins_filters_correctly() {
        let plugins = vec![
            make_plugin("a", "interceptor", Some("webhook.authenticate"), "true"),
            make_plugin("b", "cron", None, "true"),
            make_plugin("c", "cron", None, "echo hi"),
        ];
        assert_eq!(cron_plugins(&plugins).len(), 2);
    }

    #[tokio::test]
    async fn interceptor_timeout_kills_process() {
        let plugin = CrdPlugin {
            name: "slow".to_string(),
            plugin_type: "interceptor".to_string(),
            phase: Some("webhook.authenticate".to_string()),
            command: "sleep 60".to_string(),
            timeout: Some(1), // 1 second timeout
            schedule: None,
            timezone: None,
            execution_profile: None,
        };
        let headers = HashMap::new();
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    /// Verify that timeout kills the entire process group, not just the direct child.
    /// Spawns a plugin that forks a background grandchild, then asserts the grandchild
    /// is also killed when the plugin times out.
    #[tokio::test]
    async fn timeout_kills_entire_process_group() {
        let pid_file =
            std::env::temp_dir().join(format!("plugin_pgkill_test_{}", std::process::id()));
        let command = format!(
            // Fork a background child that writes its PID to a file, then sleep forever.
            // The parent also sleeps forever. On timeout, both should be killed.
            r#"sh -c 'echo $$ > {}; sleep 3600' & sleep 3600"#,
            pid_file.display()
        );
        let plugin = CrdPlugin {
            name: "pgkill".to_string(),
            plugin_type: "interceptor".to_string(),
            phase: Some("webhook.authenticate".to_string()),
            command,
            timeout: Some(1),
            schedule: None,
            timezone: None,
            execution_profile: None,
        };
        let headers = HashMap::new();
        let (runner, policy) = test_ctx();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));

        // Give the OS a moment to reap.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Read the grandchild PID and verify it's no longer running.
        if let Ok(pid_str) = std::fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                #[cfg(unix)]
                {
                    // SAFETY: kill(pid, 0) checks if process exists without
                    // sending a signal. The pid is a valid i32 parsed from the
                    // grandchild's PID file written earlier in this test.
                    let alive = unsafe { libc::kill(pid, 0) };
                    assert_ne!(alive, 0, "grandchild process {} should be dead", pid);
                }
            }
        }
        let _ = std::fs::remove_file(&pid_file);
    }

    #[tokio::test]
    async fn runtime_policy_denial_blocks_execution() {
        let plugin = make_plugin(
            "blocked",
            "interceptor",
            Some("webhook.authenticate"),
            "scripts/verify.sh",
        );
        let headers = HashMap::new();
        let runner = RunnerConfig::default();
        // Deny mode blocks all commands at runtime
        let policy = PluginPolicy {
            mode: orchestrator_config::plugin_policy::PluginPolicyMode::Deny,
            ..Default::default()
        };
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("denied at runtime"));
    }

    #[test]
    fn profile_resolution_prefers_plugin_override() {
        use orchestrator_config::config::{ExecutionProfileConfig, ExecutionProfileMode};

        let runner = RunnerConfig::default();
        let policy = PluginPolicy::default();
        let ctx = PluginExecutionContext {
            runner: &runner,
            plugin_policy: &policy,
            db_path: None,
        };

        // Plugin with explicit sandbox profile
        let mut plugin = make_plugin(
            "sandboxed",
            "interceptor",
            Some("webhook.authenticate"),
            "true",
        );
        plugin.execution_profile = Some(ExecutionProfileConfig {
            mode: ExecutionProfileMode::Sandbox,
            ..Default::default()
        });

        let (resolved, _name) = resolve_plugin_profile(&plugin, &ctx);
        assert_eq!(resolved.mode, ExecutionProfileMode::Sandbox);

        // Plugin without override falls back to policy default (Host)
        let plain = make_plugin("plain", "interceptor", Some("webhook.authenticate"), "true");
        let (resolved, _name) = resolve_plugin_profile(&plain, &ctx);
        assert_eq!(resolved.mode, ExecutionProfileMode::Host);
    }
}
