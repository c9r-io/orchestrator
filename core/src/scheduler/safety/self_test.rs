use crate::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;
use tracing::error;

pub async fn execute_self_test_step(
    workspace_root: &Path,
    state: &InnerState,
    task_id: &str,
    item_id: &str,
) -> Result<i64> {
    let core_dir = workspace_root.join("core");
    let cargo_bin = std::env::var("ORCH_SELF_TEST_CARGO").unwrap_or_else(|_| "cargo".to_string());

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check"}),
    );
    let check_output = tokio::process::Command::new(&cargo_bin)
        .args(["check", "--message-format=short"])
        .current_dir(&core_dir)
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
            "--",
            "--skip",
            "self_test_survives_smoke_test",
        ])
        .current_dir(&core_dir)
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

    let script_path = workspace_root.join("scripts/orchestrator.sh");
    if script_path.exists() {
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "manifest_validate"}),
        );
        let validate_output = tokio::process::Command::new(&script_path)
            .args([
                "manifest",
                "validate",
                "-f",
                "docs/workflow/self-bootstrap.yaml",
            ])
            .current_dir(workspace_root)
            .output()
            .await
            .context("failed to run manifest validate")?;

        if !validate_output.status.success() {
            let stderr = String::from_utf8_lossy(&validate_output.stderr);
            error!(
                phase = "manifest_validate",
                stderr = %stderr.trim(),
                "self-test phase failed"
            );
            state.emit_event(
                task_id,
                Some(item_id),
                "self_test_phase",
                json!({"phase": "manifest_validate", "passed": false}),
            );
            return Ok(validate_output.status.code().unwrap_or(1) as i64);
        }
        state.emit_event(
            task_id,
            Some(item_id),
            "self_test_phase",
            json!({"phase": "manifest_validate", "passed": true}),
        );
    }

    Ok(0)
}
