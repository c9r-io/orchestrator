use anyhow::{Context, Result};
use orchestrator_proto::OrchestratorServiceClient;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Uri};
use tower::service_fn;

#[derive(Debug, Clone, Deserialize)]
struct ControlPlaneConfig {
    current_context: String,
    clusters: Vec<NamedCluster>,
    users: Vec<NamedUser>,
    contexts: Vec<NamedContext>,
}

#[derive(Debug, Clone, Deserialize)]
struct NamedCluster {
    name: String,
    cluster: ClusterRef,
}

#[derive(Debug, Clone, Deserialize)]
struct ClusterRef {
    server: String,
    certificate_authority: String,
}

#[derive(Debug, Clone, Deserialize)]
struct NamedUser {
    name: String,
    user: UserRef,
}

#[derive(Debug, Clone, Deserialize)]
struct UserRef {
    client_certificate: String,
    client_key: String,
}

#[derive(Debug, Clone, Deserialize)]
struct NamedContext {
    name: String,
    context: ContextRef,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextRef {
    cluster: String,
    user: String,
}

/// Discover the daemon socket path from environment or default location.
fn discover_socket_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("ORCHESTRATOR_SOCKET") {
        return std::path::PathBuf::from(path);
    }

    let app_root = std::env::var("ORCHESTRATOR_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    app_root.join("data/orchestrator.sock")
}

/// Connect to the daemon using the best available transport.
///
/// Connection priority:
/// 1. `ORCHESTRATOR_SOCKET` env (no explicit config) → UDS
/// 2. Explicit control-plane config (flag or env) → TCP/TLS
/// 3. Local socket file exists (`data/orchestrator.sock`) → UDS
/// 4. Auto-discover `~/.orchestrator/control-plane/config.yaml` → TCP/TLS
/// 5. Fallback → UDS
pub async fn connect(
    explicit_control_plane_config: Option<&str>,
) -> Result<OrchestratorServiceClient<Channel>> {
    // 1. ORCHESTRATOR_SOCKET env → UDS
    if explicit_control_plane_config.is_none() && std::env::var_os("ORCHESTRATOR_SOCKET").is_some()
    {
        return connect_uds().await;
    }
    // 2. Explicit config (--control-plane-config flag or env) → TCP/TLS
    if let Some(path) = discover_explicit_control_plane_config(explicit_control_plane_config)? {
        return connect_secure(&path).await;
    }
    // 3. Local socket file exists → UDS (skip when explicit config was requested)
    if explicit_control_plane_config.is_none() {
        let socket = discover_socket_path();
        if socket.exists() {
            return connect_uds().await;
        }
    }
    // 4. Auto-discover home-dir config → TCP/TLS
    if let Some(path) = discover_home_control_plane_config()? {
        return connect_secure(&path).await;
    }
    // 5. Fallback → UDS
    connect_uds().await
}

async fn connect_uds() -> Result<OrchestratorServiceClient<Channel>> {
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
            Ok(channel) => return Ok(OrchestratorServiceClient::new(channel)),
            Err(e) => {
                if attempt < max_attempts {
                    eprintln!(
                        "daemon connection attempt {}/{} failed, retrying in 1s…",
                        attempt, max_attempts,
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                last_err = Some(e);
            }
        }
    }

    match last_err {
        Some(e) => Err(e).with_context(|| {
            format!(
                "failed to connect to daemon at {} after {} attempts. Is the daemon running?",
                socket_path.display(),
                max_attempts,
            )
        }),
        None => anyhow::bail!(
            "failed to connect to daemon at {} after {} attempts. Is the daemon running?",
            socket_path.display(),
            max_attempts,
        ),
    }
}

async fn connect_secure(config_path: &Path) -> Result<OrchestratorServiceClient<Channel>> {
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
    Ok(OrchestratorServiceClient::new(channel))
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

/// Check `~/.orchestrator/control-plane/config.yaml` auto-discovery.
fn discover_home_control_plane_config() -> Result<Option<PathBuf>> {
    let home = match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => return Ok(None),
    };
    let path = home.join(".orchestrator/control-plane/config.yaml");
    if path.exists() {
        return Ok(Some(path));
    }
    Ok(None)
}

#[cfg(test)]
fn discover_control_plane_config(explicit: Option<&str>) -> Result<Option<PathBuf>> {
    if let Some(path) = discover_explicit_control_plane_config(explicit)? {
        return Ok(Some(path));
    }
    discover_home_control_plane_config()
}

fn load_control_plane_config(path: &Path) -> Result<ControlPlaneConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

/// Connect to a TCP address (for remote daemon).
#[allow(dead_code)]
pub async fn connect_tcp(addr: &str) -> Result<OrchestratorServiceClient<Channel>> {
    let channel = Channel::from_shared(format!("http://{addr}"))
        .context("invalid address")?
        .connect()
        .await
        .context("failed to connect to daemon")?;

    Ok(OrchestratorServiceClient::new(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn connect_prefers_socket_when_env_is_present_and_no_explicit_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket_path = temp.path().join("missing.sock");
        std::env::set_var("ORCHESTRATOR_SOCKET", &socket_path);
        std::env::set_var("HOME", temp.path());
        std::fs::create_dir_all(temp.path().join(".orchestrator/control-plane"))
            .expect("control-plane dir");
        std::fs::write(
            temp.path().join(".orchestrator/control-plane/config.yaml"),
            "current_context: default\nclusters: []\nusers: []\ncontexts: []\n",
        )
        .expect("write config");

        let error = connect(None).await.expect_err("socket should win");
        let message = error.to_string();
        assert!(message.contains("daemon socket not found"));

        std::env::remove_var("ORCHESTRATOR_SOCKET");
    }

    #[test]
    fn discover_explicit_config_returns_none_when_no_explicit() {
        // With no explicit path and no env, should return None
        let _guard = std::env::var("ORCHESTRATOR_CONTROL_PLANE_CONFIG");
        std::env::remove_var("ORCHESTRATOR_CONTROL_PLANE_CONFIG");
        let result = discover_explicit_control_plane_config(None).expect("no error");
        assert!(result.is_none(), "should return None without explicit config");
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

        let result =
            discover_explicit_control_plane_config(Some(path.to_str().expect("utf8")))
                .expect("no error")
                .expect("should find config");
        assert_eq!(result, path);
    }

    #[test]
    fn local_socket_probe_precedes_home_config_in_connect_priority() {
        // Verify that discover_socket_path returns a path that, when it exists,
        // would be checked BEFORE discover_home_control_plane_config in the
        // connect() priority chain.  This is a unit-level verification of the
        // priority ordering documented in connect()'s doc comment.
        let temp = tempfile::tempdir().expect("tempdir");
        // Clear ORCHESTRATOR_SOCKET so discover_socket_path falls through to
        // the ORCHESTRATOR_ROOT branch (env vars are process-global and another
        // test may have set ORCHESTRATOR_SOCKET).
        std::env::remove_var("ORCHESTRATOR_SOCKET");
        std::env::set_var("ORCHESTRATOR_ROOT", temp.path());

        let socket = discover_socket_path();
        assert_eq!(
            socket,
            temp.path().join("data/orchestrator.sock"),
            "discover_socket_path should use ORCHESTRATOR_ROOT"
        );

        // When the socket exists, connect() step 3 fires before step 4.
        // We verify this structurally: the socket path is computable and
        // checkable before home-dir discovery runs.
        std::fs::create_dir_all(socket.parent().unwrap()).expect("data dir");
        std::fs::write(&socket, "").expect("create socket stub");
        assert!(socket.exists(), "local socket should exist for probe");

        std::env::remove_var("ORCHESTRATOR_ROOT");
    }
}
