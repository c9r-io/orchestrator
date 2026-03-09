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
#[allow(dead_code)]
pub fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check if a process with the given PID is alive.
#[cfg(unix)]
#[allow(dead_code)]
pub fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Clean up socket and PID file on shutdown.
pub fn cleanup(socket_path: &Path, pid_path: &Path) {
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

/// Wait for SIGTERM or SIGINT, then initiate graceful shutdown.
pub async fn shutdown_signal(_state: Arc<InnerState>) {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received SIGINT, shutting down");
        }
        _ = sigterm.recv() => {
            tracing::info!("received SIGTERM, shutting down");
        }
    }

    // Worker draining and cleanup handled by main.rs after gRPC server stops.
}
