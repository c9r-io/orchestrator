use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_orchestrator::state::InnerState;

/// Returns the path to the daemon Unix Domain Socket.
pub fn socket_path(app_root: &Path) -> PathBuf {
    app_root.join("data/orchestrator.sock")
}

/// Returns the path to the daemon PID file.
pub fn pid_path(app_root: &Path) -> PathBuf {
    app_root.join("data/daemon.pid")
}

/// Write the current process PID to the PID file.
pub fn write_pid_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create PID file directory: {}", parent.display())
        })?;
    }
    std::fs::write(path, std::process::id().to_string())
        .with_context(|| format!("failed to write PID file: {}", path.display()))
}

/// Read the PID from the PID file, if present.
pub fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check if a process with the given PID is alive.
#[cfg(unix)]
pub fn is_process_alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

/// Detect whether a PID file refers to a dead process (stale from a previous crash).
/// Returns `true` if a PID file exists and the process is no longer alive.
#[cfg(unix)]
pub fn detect_stale_pid(pid_path: &Path) -> bool {
    match read_pid_file(pid_path) {
        Some(pid) => !is_process_alive(pid),
        None => false,
    }
}

/// Clean up socket and PID file on shutdown.
pub fn cleanup(socket_path: &Path, pid_path: &Path) {
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

/// Remove only the PID file, leaving the socket intact.
/// Used before exec() so the new process writes its own PID while the stale
/// socket is cleaned up by the new binary's normal startup path.
pub fn cleanup_pid_only(pid_path: &Path) {
    let _ = std::fs::remove_file(pid_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_stale_pid_returns_true_for_dead_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        // PID 2_000_000_000 is almost certainly not alive
        std::fs::write(&pid_path, "2000000000").unwrap();
        assert!(detect_stale_pid(&pid_path));
    }

    #[test]
    fn detect_stale_pid_returns_false_for_current_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        std::fs::write(&pid_path, std::process::id().to_string()).unwrap();
        assert!(!detect_stale_pid(&pid_path));
    }

    #[test]
    fn detect_stale_pid_returns_false_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        assert!(!detect_stale_pid(&pid_path));
    }
}

/// Wait for SIGTERM or SIGINT, then initiate graceful shutdown.
pub async fn shutdown_signal(state: Arc<InnerState>) -> Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to install SIGTERM handler")?;

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received SIGINT, shutting down");
            state.daemon_runtime.request_shutdown();
        }
        _ = sigterm.recv() => {
            tracing::info!("received SIGTERM, shutting down");
            state.daemon_runtime.request_shutdown();
        }
    }

    // Worker draining and cleanup handled by main.rs after gRPC server stops.
    Ok(())
}
