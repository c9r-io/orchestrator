use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Uri};
use tower::service_fn;

/// Maximum gRPC decoding message size (64 MB).
const MAX_GRPC_DECODE_SIZE: usize = 64 * 1024 * 1024;

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

/// Whether the connection uses UDS (no RBAC enforcement) or TLS (RBAC enforced).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Uds,
    Tls,
}

/// Discover the daemon socket path from environment or default location.
fn discover_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("ORCHESTRATOR_SOCKET") {
        return PathBuf::from(path);
    }
    if let Ok(dir) = std::env::var("ORCHESTRATORD_DATA_DIR") {
        return PathBuf::from(dir).join("orchestrator.sock");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".orchestratord/orchestrator.sock")
}

/// Connect to the daemon using the best available transport.
///
/// Returns the raw channel and transport kind so that the caller can decide
/// whether RBAC probing is needed (TLS) or can be skipped (UDS).
pub async fn connect(
    explicit_control_plane_config: Option<&str>,
) -> Result<(Channel, TransportKind)> {
    // 1. ORCHESTRATOR_SOCKET env → UDS
    if explicit_control_plane_config.is_none() && std::env::var_os("ORCHESTRATOR_SOCKET").is_some()
    {
        return connect_uds().await.map(|ch| (ch, TransportKind::Uds));
    }
    // 2. Explicit config (--control-plane-config flag or env) → TCP/TLS
    if let Some(path) = discover_explicit_control_plane_config(explicit_control_plane_config)? {
        return connect_secure(&path)
            .await
            .map(|ch| (ch, TransportKind::Tls));
    }
    // 3. Local socket file exists → UDS
    if explicit_control_plane_config.is_none() {
        let socket = discover_socket_path();
        if socket.exists() {
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

/// Return the max decoding size for clients created from our channel.
pub fn max_decode_size() -> usize {
    MAX_GRPC_DECODE_SIZE
}

async fn connect_uds() -> Result<Channel> {
    let socket_path = discover_socket_path();

    if !socket_path.exists() {
        anyhow::bail!(
            "daemon socket not found at {} and no control-plane config was discovered. Is the daemon running?",
            socket_path.display()
        );
    }

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
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                last_err = Some(e);
            }
        }
    }

    match last_err {
        Some(e) => Err(e).with_context(|| {
            format!(
                "failed to connect to daemon at {} after {} attempts",
                socket_path.display(),
                max_attempts,
            )
        }),
        None => anyhow::bail!(
            "failed to connect to daemon at {} after {} attempts",
            socket_path.display(),
            max_attempts,
        ),
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

fn discover_home_control_plane_config() -> Result<Option<PathBuf>> {
    let home = match dirs::home_dir() {
        Some(home) => home,
        None => return Ok(None),
    };
    let path = home.join(".orchestratord/control-plane/config.yaml");
    if path.exists() {
        return Ok(Some(path));
    }
    Ok(None)
}

fn load_control_plane_config(path: &Path) -> Result<ControlPlaneConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}
