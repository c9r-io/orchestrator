use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use agent_orchestrator::state::InnerState;

/// Stores the PID of the process that sent SIGTERM (captured via `SA_SIGINFO`).
/// A value of 0 means no sender PID has been recorded yet.
static SIGTERM_SENDER_PID: AtomicI32 = AtomicI32::new(0);

/// The previous SIGTERM handler (tokio's) that we chain to after capturing the
/// sender PID.  Stored as a raw `sigaction` struct so we can invoke it from
/// our signal handler.
///
/// Accessed via raw pointer (`addr_of!`/`addr_of_mut!`) to avoid creating
/// references to mutable statics (UB since Rust 2024 edition).
static mut PREV_SIGTERM_ACTION: std::mem::MaybeUninit<libc::sigaction> =
    std::mem::MaybeUninit::uninit();

/// Signal handler for SIGTERM installed via `sigaction` with `SA_SIGINFO`.
///
/// Stores `siginfo_t.si_pid` into the global atomic, then chains to the
/// previous handler (tokio's) so the self-pipe wakeup still fires.
///
/// # Safety
///
/// This is a signal handler — it must only call async-signal-safe functions.
/// `AtomicI32::store` is safe in a signal context.
extern "C" fn sigterm_sigaction_handler(
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    ucontext: *mut libc::c_void,
) {
    if !info.is_null() {
        // SAFETY: `info` is a valid pointer provided by the kernel to a
        // SA_SIGINFO handler.
        let sender_pid = unsafe {
            // On Linux, libc exposes si_pid as a method; on macOS it is a field.
            #[cfg(target_os = "linux")]
            {
                (*info).si_pid()
            }
            #[cfg(not(target_os = "linux"))]
            {
                (*info).si_pid
            }
        };
        SIGTERM_SENDER_PID.store(sender_pid, Ordering::SeqCst);
    }

    // SAFETY: `PREV_SIGTERM_ACTION` was initialised by
    // `install_sigterm_siginfo_handler` before this handler can fire.
    // We read it via raw pointer to avoid creating a reference to a
    // mutable static.
    unsafe {
        let prev = &*std::ptr::addr_of!(PREV_SIGTERM_ACTION).cast::<libc::sigaction>();
        let handler = prev.sa_sigaction;
        if handler == libc::SIG_DFL || handler == libc::SIG_IGN {
            return;
        }
        if prev.sa_flags & libc::SA_SIGINFO != 0 {
            // Previous handler also uses SA_SIGINFO — call with 3 args.
            let func: extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void) =
                std::mem::transmute(handler);
            func(sig, info, ucontext);
        } else {
            // Previous handler is a simple sa_handler.
            let func: extern "C" fn(libc::c_int) = std::mem::transmute(handler);
            func(sig);
        }
    }
}

/// Install a `sigaction`-based SIGTERM handler that captures the sender PID
/// and chains to the previous (tokio) handler.
///
/// Must be called **after** tokio has registered its SIGTERM listener (via
/// `tokio::signal::unix::signal(SignalKind::terminate())`) so that we layer
/// on top and can forward to tokio's handler.
fn install_sigterm_siginfo_handler() -> Result<()> {
    // SAFETY: We initialise a `sigaction` struct with `SA_SIGINFO` and a
    // valid extern "C" handler.  `libc::sigaction` is a POSIX call.
    // We store the old handler in `PREV_SIGTERM_ACTION` for chaining.
    // This is only called once, before any SIGTERM can arrive, so the
    // write to `PREV_SIGTERM_ACTION` is not racy.
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigterm_sigaction_handler as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);

        let mut old_sa: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGTERM, &sa, &mut old_sa) != 0 {
            return Err(anyhow::anyhow!(
                "sigaction(SIGTERM) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        std::ptr::addr_of_mut!(PREV_SIGTERM_ACTION)
            .cast::<libc::sigaction>()
            .write(old_sa);
    }
    Ok(())
}

/// Return the PID that sent SIGTERM, or `None` if not yet captured.
pub fn sigterm_sender_pid() -> Option<i32> {
    let pid = SIGTERM_SENDER_PID.load(Ordering::SeqCst);
    if pid != 0 { Some(pid) } else { None }
}

/// Returns the path to the daemon Unix Domain Socket.
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join("orchestrator.sock")
}

/// Returns the path to the daemon PID file.
pub fn pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join("daemon.pid")
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

/// Check whether another daemon instance is already running.
/// Returns `Some(pid)` if a PID file exists, the process is alive, and it is
/// NOT the current process (i.e. not a post-exec() self-check).
#[cfg(unix)]
pub fn detect_running_daemon(pid_path: &Path) -> Option<u32> {
    match read_pid_file(pid_path) {
        Some(pid) if pid != std::process::id() && is_process_alive(pid) => Some(pid),
        _ => None,
    }
}

/// Clean up socket and PID file on shutdown.
pub fn cleanup(socket_path: &Path, pid_path: &Path) {
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

/// Wait for SIGTERM or SIGINT, then initiate graceful shutdown.
///
/// SIGHUP is continuously ignored so the daemon survives terminal closure.
pub async fn shutdown_signal(state: Arc<InnerState>) -> Result<()> {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to install SIGTERM handler")?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .context("failed to install SIGHUP handler")?;

    // Layer our SA_SIGINFO handler on top of tokio's SIGTERM handler so we
    // can capture the sender PID before forwarding to tokio's self-pipe.
    if let Err(e) = install_sigterm_siginfo_handler() {
        tracing::warn!(error = %e, "failed to install SA_SIGINFO SIGTERM handler; sender PID will not be logged");
    }

    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "ctrl_c handler failed");
                }
                tracing::info!("received SIGINT, shutting down");
                state.daemon_runtime.request_shutdown();
                break;
            }
            _ = sigterm.recv() => {
                if let Some(sender) = sigterm_sender_pid() {
                    tracing::info!(sender_pid = sender, "received SIGTERM, shutting down");
                } else {
                    tracing::info!("received SIGTERM, shutting down (sender PID unknown)");
                }
                state.daemon_runtime.request_shutdown();
                break;
            }
            _ = sighup.recv() => {
                tracing::info!("received SIGHUP, ignoring (daemon mode)");
            }
        }
    }

    // Worker draining and cleanup handled by main.rs after gRPC server stops.
    Ok(())
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

    #[test]
    fn detect_running_daemon_returns_none_for_own_pid() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        std::fs::write(&pid_path, std::process::id().to_string()).unwrap();
        // After exec(), the PID is preserved — should not block startup.
        assert!(detect_running_daemon(&pid_path).is_none());
    }

    #[test]
    fn detect_running_daemon_returns_none_for_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        std::fs::write(&pid_path, "2000000000").unwrap();
        assert!(detect_running_daemon(&pid_path).is_none());
    }

    #[test]
    fn detect_running_daemon_returns_none_when_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");
        assert!(detect_running_daemon(&pid_path).is_none());
    }
}
