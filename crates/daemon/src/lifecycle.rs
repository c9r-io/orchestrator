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

/// Whether the PID file at `path` still names *this* process.
///
/// The pidfile's owner is written in the pidfile, so this asks it directly. The
/// obvious alternative — remembering the inode at write time — is a proxy, and
/// it is wrong on the one mutation that matters: `std::fs::write` truncates in
/// place, so a second daemon writing the same path keeps the same inode and an
/// identity check would call its pidfile ours (FR-170).
#[cfg(unix)]
pub fn pid_file_is_ours(pid_path: &Path) -> bool {
    read_pid_file(pid_path) == Some(std::process::id())
}

/// Clean up the socket and PID file on shutdown — but only the ones still ours.
///
/// `socket_identity` is the `(st_dev, st_ino)` recorded when *this* daemon bound
/// the socket, and `None` when it never bound one at all (the `--bind` TCP path),
/// in which case there is nothing here to remove.
///
/// Both removals are conditional because a path is not an identity, and this is
/// the exit half of the same error `data_dir_identity` documents for the entry
/// half. Measured before this guard existed (FR-170): after the data directory
/// was deleted and recreated, a second daemon took the path and became ready,
/// and ~15s later the first daemon's unconditional teardown unlinked *its
/// successor's* socket and pidfile. The survivor stayed alive holding seven
/// database fds and a listening socket on an unlinked inode, `daemon status`
/// reported "not running", `daemon stop` could not find it, its own data
/// directory was intact so the vanish watcher never fired — and with the pidfile
/// gone, a third daemon started cleanly on the same path. Bounding the window
/// (DD-185) does not help: the damage is done by the exit, not by the overlap.
///
/// Both artifacts are now checked by the same evidence — readable content naming
/// the process that wrote it — and FR-170's socket half is the reason.
///
/// That half compared `(st_dev, st_ino)`, on the reasoning that a socket has no
/// readable content so its inode is its identity. The first premise is true and
/// the second does not follow. `bind` creates the filesystem entry, and unlinking
/// it frees the inode immediately — the listening socket does not pin it — so the
/// number is available for reuse at once, and Linux reuses it. Measured in an
/// alpine container over 50 trials: delete-and-recreate at the same path returned
/// the **same** `(st_dev, st_ino)` 50 times out of 50 for a regular file and 49
/// out of 50 for a directory. On APFS, where FR-170 was certified, it does not.
///
/// So the guard was inverted on the platform this daemon ships to: a dying daemon
/// would read a successor's socket as its own and unlink it — the exact damage the
/// guard was written to prevent. `claim_socket` gives the socket the readable
/// content it lacks, in a token file beside it, and this compares that.
#[cfg(unix)]
pub fn cleanup(socket_path: &Path, ownership: Option<&SocketOwnership>, pid_path: &Path) {
    if ownership.is_some_and(|owner| owner.still_owns(socket_path)) {
        let _ = std::fs::remove_file(socket_path);
        let _ = std::fs::remove_file(socket_owner_path(socket_path));
    }
    if pid_file_is_ours(pid_path) {
        let _ = std::fs::remove_file(pid_path);
    }
}

/// Where the ownership token for a socket lives: the socket's own name with
/// `.owner` appended, so it sits beside the socket and travels with it.
#[cfg(unix)]
pub fn socket_owner_path(socket_path: &Path) -> PathBuf {
    let mut name = socket_path.file_name().unwrap_or_default().to_os_string();
    name.push(".owner");
    socket_path.with_file_name(name)
}

/// Proof that this process is the one that bound the socket at a path.
///
/// Held for the daemon's lifetime and consulted once, at teardown. The token is a
/// v4 UUID rather than the PID: a PID is reused too, and the whole point is to
/// survive the case where a successor occupies the same names.
#[cfg(unix)]
pub struct SocketOwnership {
    token: String,
    path: PathBuf,
}

#[cfg(unix)]
impl SocketOwnership {
    /// Whether the token beside `socket_path` is still the one this process wrote.
    ///
    /// False when it cannot be read at all, which is the safe answer: an
    /// unreadable token is not proof of ownership, and the cost of declining to
    /// remove a socket is a stale file the next startup already handles, while the
    /// cost of removing a successor's is an unreachable live daemon.
    pub fn still_owns(&self, socket_path: &Path) -> bool {
        if socket_owner_path(socket_path) != self.path {
            return false;
        }
        std::fs::read_to_string(&self.path)
            .map(|found| found.trim() == self.token)
            .unwrap_or(false)
    }
}

/// Record that this process owns the socket it has just bound.
///
/// Called after `bind`, so the token is only ever written for a socket this
/// process is actually listening on. Owner-only permissions, like the socket.
#[cfg(unix)]
pub fn claim_socket(socket_path: &Path) -> std::io::Result<SocketOwnership> {
    use std::os::unix::fs::PermissionsExt;

    let token = uuid::Uuid::new_v4().to_string();
    let path = socket_owner_path(socket_path);
    std::fs::write(&path, format!("{token}\n"))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(SocketOwnership { token, path })
}

/// Identity of any path: `(st_dev, st_ino)`, or `None` when it cannot be stat'd.
///
/// Follows symlinks, so a path swapped for a link to somewhere else reads as the
/// target's identity and therefore as a change — which is the answer this wants.
#[cfg(unix)]
pub fn path_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// The data directory this daemon started with, held open for its lifetime.
///
/// FR-169 asked the right question — a directory that was **deleted** fails a
/// path check, while one that was deleted and **recreated** passes it, and the
/// second is the worse half: the path reads healthy while this daemon writes an
/// orphaned inode and a successor owns the name. It answered by comparing the
/// path's `(st_dev, st_ino)` against the pair recorded at startup, and stat'd the
/// path each time while holding nothing. That is where it failed: a freed inode
/// number is available immediately, and Linux hands it back. Measured over 50
/// trials in an alpine container, delete-and-recreate at the same path returned
/// the **same** identity 49 times out of 50 — so the watcher compared equal and
/// never fired, on the platform this daemon ships to. It was certified on APFS,
/// which does not do that.
///
/// **The descriptor is what makes the comparison sound, and it is not what is
/// compared.** An open handle pins the inode, so its number cannot be recycled
/// while this daemon lives, and the recreated directory is therefore forced to
/// differ. Re-measured with the handle held, on both platforms: identical
/// identity in **0 of 30** trials on Linux and **0 of 30** on macOS. The fix is
/// not a cleverer comparison; it is holding open the thing being compared.
///
/// `held_links` is a second, cheaper signal and deliberately not the one relied
/// on: Linux zeroes the link count of a removed directory as seen through a held
/// descriptor (30 of 30), and macOS keeps reporting 2 (0 of 30). It fires earlier
/// where it works and costs one `fstat` where it does not, so it is one arm of a
/// disjunction rather than the answer.
///
/// Reading the path also catches what a descriptor alone cannot: a directory
/// **renamed** away and replaced is still alive, so no link count moves, and only
/// the path says the name now leads somewhere else.
#[cfg(unix)]
pub struct DataDirHandle {
    dir: std::fs::File,
    identity: (u64, u64),
}

/// One poll of the data directory: what the held descriptor says, and what the
/// path says. Two facts, because they fail in different ways.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataDirObservation {
    /// Link count of the directory this daemon opened. Zero once it is removed.
    pub held_links: u64,
    /// Identity of whatever is at the path now, or `None` when it cannot be stat'd.
    pub at_path: Option<(u64, u64)>,
}

#[cfg(unix)]
impl DataDirHandle {
    /// Open the data directory and record what it was.
    pub fn open(path: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let dir = std::fs::File::open(path)?;
        let metadata = dir.metadata()?;
        Ok(Self {
            identity: (metadata.dev(), metadata.ino()),
            dir,
        })
    }

    /// The identity recorded at open, which every later observation is compared to.
    pub fn identity(&self) -> (u64, u64) {
        self.identity
    }

    /// Take one observation. A descriptor that cannot be stat'd reads as zero
    /// links — failing closed, since the alternative is reporting health from an
    /// error.
    pub fn observe(&self, path: &Path) -> DataDirObservation {
        use std::os::unix::fs::MetadataExt;
        DataDirObservation {
            held_links: self.dir.metadata().map(|m| m.nlink()).unwrap_or(0),
            at_path: path_identity(path),
        }
    }
}

/// Whether the data directory the daemon started with is gone.
///
/// True when the directory it opened has been removed, true when the path cannot
/// be stat'd, and true when something else now occupies the path. All three mean
/// the same thing to the daemon: it cannot serve anyone from that directory.
#[cfg(unix)]
pub fn data_dir_vanished(expected: (u64, u64), observed: DataDirObservation) -> bool {
    observed.held_links == 0 || observed.at_path != Some(expected)
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
    observed: DataDirObservation,
    confirmations: &mut u32,
    required: u32,
) -> bool {
    if data_dir_vanished(expected, observed) {
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

    // --- FR-170: teardown removes only what this process still owns ---------
    //
    // The positive cases below are the ones that run every day; the negative
    // cases are the defect. **Every one of them overwrites in place**, and that
    // uniformity is the correction: FR-170's socket fixture replaced the file to
    // give the successor a new inode, and asserted `assert_ne!` on the identities
    // to prove the fixture had bitten. That assertion is a claim about the
    // filesystem's inode allocator, not about this code, and it is false on Linux
    // — it failed on the first CI run these tests ever had. `fs::write` truncates
    // rather than relinking, so overwriting in place holds the inode fixed and
    // asks the only question that matters: does the check read the content?

    /// Bind a real socket so the identity recorded is a socket's, not a
    /// regular file's, exactly as `main.rs` records it after `bind`.
    #[cfg(unix)]
    fn bind_socket(path: &Path) -> std::os::unix::net::UnixListener {
        std::os::unix::net::UnixListener::bind(path).expect("bind test socket")
    }

    #[test]
    fn cleanup_removes_both_artifacts_when_they_are_still_ours() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("orchestrator.sock");
        let pid_path = dir.path().join("daemon.pid");

        let _listener = bind_socket(&socket_path);
        let ownership = claim_socket(&socket_path).unwrap();
        write_pid_file(&pid_path).unwrap();

        cleanup(&socket_path, Some(&ownership), &pid_path);

        assert!(!socket_path.exists(), "our own socket must be cleaned up");
        assert!(
            !socket_owner_path(&socket_path).exists(),
            "and its ownership token must go with it, or the next daemon inherits ours"
        );
        assert!(!pid_path.exists(), "our own pidfile must be cleaned up");
    }

    /// The defect, in the form that does not depend on the filesystem.
    ///
    /// A successor rebinds the path and claims it. The claim overwrites the token
    /// **in place**, so the token file's inode is unchanged and the socket's may
    /// be too — on Linux it is, 50 times out of 50. An identity-based guard reads
    /// the successor's socket as its own here and unlinks it; this one reads the
    /// token and declines.
    #[test]
    fn cleanup_leaves_a_socket_that_is_no_longer_the_one_we_bound() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("orchestrator.sock");

        let listener = bind_socket(&socket_path);
        let ours = claim_socket(&socket_path).unwrap();
        let token_identity = path_identity(&socket_owner_path(&socket_path));

        // A successor takes the path and claims it.
        drop(listener);
        std::fs::remove_file(&socket_path).unwrap();
        let _successor = bind_socket(&socket_path);
        let theirs = claim_socket(&socket_path).unwrap();
        assert_eq!(
            path_identity(&socket_owner_path(&socket_path)),
            token_identity,
            "the fixture is only meaningful while the token's inode is unchanged"
        );
        assert!(
            !ours.still_owns(&socket_path),
            "the socket stopped being ours the moment the successor claimed it"
        );
        assert!(theirs.still_owns(&socket_path));

        cleanup(&socket_path, Some(&ours), &dir.path().join("absent.pid"));

        assert!(
            socket_path.exists(),
            "a successor's socket must survive our teardown"
        );
        assert!(
            socket_owner_path(&socket_path).exists(),
            "and so must its claim, or the successor loses its own proof"
        );
    }

    /// A token that cannot be read is not proof of ownership.
    #[test]
    fn cleanup_leaves_a_socket_whose_token_has_been_removed() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("orchestrator.sock");

        let _listener = bind_socket(&socket_path);
        let ours = claim_socket(&socket_path).unwrap();
        std::fs::remove_file(socket_owner_path(&socket_path)).unwrap();

        cleanup(&socket_path, Some(&ours), &dir.path().join("absent.pid"));

        assert!(
            socket_path.exists(),
            "an unreadable token must fail closed: a stale socket costs a startup \
             retry, an unlinked live one costs an unreachable daemon"
        );
    }

    #[test]
    fn cleanup_leaves_a_pid_file_naming_another_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");

        write_pid_file(&pid_path).unwrap();
        let ours = path_identity(&pid_path);

        // Overwrite in place, deliberately: this is the mutation that keeps the
        // inode and so defeats an identity check.
        std::fs::write(&pid_path, "2000000000").unwrap();
        assert_eq!(
            path_identity(&pid_path),
            ours,
            "the fixture is only meaningful while the inode is unchanged"
        );

        cleanup(&dir.path().join("absent.sock"), None, &pid_path);

        assert!(
            pid_path.exists(),
            "a successor's pidfile must survive our teardown"
        );
        assert_eq!(read_pid_file(&pid_path), Some(2_000_000_000));
    }

    #[test]
    fn cleanup_removes_no_socket_when_this_daemon_bound_none() {
        // The `--bind` TCP path: a socket file at that path was left by some
        // earlier UDS daemon and was never ours to delete.
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("orchestrator.sock");
        let _stranger = bind_socket(&socket_path);

        cleanup(&socket_path, None, &dir.path().join("absent.pid"));

        assert!(socket_path.exists(), "a socket we never bound must survive");
    }

    #[test]
    fn cleanup_is_a_no_op_when_neither_artifact_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let absent_socket = dir.path().join("absent.sock");
        let ownership = claim_socket(&absent_socket).unwrap();
        cleanup(
            &absent_socket,
            Some(&ownership),
            &dir.path().join("absent.pid"),
        );
    }

    /// A claim is for one path, and says nothing about any other.
    #[test]
    fn an_ownership_claim_does_not_answer_for_a_different_socket() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("ours.sock");
        let theirs = dir.path().join("theirs.sock");

        let _listener = bind_socket(&theirs);
        let ownership = claim_socket(&ours).unwrap();

        assert!(!ownership.still_owns(&theirs));
        cleanup(&theirs, Some(&ownership), &dir.path().join("absent.pid"));
        assert!(
            theirs.exists(),
            "a claim on one path must not unlink another"
        );
    }

    #[test]
    fn pid_file_is_ours_distinguishes_owner_from_stranger_and_absence() {
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("daemon.pid");

        assert!(!pid_file_is_ours(&pid_path), "absent is not ours");
        write_pid_file(&pid_path).unwrap();
        assert!(pid_file_is_ours(&pid_path));
        std::fs::write(&pid_path, "2000000000").unwrap();
        assert!(!pid_file_is_ours(&pid_path), "a stranger's is not ours");
    }

    /// A directory the daemon is using, seen through its own handle.
    fn healthy(expected: (u64, u64)) -> DataDirObservation {
        DataDirObservation {
            held_links: 2,
            at_path: Some(expected),
        }
    }

    #[test]
    fn a_live_data_dir_reads_healthy_while_it_is_written_to() {
        let dir = tempfile::tempdir().unwrap();
        let handle = DataDirHandle::open(dir.path()).expect("open a live directory");
        let expected = handle.identity();

        // Writing inside it must change neither fact, or the watcher would fire on
        // ordinary use.
        std::fs::write(dir.path().join("some.db"), b"x").unwrap();

        let observed = handle.observe(dir.path());
        assert!(observed.held_links > 0);
        assert_eq!(observed.at_path, Some(expected));
        assert!(!data_dir_vanished(expected, observed));
    }

    #[test]
    fn a_removed_data_dir_reads_gone_by_both_facts() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("d");
        std::fs::create_dir(&path).unwrap();
        let handle = DataDirHandle::open(&path).expect("open before");
        let expected = handle.identity();

        std::fs::remove_dir_all(&path).unwrap();

        let observed = handle.observe(&path);
        assert_eq!(observed.at_path, None);
        assert!(data_dir_vanished(expected, observed));
    }

    /// The case a path-existence check cannot see, and the case FR-169's identity
    /// check could not see either.
    ///
    /// After delete-and-recreate the path is present, so `[ -d ]` and
    /// `Path::exists()` both report healthy while the daemon writes an orphaned
    /// inode and a new daemon can take the name. FR-169 compared the path's
    /// `(st_dev, st_ino)` against startup's while holding nothing open, and this
    /// fixture asserted the two differ — a claim about the inode allocator, and
    /// false on Linux, where the freed number comes straight back 49 trials in 50.
    ///
    /// The claim is true again, and now it is earned rather than assumed: the
    /// handle is open across the mutation, so the removed inode cannot be
    /// recycled into the replacement. Measured with the handle held, the identity
    /// differed in 30 of 30 trials on Linux and 30 of 30 on macOS.
    ///
    /// `held_links` is deliberately *not* asserted — it is 0 on Linux and 2 on
    /// macOS, and a fixture that pins it is testing the filesystem again, in the
    /// other direction.
    #[test]
    fn a_recreated_data_dir_cannot_reuse_the_identity_this_daemon_holds() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("d");
        std::fs::create_dir(&path).unwrap();
        let handle = DataDirHandle::open(&path).expect("open before");
        let expected = handle.identity();

        std::fs::remove_dir_all(&path).unwrap();
        std::fs::create_dir(&path).unwrap();

        assert!(path.exists(), "the fixture must leave the path present");
        let observed = handle.observe(&path);
        assert_ne!(
            observed.at_path,
            Some(expected),
            "the replacement took the identity of the directory this daemon still \
             holds open, so the watcher would not notice the orphaned inode"
        );
        assert!(data_dir_vanished(expected, observed));
    }

    /// And the case the handle cannot see, which is why the path is still read.
    ///
    /// A rename leaves the directory alive, so its link count never drops. Only
    /// the path says the name now leads somewhere else. The replacement is a fresh
    /// `mkdir` with nothing freed before it, so its identity differs by
    /// construction rather than by luck.
    #[test]
    fn a_renamed_data_dir_is_caught_by_the_path_although_the_handle_lives() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("d");
        std::fs::create_dir(&path).unwrap();
        let handle = DataDirHandle::open(&path).expect("open before");
        let expected = handle.identity();

        std::fs::rename(&path, parent.path().join("d.moved")).unwrap();
        std::fs::create_dir(&path).unwrap();

        let observed = handle.observe(&path);
        assert!(
            observed.held_links > 0,
            "a renamed directory is still a directory; the handle cannot object"
        );
        assert_ne!(observed.at_path, Some(expected));
        assert!(data_dir_vanished(expected, observed));
    }

    /// A healthy directory never accumulates confirmations, so the watcher
    /// cannot become a random killer.
    #[test]
    fn an_untouched_data_dir_never_reaches_the_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let handle = DataDirHandle::open(dir.path()).expect("open");
        let expected = handle.identity();
        let mut confirmations = 0u32;
        for _ in 0..100 {
            let now = handle.observe(dir.path());
            assert!(!observe_data_dir(expected, now, &mut confirmations, 3));
        }
        assert_eq!(confirmations, 0);
    }

    #[test]
    fn three_consecutive_vanishes_trip_it() {
        let expected = (1, 2);
        let gone = DataDirObservation {
            held_links: 0,
            at_path: None,
        };
        let mut c = 0u32;
        assert!(!observe_data_dir(expected, gone, &mut c, 3));
        assert!(!observe_data_dir(expected, gone, &mut c, 3));
        assert!(
            observe_data_dir(expected, gone, &mut c, 3),
            "third should trip"
        );
    }

    /// Either fact alone is enough, and neither is required — the two failures
    /// are independent and the predicate is their disjunction.
    #[test]
    fn each_fact_trips_it_on_its_own() {
        let expected = (1, 2);

        // Removed underneath us, but the name still resolves to the same numbers:
        // the Linux inode-reuse case, which is exactly what the old check missed.
        assert!(data_dir_vanished(
            expected,
            DataDirObservation {
                held_links: 0,
                at_path: Some(expected),
            }
        ));

        // Still alive, but the name leads elsewhere: the rename case.
        assert!(data_dir_vanished(
            expected,
            DataDirObservation {
                held_links: 2,
                at_path: Some((1, 99)),
            }
        ));

        assert!(!data_dir_vanished(expected, healthy(expected)));
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
        let gone = DataDirObservation {
            held_links: 0,
            at_path: None,
        };
        let mut c = 0u32;
        assert!(!observe_data_dir(expected, gone, &mut c, 3));
        assert!(!observe_data_dir(expected, gone, &mut c, 3));
        assert_eq!(c, 2);

        // One good observation.
        assert!(!observe_data_dir(expected, healthy(expected), &mut c, 3));
        assert_eq!(c, 0, "a healthy observation did not reset the counter");

        assert!(!observe_data_dir(expected, gone, &mut c, 3));
        assert!(
            !observe_data_dir(expected, gone, &mut c, 3),
            "four vanished observations across a recovery tripped a threshold of three"
        );
    }

    /// A different directory at the same path trips it exactly like removal.
    #[test]
    fn a_replaced_directory_trips_it_like_a_removed_one() {
        let expected = (1, 2);
        let replaced = DataDirObservation {
            held_links: 2,
            at_path: Some((1, 99)),
        };
        let mut c = 0u32;
        for _ in 0..2 {
            assert!(!observe_data_dir(expected, replaced, &mut c, 3));
        }
        assert!(observe_data_dir(expected, replaced, &mut c, 3));
    }
}
