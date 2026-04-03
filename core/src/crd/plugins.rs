use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

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
pub fn execute_interceptor(
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

    let output = run_with_timeout(&mut cmd, timeout).map_err(|e| {
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
pub fn execute_transformer(
    plugin: &CrdPlugin,
    crd_kind: &str,
    payload: &serde_json::Value,
    db_path: Option<&Path>,
) -> Result<serde_json::Value> {
    audit_plugin_execution(db_path, "plugin_execute", crd_kind, plugin);
    let timeout = Duration::from_secs(plugin.effective_timeout());
    let input = serde_json::to_string(payload)
        .map_err(|e| anyhow!("failed to serialize payload for transformer: {}", e))?;

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&plugin.command)
        .env("PLUGIN_NAME", &plugin.name)
        .env("PLUGIN_TYPE", PLUGIN_TYPE_TRANSFORMER)
        .env("CRD_KIND", crd_kind)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            anyhow!(
                "failed to spawn transformer plugin '{}' for CRD '{}': {}",
                plugin.name,
                crd_kind,
                e
            )
        })?;

    // Write payload to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    let output = wait_with_timeout(&mut child, timeout).map_err(|e| {
        let _ = child.kill();
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
pub fn execute_cron_plugin(
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

    let output = run_with_timeout(&mut cmd, timeout).map_err(|e| {
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

fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<std::process::Output> {
    let mut child = cmd.spawn().map_err(|e| anyhow!("spawn failed: {}", e))?;
    wait_with_timeout(&mut child, timeout)
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .as_mut()
                    .map(|s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .as_mut()
                    .map(|s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(anyhow!("timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(anyhow!("wait failed: {}", e)),
        }
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

    #[test]
    fn interceptor_accepts_on_exit_zero() {
        let plugin = make_plugin("test", "interceptor", Some("webhook.authenticate"), "true");
        let headers = HashMap::new();
        assert!(execute_interceptor(&plugin, "Foo", &headers, "{}", None).is_ok());
    }

    #[test]
    fn interceptor_rejects_on_exit_nonzero() {
        let plugin = make_plugin(
            "test",
            "interceptor",
            Some("webhook.authenticate"),
            "exit 1",
        );
        let headers = HashMap::new();
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", None).unwrap_err();
        assert!(err.to_string().contains("rejected request"));
    }

    #[test]
    fn interceptor_passes_headers_and_body() {
        let plugin = make_plugin(
            "check-env",
            "interceptor",
            Some("webhook.authenticate"),
            r#"test "$WEBHOOK_BODY" = '{"ok":true}' && test "$WEBHOOK_HEADER_X_SIG" = "abc""#,
        );
        let mut headers = HashMap::new();
        headers.insert("X-Sig".to_string(), "abc".to_string());
        assert!(execute_interceptor(&plugin, "Foo", &headers, r#"{"ok":true}"#, None).is_ok());
    }

    #[test]
    fn transformer_returns_modified_json() {
        // Transformer that wraps input in {"wrapped": <input>}
        let plugin = make_plugin(
            "wrap",
            "transformer",
            Some("webhook.transform"),
            r#"read input; echo "{\"wrapped\":$input}""#,
        );
        let payload = serde_json::json!({"a": 1});
        let result = execute_transformer(&plugin, "Foo", &payload, None).unwrap();
        assert!(result.get("wrapped").is_some());
    }

    #[test]
    fn transformer_rejects_invalid_json_output() {
        let plugin = make_plugin(
            "bad",
            "transformer",
            Some("webhook.transform"),
            "echo 'not json'",
        );
        let payload = serde_json::json!({});
        assert!(execute_transformer(&plugin, "Foo", &payload, None).is_err());
    }

    #[test]
    fn cron_plugin_success() {
        let plugin = make_plugin("daily", "cron", None, "true");
        assert!(execute_cron_plugin(&plugin, "Foo", None).is_ok());
    }

    #[test]
    fn cron_plugin_failure() {
        let plugin = make_plugin("daily", "cron", None, "exit 42");
        assert!(execute_cron_plugin(&plugin, "Foo", None).is_err());
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

    #[test]
    fn interceptor_timeout_kills_process() {
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
        let err = execute_interceptor(&plugin, "Foo", &headers, "{}", None).unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }
}
