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
/// Prefers the Unix socket when `ORCHESTRATOR_SOCKET` is set and no explicit
/// control-plane config is requested. Otherwise it discovers a TLS client
/// config and falls back to the Unix socket when none is present.
pub async fn connect(
    explicit_control_plane_config: Option<&str>,
) -> Result<OrchestratorServiceClient<Channel>> {
    if explicit_control_plane_config.is_none() && std::env::var_os("ORCHESTRATOR_SOCKET").is_some()
    {
        return connect_uds().await;
    }
    if let Some(path) = discover_control_plane_config(explicit_control_plane_config)? {
        return connect_secure(&path).await;
    }
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

    Err(last_err.unwrap()).with_context(|| {
        format!(
            "failed to connect to daemon at {} after {} attempts. Is the daemon running?",
            socket_path.display(),
            max_attempts,
        )
    })
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

fn discover_control_plane_config(explicit: Option<&str>) -> Result<Option<PathBuf>> {
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
}
