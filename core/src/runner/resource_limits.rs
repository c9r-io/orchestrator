#[cfg(unix)]
use super::profile::ResolvedExecutionProfile;
#[cfg(unix)]
use super::profile::UnixResourceLimits;
#[cfg(unix)]
use anyhow::{anyhow, Result};
#[cfg(unix)]
use std::io;

#[cfg(unix)]
pub(crate) fn apply_unix_resource_limits_to_command(
    cmd: &mut tokio::process::Command,
    execution_profile: &ResolvedExecutionProfile,
) -> Result<()> {
    let Some(limits) = UnixResourceLimits::from_execution_profile(execution_profile) else {
        return Ok(());
    };
    // SAFETY: `pre_exec` runs in the forked child between fork() and exec().
    // The closure only calls `setrlimit`, which is async-signal-safe per POSIX.
    // No heap allocations, mutex acquisitions, or non-signal-safe operations
    // occur inside the closure.
    unsafe {
        cmd.pre_exec(move || apply_unix_resource_limits(&limits).map_err(io::Error::other));
    }
    Ok(())
}

#[cfg(unix)]
fn apply_unix_resource_limits(limits: &UnixResourceLimits) -> Result<()> {
    if let Some(value) = limits.max_memory_bytes {
        set_rlimit(rlimit_resource(libc::RLIMIT_AS as u64)?, value)?;
    }
    if let Some(value) = limits.max_cpu_seconds {
        set_rlimit(rlimit_resource(libc::RLIMIT_CPU as u64)?, value)?;
    }
    if let Some(value) = limits.max_processes {
        set_rlimit(rlimit_resource(libc::RLIMIT_NPROC as u64)?, value)?;
    }
    if let Some(value) = limits.max_open_files {
        set_rlimit(rlimit_resource(libc::RLIMIT_NOFILE as u64)?, value)?;
    }
    Ok(())
}

#[cfg(unix)]
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimitResource = libc::__rlimit_resource_t;

#[cfg(unix)]
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimitResource = libc::c_int;

#[cfg(unix)]
fn rlimit_resource(resource: u64) -> Result<RlimitResource> {
    // libc exposes RLIMIT_* with target-specific integer types. Convert via a
    // wide intermediate so Linux GNU x86/u32 and Darwin/i32 both type-check.
    RlimitResource::try_from(resource)
        .map_err(|_| anyhow!("unsupported rlimit resource selector: {resource}"))
}

#[cfg(unix)]
fn set_rlimit(resource: RlimitResource, value: u64) -> Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `setrlimit` is called in the child process before exec with a
    // valid resource selector and initialized `rlimit` struct.
    let rc = unsafe { libc::setrlimit(resource, &limit) };
    if rc == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "setrlimit({resource}) failed: {}",
            io::Error::last_os_error()
        ))
    }
}
