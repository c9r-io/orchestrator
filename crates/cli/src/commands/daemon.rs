//! Local daemon lifecycle commands (stop / status).
//!
//! These commands operate directly on the PID file and process signals
//! without requiring a gRPC connection to the daemon.

use std::path::Path;

use anyhow::{Context, Result};

use crate::DaemonCommands;

/// Dispatch a daemon subcommand.
pub async fn dispatch(cmd: DaemonCommands) -> Result<()> {
    let app_root = agent_orchestrator::config_load::detect_app_root();
    let pid_path = app_root.join("data/daemon.pid");

    match cmd {
        DaemonCommands::Stop => stop(&pid_path).await,
        DaemonCommands::Status => status(&pid_path),
    }
}

/// Read the PID from the PID file, if present and parseable.
fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check whether a process is alive using a zero-signal kill probe.
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

/// Send SIGTERM to the daemon and wait for it to exit.
async fn stop(pid_path: &Path) -> Result<()> {
    let pid = match read_pid_file(pid_path) {
        Some(pid) if is_process_alive(pid) => pid,
        Some(_) => {
            // PID file exists but process is dead — clean up stale file
            let _ = std::fs::remove_file(pid_path);
            println!("orchestratord is not running (stale PID file removed)");
            return Ok(());
        }
        None => {
            println!("orchestratord is not running (no PID file)");
            return Ok(());
        }
    };

    println!("stopping orchestratord (PID {pid})...");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .context("failed to send SIGTERM")?;

    // Wait for the process to exit (up to 30 seconds)
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if !is_process_alive(pid) {
            println!("orchestratord stopped");
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("orchestratord (PID {pid}) did not exit within 30 seconds");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Print the current daemon status.
fn status(pid_path: &Path) -> Result<()> {
    match read_pid_file(pid_path) {
        Some(pid) if is_process_alive(pid) => {
            println!("orchestratord is running (PID {pid})");
        }
        Some(_) => {
            println!("orchestratord is not running (stale PID file)");
        }
        None => {
            println!("orchestratord is not running");
        }
    }
    Ok(())
}
