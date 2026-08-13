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
///
/// Both names delegate to `orchestrator_config::paths` rather than joining a
/// literal: the CLI and the client crate resolve the same two files and cannot
/// see this module, so a literal here is half of a disagreement (FR-163).
pub fn socket_path(data_dir: &Path) -> PathBuf {
    agent_orchestrator::paths::socket_path(data_dir)
}

/// Returns the path to the daemon PID file.
pub fn pid_path(data_dir: &Path) -> PathBuf {
    agent_orchestrator::paths::pid_path(data_dir)
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

/// Identity of the data directory: `(st_dev, st_ino)`.
///
/// `None` when the path cannot be stat'd at all, which includes the case that
/// matters most — it was removed.
///
/// The identity rather than the path is the subject, because the two failures
/// this exists to catch are not the same shape. A directory that was **deleted**
/// fails a path check. A directory that was deleted and **recreated** passes
/// one: the path is there, and the daemon is writing an orphaned inode while a
/// new daemon owns the name. Measured before this was written — delete plus
/// `mkdir` of the same path left the old daemon alive holding seven open file
/// descriptors on an unlinked database, while a second daemon started on the
/// same path and became ready. A path check sees one of those two and reports
/// the other as healthy.
#[cfg(unix)]
pub fn data_dir_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Whether the data directory the daemon started with is gone.
///
/// True when it cannot be stat'd, and true when something else now occupies the
/// path. Both mean the same thing to the daemon: the directory it opened is no
/// longer the directory at that path, so it cannot serve anyone from it.
#[cfg(unix)]
pub fn data_dir_vanished(expected: (u64, u64), current: Option<(u64, u64)>) -> bool {
    current != Some(expected)
}

/// Folds one observation into the confirmation counter, returning whether the
/// daemon should now shut down.
///
/// Separated from the watcher loop so the hysteresis can be asserted without
/// waiting on wall-clock time: a test that proves the reset works by sleeping is
/// a test that will one day be flaky and be deleted. The counter resets on any
/// match, which is the whole of the hysteresis — without the reset, a daemon that
/// saw one failed `stat` per hour would eventually accumulate three of them and
/// exit for no reason.
#[cfg(unix)]
pub fn observe_data_dir(
    expected: (u64, u64),
    current: Option<(u64, u64)>,
    confirmations: &mut u32,
    required: u32,
) -> bool {
    if data_dir_vanished(expected, current) {
        *confirmations += 1;
        *confirmations >= required
    } else {
        *confirmations = 0;
        false
    }
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

    #[test]
    fn data_dir_identity_is_stable_while_the_directory_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let first = data_dir_identity(dir.path()).expect("stat a live directory");
        // Writing inside it must not change the directory's own identity, or the
        // watcher would fire on ordinary use.
        std::fs::write(dir.path().join("some.db"), b"x").unwrap();
        let second = data_dir_identity(dir.path()).expect("stat it again");
        assert_eq!(first, second);
        assert!(!data_dir_vanished(first, Some(second)));
    }

    #[test]
    fn data_dir_identity_is_none_once_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let before = data_dir_identity(&path).expect("stat before");
        std::fs::remove_dir_all(&path).unwrap();
        assert_eq!(data_dir_identity(&path), None);
        assert!(data_dir_vanished(before, None));
    }

    /// The case a path-existence check cannot see.
    ///
    /// After delete-and-recreate the path is present, so `[ -d ]` and
    /// `Path::exists()` both report healthy, while the daemon holds an orphaned
    /// inode and a new daemon can take the name. Identity is what separates them.
    #[test]
    fn data_dir_identity_changes_when_the_path_is_recreated() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("d");
        std::fs::create_dir(&path).unwrap();
        let before = data_dir_identity(&path).expect("stat before");

        std::fs::remove_dir_all(&path).unwrap();
        std::fs::create_dir(&path).unwrap();

        assert!(path.exists(), "the fixture must leave the path present");
        let after = data_dir_identity(&path).expect("stat after");
        assert_ne!(
            before, after,
            "delete-and-recreate produced the same identity, so the watcher would \
             not notice the daemon is writing an orphaned inode"
        );
        assert!(data_dir_vanished(before, Some(after)));
    }

    /// A healthy directory never accumulates confirmations, so the watcher
    /// cannot become a random killer.
    #[test]
    fn an_untouched_data_dir_never_reaches_the_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let expected = data_dir_identity(dir.path()).expect("stat");
        let mut confirmations = 0u32;
        for _ in 0..100 {
            let now = data_dir_identity(dir.path());
            assert!(!observe_data_dir(expected, now, &mut confirmations, 3));
        }
        assert_eq!(confirmations, 0);
    }

    #[test]
    fn three_consecutive_vanishes_trip_it() {
        let expected = (1, 2);
        let mut c = 0u32;
        assert!(!observe_data_dir(expected, None, &mut c, 3));
        assert!(!observe_data_dir(expected, None, &mut c, 3));
        assert!(
            observe_data_dir(expected, None, &mut c, 3),
            "third should trip"
        );
    }

    /// The reset is load-bearing, not decorative.
    ///
    /// Two failures, a recovery, two more failures: five observations of which
    /// four saw a vanished directory, and it must NOT trip — because they were
    /// never consecutive. Without the reset this reaches three and ends a healthy
    /// daemon. That is the mutation this fixture is aimed at: deleting the
    /// `*confirmations = 0` line, which no other test in this file would catch.
    #[test]
    fn a_recovery_between_failures_prevents_the_trip() {
        let expected = (1, 2);
        let mut c = 0u32;
        assert!(!observe_data_dir(expected, None, &mut c, 3));
        assert!(!observe_data_dir(expected, None, &mut c, 3));
        assert_eq!(c, 2);

        // One good observation.
        assert!(!observe_data_dir(expected, Some(expected), &mut c, 3));
        assert_eq!(c, 0, "a successful stat did not reset the counter");

        assert!(!observe_data_dir(expected, None, &mut c, 3));
        assert!(
            !observe_data_dir(expected, None, &mut c, 3),
            "four vanished observations across a recovery tripped a threshold of three"
        );
    }

    /// A different directory at the same path trips it exactly like removal.
    #[test]
    fn a_replaced_directory_trips_it_like_a_removed_one() {
        let expected = (1, 2);
        let mut c = 0u32;
        for _ in 0..2 {
            assert!(!observe_data_dir(expected, Some((1, 99)), &mut c, 3));
        }
        assert!(observe_data_dir(expected, Some((1, 99)), &mut c, 3));
    }
}
