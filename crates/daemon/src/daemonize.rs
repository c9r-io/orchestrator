//! Standard Unix double-fork daemonization.
//!
//! When `--foreground` is NOT specified, the daemon detaches from the
//! controlling terminal so it survives terminal closure and SIGHUP.

use std::path::Path;

use anyhow::{Context, Result};

/// Perform a standard Unix double-fork to daemonize the current process.
///
/// After this function returns, the caller is the final grandchild process
/// with no controlling terminal. stdin is redirected to `/dev/null` and
/// stdout/stderr are redirected to `log_path` in append mode.
///
/// The original parent process (and the intermediate child) exit immediately.
/// The original parent prints the final daemon PID before exiting (communicated
/// from the grandchild via a pipe).
pub fn daemonize(log_path: &Path) -> Result<()> {
    use nix::unistd::{fork, setsid, ForkResult};
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::os::unix::io::AsRawFd;

    // Ensure log directory exists
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create log directory: {}", parent.display()))?;
    }

    // Create a pipe so the grandchild can send its PID back to the original parent.
    let (pipe_read, pipe_write) = nix::unistd::pipe().context("failed to create pipe")?;
    let pipe_read_fd = pipe_read.as_raw_fd();

    // First fork — parent waits for grandchild PID, prints it, then exits
    // SAFETY: fork() is called before the tokio runtime is started and before
    // any threads are spawned, so this is safe in a single-threaded context.
    match unsafe { fork() }.context("first fork failed")? {
        ForkResult::Parent { .. } => {
            // Close write end in parent
            drop(pipe_write);
            // Read grandchild PID from pipe
            // SAFETY: pipe_read_fd is a valid fd owned by the pipe_read OwnedFd
            // which is still alive. We wrap it in a File for convenient reading,
            // then forget the OwnedFd so we don't double-close.
            let mut reader = unsafe { File::from_raw_fd(pipe_read_fd) };
            std::mem::forget(pipe_read);
            let mut buf = String::new();
            let _ = reader.read_to_string(&mut buf);
            let pid_str = buf.trim();
            if pid_str.is_empty() {
                eprintln!("orchestratord daemonized");
            } else {
                eprintln!("orchestratord daemonized (PID {pid_str})");
            }
            std::process::exit(0);
        }
        ForkResult::Child => {
            // Close read end in child
            drop(pipe_read);
        }
    }

    // Become session leader — detach from controlling terminal
    setsid().context("setsid failed")?;

    // Second fork — prevent re-acquisition of a controlling terminal
    // SAFETY: Same single-threaded context as above; the intermediate child
    // has no threads.
    match unsafe { fork() }.context("second fork failed")? {
        ForkResult::Parent { .. } => {
            // Close write end in intermediate child before exiting
            drop(pipe_write);
            std::process::exit(0);
        }
        ForkResult::Child => {}
    }

    // Grandchild: send our PID back to the original parent via the pipe
    {
        let pid = std::process::id();
        let pid_bytes = pid.to_string().into_bytes();
        let _ = nix::unistd::write(&pipe_write, &pid_bytes);
        drop(pipe_write);
    }

    // Redirect stdin → /dev/null
    let devnull = File::open("/dev/null").context("failed to open /dev/null")?;
    nix::unistd::dup2(devnull.as_raw_fd(), 0).context("dup2 stdin failed")?;

    // Redirect stdout/stderr → log file (append)
    let logfile = File::options()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open daemon log: {}", log_path.display()))?;
    nix::unistd::dup2(logfile.as_raw_fd(), 1).context("dup2 stdout failed")?;
    nix::unistd::dup2(logfile.as_raw_fd(), 2).context("dup2 stderr failed")?;

    Ok(())
}
