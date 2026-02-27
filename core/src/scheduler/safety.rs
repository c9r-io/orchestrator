use crate::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};

pub async fn create_checkpoint(workspace_root: &Path, task_id: &str, cycle: u32) -> Result<String> {
    let tag_name = format!("checkpoint/{}/{}", task_id, cycle);
    let output = tokio::process::Command::new("git")
        .args(["tag", "-f", &tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to create git checkpoint tag")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git tag failed: {}", stderr);
    }
    Ok(tag_name)
}

pub async fn rollback_to_checkpoint(workspace_root: &Path, tag_name: &str) -> Result<()> {
    let output = tokio::process::Command::new("git")
        .args(["reset", "--hard", tag_name])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to rollback to checkpoint")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr);
    }
    Ok(())
}

pub async fn snapshot_binary(workspace_root: &Path) -> Result<PathBuf> {
    let binary_path = workspace_root.join("core/target/release/agent-orchestrator");
    let stable_path = workspace_root.join(".stable");

    if !binary_path.exists() {
        anyhow::bail!(
            "release binary not found at {}; skipping binary snapshot",
            binary_path.display()
        );
    }

    tokio::fs::copy(&binary_path, &stable_path)
        .await
        .with_context(|| {
            format!(
                "failed to snapshot binary from {} to {}",
                binary_path.display(),
                stable_path.display()
            )
        })?;

    Ok(stable_path)
}

pub async fn restore_binary_snapshot(workspace_root: &Path) -> Result<()> {
    let stable_path = workspace_root.join(".stable");
    let binary_path = workspace_root.join("core/target/release/agent-orchestrator");

    if !stable_path.exists() {
        anyhow::bail!(
            "no .stable binary snapshot found at {}",
            stable_path.display()
        );
    }

    tokio::fs::copy(&stable_path, &binary_path)
        .await
        .with_context(|| {
            format!(
                "failed to restore binary snapshot from {} to {}",
                stable_path.display(),
                binary_path.display()
            )
        })?;

    Ok(())
}

pub async fn execute_self_test_step(
    workspace_root: &Path,
    state: &InnerState,
    task_id: &str,
    item_id: &str,
) -> Result<i64> {
    let core_dir = workspace_root.join("core");

    state.emit_event(
        task_id,
        Some(item_id),
        "self_test_phase",
        json!({"phase": "cargo_check"}),
    );
    let check_output = tokio::process::Command::new("cargo")
        .args(["check", "--message-format=short"])
        .current_dir(&core_dir)
        .output()
        .await
        .context("failed to run cargo check")?;

    if !check_output.status.success() {
        let stderr = String::from_utf8_lossy(&check_output.stderr);
        eprintln!("[self_test] cargo check failed:\n{}", stderr);
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
    let test_output = tokio::process::Command::new("cargo")
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
        eprintln!("[self_test] cargo test --lib failed:\n{}", stderr);
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
            eprintln!("[self_test] manifest validate failed:\n{}", stderr);
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
