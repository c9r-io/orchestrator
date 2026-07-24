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

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct SocketEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<std::ffi::OsString>,
    }

    impl SocketEnvGuard {
        fn missing_socket(path: &std::path::Path) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
            let previous = std::env::var_os("ORCHESTRATOR_SOCKET");
            // SAFETY: the module-local lock serializes this environment mutation.
            unsafe { std::env::set_var("ORCHESTRATOR_SOCKET", path) };
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for SocketEnvGuard {
        fn drop(&mut self) {
            // SAFETY: the module-local lock remains held while the value is restored.
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var("ORCHESTRATOR_SOCKET", value),
                    None => std::env::remove_var("ORCHESTRATOR_SOCKET"),
                }
            }
        }
    }

    #[tokio::test]
    async fn missing_uds_endpoint_is_reported_before_dispatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join("missing.sock");
        let _guard = SocketEnvGuard::missing_socket(&socket);

        let error = match connect(None).await {
            Ok(_) => panic!("missing UDS unexpectedly connected"),
            Err(error) => error,
        };
        let message = format!("{error:#}");
        assert!(message.contains("daemon socket not found"), "{message}");
        assert!(message.contains("missing.sock"), "{message}");
    }

    #[tokio::test]
    async fn invalid_tls_material_is_reported_before_dispatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("control-plane.yaml");
        std::fs::write(
            &config,
            format!(
                r#"
current_context: default
clusters:
  - name: default
    cluster:
      server: https://127.0.0.1:50051
      certificate_authority: {}/missing-ca.pem
users:
  - name: default
    user:
      client_certificate: {}/missing-client.pem
      client_key: {}/missing-key.pem
contexts:
  - name: default
    context:
      cluster: default
      user: default
"#,
                temp.path().display(),
                temp.path().display(),
                temp.path().display()
            ),
        )
        .expect("write control-plane config");

        let error = match connect(config.to_str()).await {
            Ok(_) => panic!("invalid TLS unexpectedly connected"),
            Err(error) => error,
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("failed to read CA certificate"),
            "{message}"
        );
        assert!(!message.contains("PRIVATE KEY"), "{message}");
    }

    #[tokio::test]
    async fn invalid_tcp_address_is_rejected_without_fallback() {
        let error = match connect_tcp("not a valid authority").await {
            Ok(_) => panic!("invalid address unexpectedly connected"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("invalid address"));
    }
}
