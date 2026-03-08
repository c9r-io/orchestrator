use anyhow::{Context, Result};
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

/// Discover the daemon socket path from environment or default location.
fn discover_socket_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("ORCHESTRATOR_SOCKET") {
        return std::path::PathBuf::from(path);
    }

    // Default: look in the app root's data directory
    let app_root = std::env::var("ORCHESTRATOR_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    app_root.join("data/orchestrator.sock")
}

/// Connect to the daemon via Unix Domain Socket.
pub async fn connect() -> Result<OrchestratorServiceClient<Channel>> {
    let socket_path = discover_socket_path();

    if !socket_path.exists() {
        anyhow::bail!(
            "daemon socket not found at {}. Is the daemon running?\n  Start it with: orchestratord --foreground --workers 2",
            socket_path.display()
        );
    }

    let socket_path_clone = socket_path.clone();
    let channel = Endpoint::try_from("http://[::]:50051")
        .context("failed to create endpoint")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = socket_path_clone.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
            }
        }))
        .await
        .with_context(|| {
            format!(
                "failed to connect to daemon at {}. Is the daemon running?",
                socket_path.display()
            )
        })?;

    Ok(OrchestratorServiceClient::new(channel))
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
