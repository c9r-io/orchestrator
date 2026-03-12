use super::snapshot::{sha256_hex, snapshot_binary, RELEASE_BINARY_REL};
use crate::async_database::flatten_err;
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::error;

/// Exit code that signals the process wrapper to relaunch the binary.
/// Uses sysexits.h EX_TEMPFAIL (75) — "temporary failure, try again."
/// Kept as fallback for the CLI foreground restart loop.
pub const EXIT_RESTART: i64 = 75;

/// Outcome of `execute_self_restart_step`.
/// Core returns this signal; the daemon layer decides how to act on it.
#[derive(Debug)]
pub enum SelfRestartOutcome {
    /// Build, verify, and snapshot succeeded. Daemon should exec the new binary.
    RestartReady {
        /// Path to the verified release binary that should be executed next.
        binary_path: PathBuf,
    },
    /// A phase failed. The returned code is the step exit code (non-75).
    Failed(i64),
}

/// Sentinel error to propagate a restart signal up the call stack.
/// Daemon layer catches this via `downcast_ref` and performs `exec()`.
#[derive(Debug)]
pub struct RestartRequestedError {
    /// Path to the verified release binary that should replace the current process.
    pub binary_path: PathBuf,
}

impl std::fmt::Display for RestartRequestedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "restart requested: exec {}", self.binary_path.display())
    }
}

impl std::error::Error for RestartRequestedError {}

/// Self-restart builtin step: rebuild the binary, verify it, snapshot .stable,
/// set task status to restart_pending, and return `SelfRestartOutcome` so the
/// daemon can exec the new binary (or the CLI wrapper can fallback-restart).
pub async fn execute_self_restart_step(
    workspace_root: &Path,
    state: &InnerState,
    task_id: &str,
    item_id: &str,
) -> Result<SelfRestartOutcome> {
    let cargo_bin = std::env::var("ORCH_SELF_TEST_CARGO").unwrap_or_else(|_| "cargo".to_string());
    let binary_path = workspace_root.join(RELEASE_BINARY_REL);

    // Phase 1: cargo build --release -p orchestratord
    state.emit_event(
        task_id,
        Some(item_id),
        "self_restart_phase",
        json!({"phase": "cargo_build_release"}),
    );
    let build_output = tokio::process::Command::new(&cargo_bin)
        .args(["build", "--release", "-p", "orchestratord"])
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to run cargo build --release")?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        error!(phase = "cargo_build_release", stderr = %stderr.trim(), "self-restart build failed");
        state.emit_event(
            task_id,
            Some(item_id),
            "self_restart_phase",
            json!({"phase": "cargo_build_release", "passed": false}),
        );
        return Ok(SelfRestartOutcome::Failed(
            build_output.status.code().unwrap_or(1) as i64,
        ));
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_restart_phase",
        json!({"phase": "cargo_build_release", "passed": true}),
    );

    // Phase 2: verify new binary responds to --help (timeout 30s)
    // macOS Gatekeeper / code signing checks on first cold launch can exceed 10s.
    state.emit_event(
        task_id,
        Some(item_id),
        "self_restart_phase",
        json!({"phase": "verify_binary"}),
    );
    let verify_timeout_secs: u64 = std::env::var("ORCH_VERIFY_BINARY_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let verify_result = tokio::time::timeout(
        std::time::Duration::from_secs(verify_timeout_secs),
        tokio::process::Command::new(&binary_path)
            .arg("--help")
            .output(),
    )
    .await;

    match verify_result {
        Ok(Ok(output)) if output.status.success() => {
            state.emit_event(
                task_id,
                Some(item_id),
                "self_restart_phase",
                json!({"phase": "verify_binary", "passed": true}),
            );
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(phase = "verify_binary", stderr = %stderr.trim(), "new binary --help failed");
            state.emit_event(
                task_id,
                Some(item_id),
                "self_restart_phase",
                json!({"phase": "verify_binary", "passed": false}),
            );
            return Ok(SelfRestartOutcome::Failed(1));
        }
        Ok(Err(e)) => {
            error!(phase = "verify_binary", error = %e, "failed to execute new binary");
            state.emit_event(
                task_id,
                Some(item_id),
                "self_restart_phase",
                json!({"phase": "verify_binary", "passed": false, "error": e.to_string()}),
            );
            return Ok(SelfRestartOutcome::Failed(1));
        }
        Err(_) => {
            error!(
                phase = "verify_binary",
                timeout_secs = verify_timeout_secs,
                "new binary --help timed out"
            );
            state.emit_event(
                task_id,
                Some(item_id),
                "self_restart_phase",
                json!({"phase": "verify_binary", "passed": false, "error": "timeout"}),
            );
            return Ok(SelfRestartOutcome::Failed(1));
        }
    }

    // Capture the SHA256 of the currently running binary before replacing it
    let old_binary_sha256 = match std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::read(p).ok())
    {
        Some(content) => sha256_hex(&content),
        None => "unknown".to_string(),
    };

    // Phase 3: snapshot the new binary as .stable
    state.emit_event(
        task_id,
        Some(item_id),
        "self_restart_phase",
        json!({"phase": "snapshot_binary"}),
    );
    let current_cycle: u32 = crate::db::open_conn(&state.db_path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT current_cycle FROM tasks WHERE id = ?1",
                rusqlite::params![task_id],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(0) as u32;

    if let Err(e) = snapshot_binary(workspace_root, task_id, current_cycle).await {
        error!(phase = "snapshot_binary", error = %e, "snapshot failed");
        state.emit_event(
            task_id,
            Some(item_id),
            "self_restart_phase",
            json!({"phase": "snapshot_binary", "passed": false, "error": e.to_string()}),
        );
        return Ok(SelfRestartOutcome::Failed(1));
    }
    state.emit_event(
        task_id,
        Some(item_id),
        "self_restart_phase",
        json!({"phase": "snapshot_binary", "passed": true}),
    );

    // Compute SHA256 of the newly built binary for post-restart verification
    let new_binary_sha256 = match tokio::fs::read(&binary_path).await {
        Ok(content) => sha256_hex(&content),
        Err(_) => "unknown".to_string(),
    };

    // Phase 4: set task status to restart_pending
    state
        .db_writer
        .set_task_status(task_id, "restart_pending", false)
        .await?;

    // Persist the event to SQLite (insert_event) so the new process can verify
    // it is running the expected binary by comparing SHA256.
    insert_event(
        state,
        task_id,
        Some(item_id),
        "self_restart_ready",
        json!({
            "exit_code": EXIT_RESTART,
            "old_binary_sha256": old_binary_sha256,
            "new_binary_sha256": new_binary_sha256,
            "binary_changed": old_binary_sha256 != new_binary_sha256
                && old_binary_sha256 != "unknown"
                && new_binary_sha256 != "unknown",
            "binary_path": binary_path.to_string_lossy(),
            "build_git_hash": env!("BUILD_GIT_HASH"),
            "build_timestamp": env!("BUILD_TIMESTAMP"),
        }),
    )
    .await?;

    Ok(SelfRestartOutcome::RestartReady { binary_path })
}

/// Verify the running binary matches what was recorded before restart.
/// Called after claiming a `restart_pending` task to confirm the new binary
/// is actually running (not the old one).
/// Returns Ok(true) if verified, Ok(false) if mismatch, Err on read failure.
pub async fn verify_post_restart_binary(state: &InnerState, task_id: &str) -> Result<bool> {
    // Read the self_restart_ready event from the DB
    let task_id_owned = task_id.to_owned();
    let sha256_pair: Option<(String, String)> = state
        .async_database
        .reader()
        .call(move |conn| {
            let result: Option<(String, String)> = conn.query_row(
                "SELECT payload_json FROM events WHERE task_id = ?1 AND event_type = 'self_restart_ready' ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![task_id_owned],
                |row| row.get::<_, String>(0),
            ).ok().and_then(|json_str| {
                serde_json::from_str::<serde_json::Value>(&json_str).ok()
                    .and_then(|v| {
                        // Support both old field name (binary_sha256) and new (new_binary_sha256)
                        let new_sha = v.get("new_binary_sha256")
                            .or_else(|| v.get("binary_sha256"))
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string())?;
                        let old_sha = v.get("old_binary_sha256")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        Some((new_sha, old_sha))
                    })
            });
            Ok(result)
        })
        .await
        .map_err(flatten_err)?;

    let (expected, old_binary_sha256) = match sha256_pair {
        Some((s, old)) if s != "unknown" => (s, old),
        _ => return Ok(true), // No recorded hash — skip verification
    };

    // Compute SHA256 of the currently running binary
    let current_exe = std::env::current_exe().context("cannot resolve current executable")?;
    let content = std::fs::read(&current_exe).context("cannot read current executable")?;
    let actual = sha256_hex(&content);

    if actual == expected {
        insert_event(
            state,
            task_id,
            None,
            "binary_verification",
            json!({"verified": true, "old_binary_sha256": old_binary_sha256, "expected_sha256": expected, "actual_sha256": actual, "build_git_hash": env!("BUILD_GIT_HASH"), "build_timestamp": env!("BUILD_TIMESTAMP")}),
        ).await?;
        Ok(true)
    } else {
        insert_event(
            state,
            task_id,
            None,
            "binary_verification",
            json!({"verified": false, "old_binary_sha256": old_binary_sha256, "expected_sha256": expected, "actual_sha256": actual, "current_exe": current_exe.to_string_lossy(), "build_git_hash": env!("BUILD_GIT_HASH"), "build_timestamp": env!("BUILD_TIMESTAMP")}),
        ).await?;
        Ok(false)
    }
}
