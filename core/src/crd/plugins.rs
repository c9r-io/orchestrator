use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::crd::types::CrdPlugin;

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
    db_path: Option<&Path>,
) -> Result<()> {
    audit_plugin_execution(db_path, "plugin_execute", crd_kind, plugin);
    let timeout = Duration::from_secs(plugin.effective_timeout());

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&plugin.command)
        .env("PLUGIN_NAME", &plugin.name)
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
            audit_plugin_timeout(db_path, crd_kind, plugin, &e);
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
    db_path: Option<&Path>,
) -> Result<serde_json::Value> {
    audit_plugin_execution(db_path, "plugin_execute", crd_kind, plugin);
    let timeout = Duration::from_secs(plugin.effective_timeout());
    let input = serde_json::to_string(payload)
        .map_err(|e| anyhow!("failed to serialize payload for transformer: {}", e))?;

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&plugin.command)
        .env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_TRANSFORMER)
        .env("CRD_KIND", crd_kind);

    let output = run_plugin_with_timeout(&mut cmd, Some(input.as_bytes()), timeout)
        .await
        .map_err(|e| {
            audit_plugin_timeout(db_path, crd_kind, plugin, &e);
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
    db_path: Option<&Path>,
) -> Result<()> {
    audit_plugin_execution(db_path, "plugin_execute", crd_kind, plugin);
    let timeout = Duration::from_secs(plugin.effective_timeout());

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&plugin.command)
        .env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_CRON)
        .env("CRD_KIND", crd_kind);

    let output = run_plugin_with_timeout(&mut cmd, None, timeout)
        .await
        .map_err(|e| {
            audit_plugin_timeout(db_path, crd_kind, plugin, &e);
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

// --- audit helper ---

fn audit_plugin_execution(
    db_path: Option<&Path>,
    action: &str,
    crd_kind: &str,
    plugin: &CrdPlugin,
) {
    if let Some(path) = db_path {
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
                result: "allowed".into(),
                policy_mode: None,
            },
        );
    }
}

// --- internal helpers ---

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

fn audit_plugin_timeout(
    db_path: Option<&Path>,
    crd_kind: &str,
    plugin: &CrdPlugin,
    error: &anyhow::Error,
) {
    if !error.to_string().contains("timed out") {
        return;
    }
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
        }
    }

    #[tokio::test]
    async fn interceptor_accepts_on_exit_zero() {
        let plugin = make_plugin("test", "interceptor", Some("webhook.authenticate"), "true");
        let headers = HashMap::new();
        assert!(
            execute_interceptor(&plugin, "Foo", &headers, "{}", None)
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
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", None)
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
        assert!(
            execute_interceptor(&plugin, "Foo", &headers, r#"{"ok":true}"#, None)
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
        let result = execute_transformer(&plugin, "Foo", &payload, None)
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
        assert!(
            execute_transformer(&plugin, "Foo", &payload, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cron_plugin_success() {
        let plugin = make_plugin("daily", "cron", None, "true");
        assert!(execute_cron_plugin(&plugin, "Foo", None).await.is_ok());
    }

    #[tokio::test]
    async fn cron_plugin_failure() {
        let plugin = make_plugin("daily", "cron", None, "exit 42");
        assert!(execute_cron_plugin(&plugin, "Foo", None).await.is_err());
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
        };
        let headers = HashMap::new();
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", None)
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
        };
        let headers = HashMap::new();
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", None)
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
}
