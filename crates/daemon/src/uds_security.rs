//! UDS transport security: peer credential validation, audit, and optional
//! authorization policy.
//!
//! This module hardens the Unix Domain Socket control-plane path with:
//! - Same-UID enforcement via `SO_PEERCRED` / `getpeereid`
//! - `UdsStream` IO wrapper implementing tonic's `Connected` trait so that
//!   peer metadata is accessible through request extensions
//! - An optional `UdsAuthPolicy` that can restrict the maximum role available
//!   to UDS callers

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Result;
use pin_project_lite::pin_project;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UnixStream;
use tonic::transport::server::Connected;

use crate::control_plane::Role;

// ---------------------------------------------------------------------------
// UdsPeerInfo — injected into tonic request extensions via Connected
// ---------------------------------------------------------------------------

/// Peer credentials extracted from a UDS connection.
#[derive(Debug, Clone)]
pub struct UdsPeerInfo {
    /// User ID of the connecting process.
    pub uid: u32,
    /// Group ID of the connecting process (reserved for future policy use).
    pub _gid: u32,
    /// Process ID (available on Linux and macOS; `None` on some BSDs).
    pub pid: Option<i32>,
}

// ---------------------------------------------------------------------------
// UdsStream — AsyncRead + AsyncWrite + Connected wrapper
// ---------------------------------------------------------------------------

pin_project! {
    /// Wraps a [`UnixStream`] to carry peer credentials through tonic's
    /// `Connected` trait.
    pub struct UdsStream {
        #[pin]
        inner: UnixStream,
        peer: UdsPeerInfo,
    }
}

impl UdsStream {
    /// Construct from an accepted `UnixStream`.
    ///
    /// Peer credentials are extracted eagerly so they are available before any
    /// read/write.
    pub fn new(stream: UnixStream, peer: UdsPeerInfo) -> Self {
        Self {
            inner: stream,
            peer,
        }
    }
}

impl Connected for UdsStream {
    type ConnectInfo = UdsPeerInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        self.peer.clone()
    }
}

impl AsyncRead for UdsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl AsyncWrite for UdsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

// ---------------------------------------------------------------------------
// Peer UID validation
// ---------------------------------------------------------------------------

/// Reject connections from a UID different from the daemon's own UID.
///
/// Returns the extracted [`UdsPeerInfo`] on success.
pub fn validate_peer(stream: &UnixStream) -> io::Result<UdsPeerInfo> {
    let cred = stream.peer_cred()?;
    let peer = UdsPeerInfo {
        uid: cred.uid(),
        _gid: cred.gid(),
        pid: cred.pid(),
    };

    // SAFETY: libc::getuid is always safe to call.
    #[allow(clippy::undocumented_unsafe_blocks)]
    let my_uid = unsafe { libc::getuid() };

    if peer.uid != my_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "UDS peer UID {} does not match daemon UID {}",
                peer.uid, my_uid
            ),
        ));
    }
    Ok(peer)
}

// ---------------------------------------------------------------------------
// Optional UDS authorization policy
// ---------------------------------------------------------------------------

/// Lightweight UDS authorization policy.
///
/// When loaded, restricts the maximum role available to UDS callers.  An absent
/// policy file means "no restriction" (backward-compatible full admin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdsAuthPolicy {
    /// Maximum role granted to UDS callers.
    #[serde(default = "default_max_role")]
    pub max_role: Role,
    /// When true, read-only RPCs are also recorded in the audit log.
    /// Useful in multi-user deployments where forensic coverage matters.
    #[serde(default)]
    pub audit_all_reads: bool,
}

fn default_max_role() -> Role {
    Role::Operator
}

impl Default for UdsAuthPolicy {
    fn default() -> Self {
        Self {
            max_role: default_max_role(),
            audit_all_reads: false,
        }
    }
}

/// Load the UDS authorization policy from `{cp_dir}/uds-policy.yaml`.
///
/// Returns `None` if the file does not exist (no restriction).
pub fn load_uds_policy(
    data_dir: &Path,
    control_plane_dir: Option<&Path>,
) -> Result<Option<UdsAuthPolicy>> {
    let cp_dir = match control_plane_dir {
        Some(d) => d.to_path_buf(),
        None => data_dir.join("control-plane"),
    };
    let path = cp_dir.join("uds-policy.yaml");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let policy: UdsAuthPolicy = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(Some(policy))
}

// ---------------------------------------------------------------------------
// Peer executable resolution (forensic audit only — NOT for authorization)
// ---------------------------------------------------------------------------

/// Attempt to resolve the executable path of a peer process by PID.
///
/// This is best-effort and used solely for audit enrichment.  The result
/// must **never** be used for authorization decisions because the
/// executable could change between credential extraction and this call
/// (TOCTOU), and a same-UID process can trivially spoof its binary path.
pub fn resolve_peer_exe(pid: i32) -> Option<String> {
    resolve_peer_exe_platform(pid)
}

#[cfg(target_os = "linux")]
fn resolve_peer_exe_platform(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn resolve_peer_exe_platform(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buf` is a valid, non-aliased allocation large enough for
    // PROC_PIDPATHINFO_MAXSIZE bytes.  `proc_pidpath` writes up to that
    // many bytes and returns the actual length written, which we use to
    // truncate the buffer before converting to a String.
    let ret =
        unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32) };
    if ret > 0 {
        buf.truncate(ret as usize);
        String::from_utf8(buf).ok()
    } else {
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn resolve_peer_exe_platform(_pid: i32) -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_allows_operator() {
        let policy = UdsAuthPolicy::default();
        assert_eq!(policy.max_role, Role::Operator);
        assert!(policy.max_role.allows(Role::Operator));
        assert!(!policy.max_role.allows(Role::Admin));
    }

    #[test]
    fn read_only_policy_denies_operator() {
        let policy = UdsAuthPolicy {
            max_role: Role::ReadOnly,
            ..Default::default()
        };
        assert!(policy.max_role.allows(Role::ReadOnly));
        assert!(!policy.max_role.allows(Role::Operator));
        assert!(!policy.max_role.allows(Role::Admin));
    }

    #[test]
    fn operator_policy_allows_operator_denies_admin() {
        let policy = UdsAuthPolicy {
            max_role: Role::Operator,
            ..Default::default()
        };
        assert!(policy.max_role.allows(Role::ReadOnly));
        assert!(policy.max_role.allows(Role::Operator));
        assert!(!policy.max_role.allows(Role::Admin));
    }

    #[test]
    fn absent_policy_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_uds_policy(tmp.path(), None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn policy_file_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let cp_dir = tmp.path().join("control-plane");
        std::fs::create_dir_all(&cp_dir).unwrap();
        let path = cp_dir.join("uds-policy.yaml");
        std::fs::write(&path, "max_role: read_only\n").unwrap();

        let policy = load_uds_policy(tmp.path(), None).unwrap().unwrap();
        assert_eq!(policy.max_role, Role::ReadOnly);
    }

    /// Existing policy files that omit max_role should now default to Operator
    /// (not Admin) via the serde default.
    #[test]
    fn policy_file_omitting_max_role_defaults_to_operator() {
        let tmp = tempfile::tempdir().unwrap();
        let cp_dir = tmp.path().join("control-plane");
        std::fs::create_dir_all(&cp_dir).unwrap();
        let path = cp_dir.join("uds-policy.yaml");
        std::fs::write(&path, "audit_all_reads: true\n").unwrap();

        let policy = load_uds_policy(tmp.path(), None).unwrap().unwrap();
        assert_eq!(policy.max_role, Role::Operator);
        assert!(policy.audit_all_reads);
    }

    #[test]
    fn explicit_admin_policy_file_grants_admin() {
        let tmp = tempfile::tempdir().unwrap();
        let cp_dir = tmp.path().join("control-plane");
        std::fs::create_dir_all(&cp_dir).unwrap();
        let path = cp_dir.join("uds-policy.yaml");
        std::fs::write(&path, "max_role: admin\n").unwrap();

        let policy = load_uds_policy(tmp.path(), None).unwrap().unwrap();
        assert_eq!(policy.max_role, Role::Admin);
        assert!(policy.max_role.allows(Role::Admin));
    }
}
