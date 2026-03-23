//! Daemon lifecycle commands (stop / status / maintenance).
//!
//! Stop and status operate directly on the PID file and process signals
//! without requiring a gRPC connection. Maintenance mode requires a gRPC
//! connection to the running daemon.

use std::path::Path;

use anyhow::{Context, Result};

use crate::DaemonCommands;

/// Dispatch a daemon subcommand.
pub async fn dispatch(cmd: DaemonCommands) -> Result<()> {
    let data_dir = agent_orchestrator::config_load::data_dir();
    let pid_path = data_dir.join("daemon.pid");

    match cmd {
        DaemonCommands::Stop => stop(&pid_path).await,
        DaemonCommands::Status => status(&pid_path),
        DaemonCommands::Maintenance { enable, disable } => {
            let flag = if enable {
                true
            } else if disable {
                false
            } else {
                anyhow::bail!("specify --enable or --disable");
            };
            maintenance(flag).await
        }
    }
}

/// Toggle maintenance mode via gRPC.
async fn maintenance(enable: bool) -> Result<()> {
    let mut client = crate::client::connect(None).await?;
    let resp = client
        .maintenance_mode(orchestrator_proto::MaintenanceModeRequest { enable })
        .await
        .context("failed to set maintenance mode")?
        .into_inner();
    println!("{}", resp.message);
    Ok(())
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

    // Guard: refuse to stop the daemon if we are a descendant of it.
    // This prevents agent processes spawned by the daemon from killing
    // their own ancestor (the SIGTERM root cause in full-qa execution).
    if is_descendant_of(pid) {
        anyhow::bail!(
            "refusing to stop orchestratord (PID {pid}): the calling process (PID {}) \
             is a descendant of the daemon. An agent subprocess cannot stop its own parent.",
            std::process::id()
        );
    }

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

/// Check whether the current process is a descendant of `ancestor_pid`.
///
/// Walks the PPID chain from our PID upward until we reach PID 0/1 or find
/// the ancestor.  Bounded to 64 iterations to avoid infinite loops.
#[cfg(unix)]
fn is_descendant_of(ancestor_pid: u32) -> bool {
    let mut current = std::process::id();
    for _ in 0..64 {
        if current == ancestor_pid {
            return true;
        }
        if current <= 1 {
            return false;
        }
        match get_ppid(current) {
            Some(ppid) if ppid != current => current = ppid,
            _ => return false,
        }
    }
    false
}

/// Get the parent PID of a process via `sysctl` (macOS) or `/proc` (Linux).
#[cfg(target_os = "macos")]
fn get_ppid(pid: u32) -> Option<u32> {
    let output = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let ppid_str = String::from_utf8_lossy(&output.stdout);
    ppid_str.trim().parse().ok()
}

/// Get the parent PID of a process via `/proc` (Linux).
#[cfg(target_os = "linux")]
fn get_ppid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(1)?.parse().ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_of_init() {
        // PID 1 (launchd/init) is an ancestor of every user process.
        assert!(is_descendant_of(1));
    }

    #[test]
    fn not_descendant_of_nonexistent() {
        assert!(!is_descendant_of(2_000_000_000));
    }

    #[test]
    fn descendant_of_self() {
        assert!(is_descendant_of(std::process::id()));
    }
}
