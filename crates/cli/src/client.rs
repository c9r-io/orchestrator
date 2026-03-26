use anyhow::{Context, Result};
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

/// Connect to the daemon using the best available transport.
///
/// Delegates discovery and connection logic to `orchestrator_client` and wraps
/// the resulting channel in an `OrchestratorServiceClient`.
pub async fn connect(
    explicit_control_plane_config: Option<&str>,
) -> Result<OrchestratorServiceClient<Channel>> {
    let (channel, _transport) = orchestrator_client::connect(explicit_control_plane_config).await?;
    Ok(OrchestratorServiceClient::new(channel)
        .max_decoding_message_size(orchestrator_client::MAX_GRPC_DECODE_SIZE))
}

/// Connect to a TCP address (for remote daemon).
#[allow(dead_code)]
pub async fn connect_tcp(addr: &str) -> Result<OrchestratorServiceClient<Channel>> {
    let channel = Channel::from_shared(format!("http://{addr}"))
        .context("invalid address")?
        .connect()
        .await
        .context("failed to connect to daemon")?;

    Ok(OrchestratorServiceClient::new(channel)
        .max_decoding_message_size(orchestrator_client::MAX_GRPC_DECODE_SIZE))
}
