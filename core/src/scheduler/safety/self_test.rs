use crate::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use tracing::error;

/// Executes the builtin self-test step against the orchestrator workspace.
pub async fn execute_self_test_step(
    workspace_root: &Path,
    state: &InnerState,
    task_id: &str,
    item_id: &str,
    project_id: Option<&str>,
) -> Result<i64> {
    let cargo_bin = std::env::var("ORCH_SELF_TEST_CARGO").unwrap_or_else(|_| "cargo".to_string());

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check"}),
    );
    let check_output = tokio::process::Command::new(&cargo_bin)
        .args(["check", "--workspace", "--message-format=short"])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to run cargo check")?;

    if !check_output.status.success() {
        let stderr = String::from_utf8_lossy(&check_output.stderr);
        error!(phase = "cargo_check", stderr = %stderr.trim(), "self-test phase failed");
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "cargo_check", "passed": false}),
        );
        return Ok(check_output.status.code().unwrap_or(1) as i64);
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check", "passed": true}),
    );

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_test_lib"}),
    );
    let test_output = tokio::process::Command::new(&cargo_bin)
        .args([
            "test",
            "--lib",
            "-p",
            "agent-orchestrator",
            "--",
            "--skip",
            "self_test_survives_smoke_test",
        ])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to run cargo test --lib")?;

    if !test_output.status.success() {
        let stderr = String::from_utf8_lossy(&test_output.stderr);
        error!(
            phase = "cargo_test_lib",
            stderr = %stderr.trim(),
            "self-test phase failed"
        );
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "cargo_test_lib", "passed": false}),
        );
        return Ok(test_output.status.code().unwrap_or(1) as i64);
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_test_lib", "passed": true}),
    );

    // Manifest validate — use direct library call instead of shelling out
    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "manifest_validate"}),
    );
    let manifest_path = workspace_root.join("docs/workflow/self-bootstrap.yaml");
    if manifest_path.exists() {
        let validate_passed = match std::fs::read_to_string(&manifest_path) {
            Ok(content) => {
                match crate::service::system::validate_manifests(state, &content, project_id) {
                    Ok(report) => {
                        if !report.valid {
                            for err in &report.errors {
                                error!(phase = "manifest_validate", error = %err, "validation error");
                            }
                        }
                        report.valid
                    }
                    Err(e) => {
                        error!(phase = "manifest_validate", error = %e, "validation failed");
                        false
                    }
                }
            }
            Err(e) => {
                error!(phase = "manifest_validate", error = %e, "failed to read manifest");
                false
            }
        };

        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "manifest_validate", "passed": validate_passed}),
        );
        if !validate_passed {
            return Ok(1);
        }
    }

    Ok(0)
}
