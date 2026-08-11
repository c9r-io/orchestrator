use anyhow::{Context, Result};
use orchestrator_config::paths;
use std::path::{Path, PathBuf};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Uri};
use tower::service_fn;

use crate::config::ControlPlaneConfig;

/// Maximum gRPC decoding message size (64 MB).
///
/// The default tonic limit is 4 MB, which is too small for `manifest export`
/// on repositories with many resources.  We raise the ceiling here so that
/// every client can receive large payloads without hitting a decoding error.
pub const MAX_GRPC_DECODE_SIZE: usize = 64 * 1024 * 1024;

/// Whether the connection uses UDS (no RBAC enforcement) or TLS (RBAC enforced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Uds,
    Tls,
}

/// Environment variable naming the daemon socket directly.
pub const SOCKET_ENV: &str = "ORCHESTRATOR_SOCKET";

/// Discover the daemon socket path from environment or default location.
///
/// `ORCHESTRATOR_SOCKET` names a socket outright and is the one thing this
/// function knows that `orchestrator_config::paths` does not; everything below
/// it is that module's answer, not a second opinion. FR-163 removed the second
/// opinion — this function used to spell both `orchestrator.sock` and the home
/// fallback itself.
pub fn discover_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var(SOCKET_ENV) {
        return PathBuf::from(path);
    }
    paths::socket_path(&paths::data_dir())
}

/// Whether a daemon is currently accepting connections on `socket_path`.
///
/// The question is "is anyone listening", and `Path::exists` answers a different
/// one. The daemon unlinks its socket when it binds and again on a clean
/// shutdown, but a crash or `SIGKILL` leaves the inode behind, and an existence
/// probe reads that corpse as a live daemon: the CLI commits to UDS, retries
/// three times, and reports "Is the daemon running?" while a working TLS
/// control plane sits one discovery step later. Connecting is the only probe
/// that distinguishes the two.
async fn socket_is_listening(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    tokio::net::UnixStream::connect(socket_path).await.is_ok()
}

/// Connect to the daemon using the best available transport.
///
/// Returns the raw channel and transport kind so that the caller can decide
/// whether RBAC probing is needed (TLS) or can be skipped (UDS).
///
/// Connection priority:
/// 1. `ORCHESTRATOR_SOCKET` env (no explicit config) → UDS
/// 2. Explicit control-plane config (flag or env) → TCP/TLS
/// 3. Default socket is **accepting connections** → UDS
/// 4. Auto-discover `~/.orchestrator/control-plane/config.yaml` → TCP/TLS
/// 5. Fallback → UDS
///
/// Step 3 probes by connecting, not by existence: a socket left behind by a
/// crashed daemon must not capture the discovery chain that would otherwise
/// reach step 4. Step 1 is deliberately *not* probed the same way — naming
/// `ORCHESTRATOR_SOCKET` is an explicit choice of transport, and silently
/// falling through to TLS because it happens to be down would hide the operator's
/// own configuration from them.
pub async fn connect(
    explicit_control_plane_config: Option<&str>,
) -> Result<(Channel, TransportKind)> {
    // 1. ORCHESTRATOR_SOCKET env → UDS
    if explicit_control_plane_config.is_none() && std::env::var_os(SOCKET_ENV).is_some() {
        return connect_uds().await.map(|ch| (ch, TransportKind::Uds));
    }
    // 2. Explicit config (--control-plane-config flag or env) → TCP/TLS
    if let Some(path) = discover_explicit_control_plane_config(explicit_control_plane_config)? {
        return connect_secure(&path)
            .await
            .map(|ch| (ch, TransportKind::Tls));
    }
    // 3. Local socket is listening → UDS (skip when explicit config was requested)
    if explicit_control_plane_config.is_none() {
        let socket = discover_socket_path();
        if socket_is_listening(&socket).await {
            return connect_uds().await.map(|ch| (ch, TransportKind::Uds));
        }
    }
    // 4. Auto-discover home-dir config → TCP/TLS
    if let Some(path) = discover_home_control_plane_config()? {
        return connect_secure(&path)
            .await
            .map(|ch| (ch, TransportKind::Tls));
    }
    // 5. Fallback → UDS
    connect_uds().await.map(|ch| (ch, TransportKind::Uds))
}

/// Diagnostic printed when a socket file exists but nothing is accepting on it.
///
/// Asserted by name in `scripts/qa/test-stale-socket-discovery.sh`. The two
/// failures need different words because they need different fixes: a missing
/// socket means start the daemon, a dead socket means the last daemon did not
/// exit cleanly and the file is debris. Reporting the second as the first is
/// what sent operators looking for a daemon that was never going to be there.
pub const STALE_SOCKET_DIAGNOSTIC: &str = "socket exists but nothing is listening";

async fn connect_uds() -> Result<Channel> {
    let socket_path = discover_socket_path();

    if !socket_path.exists() {
        anyhow::bail!(
            "daemon socket not found at {} and no control-plane config was discovered. Is the daemon running?\n  Start it with: orchestratord --foreground --workers 2",
            socket_path.display()
        );
    }

    // Retry up to 3 times with 1s intervals to tolerate transient unavailability
    // (e.g. daemon is restarting via exec() and the new process hasn't bound the
    // socket yet).
    let max_attempts = 3;
    let mut last_err = None;
    for attempt in 1..=max_attempts {
        let socket_path_clone = socket_path.clone();
        let result = Endpoint::try_from("http://[::]:50051")
            .context("failed to create endpoint")?
            .connect_with_connector(service_fn(move |_: Uri| {
                let path = socket_path_clone.clone();
                async move {
                    let stream = tokio::net::UnixStream::connect(path).await?;
                    Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                }
            }))
            .await;

        match result {
            Ok(channel) => return Ok(channel),
            Err(e) => {
                if attempt < max_attempts {
                    eprintln!(
                        "daemon connection attempt {attempt}/{max_attempts} failed, retrying in 1s…",
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                last_err = Some(e);
            }
        }
    }

    // The retries above exist to ride out a restarting daemon, so the stale-socket
    // verdict is only reachable once they are spent: a socket that is still not
    // accepting after three attempts is debris, not a daemon mid-`exec()`.
    let context = format!(
        "failed to connect to daemon at {} after {} attempts. {}",
        socket_path.display(),
        max_attempts,
        stale_socket_advice(&socket_path)
    );
    match last_err {
        Some(e) => Err(e).context(context),
        None => anyhow::bail!(context),
    }
}

/// The trailing half of a UDS connection failure: what the operator should do.
fn stale_socket_advice(socket_path: &Path) -> String {
    if socket_path.exists() {
        format!(
            "The {} at {} — the daemon that created it exited without cleaning up, which is what a crash or SIGKILL leaves behind.\n  Start it with: orchestratord --foreground --workers 2\n  Or remove the stale socket: rm {}",
            STALE_SOCKET_DIAGNOSTIC,
            socket_path.display(),
            socket_path.display()
        )
    } else {
        "Is the daemon running?\n  Start it with: orchestratord --foreground --workers 2"
            .to_string()
    }
}

async fn connect_secure(config_path: &Path) -> Result<Channel> {
    let config = load_control_plane_config(config_path)?;
    let context = config
        .contexts
        .iter()
        .find(|entry| entry.name == config.current_context)
        .with_context(|| {
            format!(
                "current context '{}' not found in {}",
                config.current_context,
                config_path.display()
            )
        })?;
    let cluster = config
        .clusters
        .iter()
        .find(|entry| entry.name == context.context.cluster)
        .with_context(|| {
            format!(
                "cluster '{}' not found in {}",
                context.context.cluster,
                config_path.display()
            )
        })?;
    let user = config
        .users
        .iter()
        .find(|entry| entry.name == context.context.user)
        .with_context(|| {
            format!(
                "user '{}' not found in {}",
                context.context.user,
                config_path.display()
            )
        })?;

    let ca = std::fs::read(&cluster.cluster.certificate_authority).with_context(|| {
        format!(
            "failed to read CA certificate {}",
            cluster.cluster.certificate_authority
        )
    })?;
    let cert = std::fs::read(&user.user.client_certificate).with_context(|| {
        format!(
            "failed to read client certificate {}",
            user.user.client_certificate
        )
    })?;
    let key = std::fs::read(&user.user.client_key)
        .with_context(|| format!("failed to read client key {}", user.user.client_key))?;

    let endpoint = Endpoint::from_shared(cluster.cluster.server.clone())
        .context("invalid control-plane endpoint")?
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca))
                .identity(Identity::from_pem(cert, key)),
        )
        .context("failed to configure TLS client")?;
    let channel = endpoint
        .connect()
        .await
        .with_context(|| format!("failed to connect to {}", cluster.cluster.server))?;
    Ok(channel)
}

/// Check explicit path and `ORCHESTRATOR_CONTROL_PLANE_CONFIG` env only.
fn discover_explicit_control_plane_config(explicit: Option<&str>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        if !path.exists() {
            anyhow::bail!("control-plane config not found at {}", path.display());
        }
        return Ok(Some(path));
    }

    if let Ok(path) = std::env::var("ORCHESTRATOR_CONTROL_PLANE_CONFIG") {
        let path = PathBuf::from(path);
        if !path.exists() {
            anyhow::bail!("control-plane config not found at {}", path.display());
        }
        return Ok(Some(path));
    }

    Ok(None)
}

/// Check `~/.orchestratord/control-plane/config.yaml` auto-discovery.
/// Auto-discovery of the client control-plane config generated by a local daemon.
///
/// The canonical location is checked first and the pre-FR-163 documented one
/// second. Before FR-163 only the second was checked, and nothing has ever
/// written there: the daemon emits the bundle to
/// `~/.orchestrator/control-plane/config.yaml` while this function looked in
/// `~/.orchestratord/control-plane/config.yaml`. The branch was dead for its
/// whole life, which is why more than ten QA gates set
/// `ORCHESTRATOR_CONTROL_PLANE_CONFIG` explicitly instead of relying on it.
fn discover_home_control_plane_config() -> Result<Option<PathBuf>> {
    let home = match dirs::home_dir() {
        Some(home) => home,
        None => return Ok(None),
    };
    for path in [
        paths::client_control_plane_config(&home),
        paths::legacy_client_control_plane_config(&home),
    ] {
        if path.exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn load_control_plane_config(path: &Path) -> Result<ControlPlaneConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
fn discover_control_plane_config(explicit: Option<&str>) -> Result<Option<PathBuf>> {
    if let Some(path) = discover_explicit_control_plane_config(explicit)? {
        return Ok(Some(path));
    }
    discover_home_control_plane_config()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Process-local lock that serializes every test in this module which
    /// either *mutates* or *reads* one of the connect-related environment
    /// variables (`ORCHESTRATOR_SOCKET`, `ORCHESTRATORD_DATA_DIR`,
    /// `ORCHESTRATOR_CONTROL_PLANE_CONFIG`).  Cargo runs unit tests in
    /// parallel by default, and these vars are process-wide state — so
    /// without this serialization the tests race against each other and
    /// flake under workspace load (e.g. `local_socket_probe_…` would
    /// observe `ORCHESTRATOR_SOCKET` set by `connect_prefers_socket_…`
    /// running concurrently).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that takes the process-local env lock, snapshots the
    /// listed environment variables on construction, and restores them on
    /// drop.  Tests that mutate env vars must construct an `EnvGuard`
    /// before any `std::env::set_var` / `remove_var` call so that:
    ///
    /// 1. concurrent tests in the same module are serialized
    /// 2. the test cannot leak env-var mutations to subsequent tests even
    ///    if it panics mid-way through
    ///
    /// Mutex poisoning is recovered automatically (`into_inner`) so a
    /// single panicking test does not cascade-fail every other env-using
    /// test in the module.
    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        snapshot: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn new(vars: &[&'static str]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
            let snapshot = vars.iter().map(|&k| (k, std::env::var_os(k))).collect();
            Self {
                _lock: lock,
                snapshot,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: ENV_LOCK ensures no other test in this module is
            // reading or writing the env while we restore it.
            for (key, value) in &self.snapshot {
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn load_control_plane_config_parses_kubeconfig_shape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(
            &path,
            r#"
current_context: default
clusters:
  - name: default
    cluster:
      server: https://127.0.0.1:50051
      certificate_authority: /tmp/ca.crt
users:
  - name: default
    user:
      client_certificate: /tmp/client.crt
      client_key: /tmp/client.key
contexts:
  - name: default
    context:
      cluster: default
      user: default
"#,
        )
        .expect("write config");

        let config = load_control_plane_config(&path).expect("config");
        assert_eq!(config.current_context, "default");
        assert_eq!(config.clusters[0].cluster.server, "https://127.0.0.1:50051");
        assert_eq!(config.users[0].user.client_key, "/tmp/client.key");
    }

    #[test]
    fn discover_control_plane_config_prefers_explicit_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(
            &path,
            "current_context: default\nclusters: []\nusers: []\ncontexts: []\n",
        )
        .expect("write config");

        let discovered = discover_control_plane_config(Some(path.to_str().expect("utf8")))
            .expect("discover")
            .expect("path");
        assert_eq!(discovered, path);
    }

    #[test]
    fn connect_prefers_socket_when_env_is_present_and_no_explicit_config() {
        // EnvGuard takes ENV_LOCK and restores ORCHESTRATOR_SOCKET on drop,
        // so concurrent env-var tests cannot race with us.
        let _env = EnvGuard::new(&["ORCHESTRATOR_SOCKET", "ORCHESTRATORD_DATA_DIR"]);
        let temp = tempfile::tempdir().expect("tempdir");
        let expected = temp.path().join("missing.sock");
        // SAFETY: ENV_LOCK held by `_env` serializes env access across this
        // test module.
        unsafe {
            std::env::set_var("ORCHESTRATOR_SOCKET", &expected);
            // Defensive: clear DATA_DIR so a stale value from a prior test
            // run can't change discover_socket_path's fallback path.
            std::env::remove_var("ORCHESTRATORD_DATA_DIR");
        }

        let resolved = discover_socket_path();
        assert_eq!(
            resolved, expected,
            "discover_socket_path should prefer ORCHESTRATOR_SOCKET env"
        );
        assert!(
            !resolved.exists(),
            "socket should not exist — connect_uds would bail with 'daemon socket not found'"
        );
        // EnvGuard::drop restores the original env values.
    }

    #[test]
    fn discover_explicit_config_returns_none_when_no_explicit() {
        // With no explicit path and no env, should return None.
        let _env = EnvGuard::new(&["ORCHESTRATOR_CONTROL_PLANE_CONFIG"]);
        // SAFETY: ENV_LOCK held by `_env` serializes env access across this
        // test module.
        unsafe { std::env::remove_var("ORCHESTRATOR_CONTROL_PLANE_CONFIG") };
        let result = discover_explicit_control_plane_config(None).expect("no error");
        assert!(
            result.is_none(),
            "should return None without explicit config"
        );
    }

    #[test]
    fn discover_explicit_config_uses_explicit_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.yaml");
        std::fs::write(
            &path,
            "current_context: default\nclusters: []\nusers: []\ncontexts: []\n",
        )
        .expect("write config");

        let result = discover_explicit_control_plane_config(Some(path.to_str().expect("utf8")))
            .expect("no error")
            .expect("should find config");
        assert_eq!(result, path);
    }

    #[test]
    fn local_socket_probe_precedes_home_config_in_connect_priority() {
        let _env = EnvGuard::new(&["ORCHESTRATOR_SOCKET", "ORCHESTRATORD_DATA_DIR"]);
        let temp = tempfile::tempdir().expect("tempdir");
        // SAFETY: ENV_LOCK held by `_env` serializes env access across this
        // test module.
        unsafe {
            std::env::remove_var("ORCHESTRATOR_SOCKET");
            std::env::set_var("ORCHESTRATORD_DATA_DIR", temp.path());
        }

        let socket = discover_socket_path();
        assert_eq!(
            socket,
            temp.path().join("orchestrator.sock"),
            "discover_socket_path should use ORCHESTRATORD_DATA_DIR"
        );

        // When the socket exists, connect() step 3 fires before step 4.
        std::fs::write(&socket, "").expect("create socket stub");
        assert!(socket.exists(), "local socket should exist for probe");
        // EnvGuard::drop restores the original env values.
    }
}
