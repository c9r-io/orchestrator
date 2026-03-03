use anyhow::{anyhow, Result};
use std::process::Command;

/// Execute a lifecycle hook command synchronously.
///
/// Context is passed via environment variables:
/// - `RESOURCE_KIND`: the CRD kind name
/// - `RESOURCE_NAME`: the resource instance name
/// - `RESOURCE_ACTION`: "create", "update", or "delete"
/// - `RESOURCE_SPEC`: JSON string of the spec
///
/// Hook failure (non-zero exit or execution error) blocks the operation.
pub fn execute_hook(
    hook_command: &str,
    kind: &str,
    name: &str,
    action: &str,
    spec: &serde_json::Value,
) -> Result<()> {
    let spec_json = serde_json::to_string(spec)
        .map_err(|e| anyhow!("failed to serialize spec for hook: {}", e))?;

    let output = Command::new("sh")
        .arg("-c")
        .arg(hook_command)
        .env("RESOURCE_KIND", kind)
        .env("RESOURCE_NAME", name)
        .env("RESOURCE_ACTION", action)
        .env("RESOURCE_SPEC", &spec_json)
        .output()
        .map_err(|e| {
            anyhow!(
                "failed to execute {} hook for {}/{}: {}",
                action,
                kind,
                name,
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "{} hook for {}/{} failed (exit {}): {}",
            action,
            kind,
            name,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(())
}

/// Select and execute the appropriate hook for an action, if defined.
pub fn run_hook_if_defined(
    hooks: &crate::crd::types::CrdHooks,
    kind: &str,
    name: &str,
    action: &str,
    spec: &serde_json::Value,
) -> Result<()> {
    let hook_cmd = match action {
        "create" => hooks.on_create.as_deref(),
        "update" => hooks.on_update.as_deref(),
        "delete" => hooks.on_delete.as_deref(),
        _ => None,
    };

    if let Some(cmd) = hook_cmd {
        execute_hook(cmd, kind, name, action, spec)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::types::CrdHooks;

    #[test]
    fn execute_hook_success() {
        let result = execute_hook(
            "true", // always succeeds
            "Foo",
            "my-foo",
            "create",
            &serde_json::json!({"key": "value"}),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn execute_hook_failure() {
        let result = execute_hook("exit 1", "Foo", "my-foo", "create", &serde_json::json!({}));
        assert!(result.is_err());
        assert!(result
            .expect_err("should fail")
            .to_string()
            .contains("hook"));
    }

    #[test]
    fn execute_hook_passes_env_vars() {
        let result = execute_hook(
            r#"test "$RESOURCE_KIND" = "Bar" && test "$RESOURCE_NAME" = "baz" && test "$RESOURCE_ACTION" = "delete""#,
            "Bar",
            "baz",
            "delete",
            &serde_json::json!({}),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn run_hook_if_defined_skips_when_none() {
        let hooks = CrdHooks::default();
        assert!(run_hook_if_defined(&hooks, "Foo", "x", "create", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn run_hook_if_defined_runs_on_create() {
        let hooks = CrdHooks {
            on_create: Some("true".to_string()),
            on_update: None,
            on_delete: None,
        };
        assert!(run_hook_if_defined(&hooks, "Foo", "x", "create", &serde_json::json!({})).is_ok());
    }

    #[test]
    fn run_hook_if_defined_ignores_unknown_action() {
        let hooks = CrdHooks {
            on_create: Some("exit 1".to_string()),
            on_update: None,
            on_delete: None,
        };
        // "unknown" action has no hook, so it should succeed
        assert!(run_hook_if_defined(&hooks, "Foo", "x", "unknown", &serde_json::json!({})).is_ok());
    }
}
