//! Outbound-only client for the optional managed Slack Integration Gateway.

use agent_orchestrator::source_connection::{
    AsyncSourceConnectionRepository, SourceConnectionProvider,
};
use agent_orchestrator::state::InnerState;
use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

const MAX_GATEWAY_RESPONSE_BYTES: usize = 256 * 1024;

/// Validated optional Gateway client configuration.
#[derive(Clone)]
pub(crate) struct SlackGatewayClient {
    client: Client,
    origin: Url,
    enrollment_key: String,
}

/// Daemon-side provider port that decrypts one installation credential only for one call.
pub(crate) struct SlackGatewayProvider {
    state: Arc<InnerState>,
    client: Arc<SlackGatewayClient>,
}

impl SlackGatewayProvider {
    pub(crate) fn new(state: Arc<InnerState>, client: Arc<SlackGatewayClient>) -> Self {
        Self { state, client }
    }
}

impl SourceConnectionProvider for SlackGatewayProvider {
    fn permalink<'a>(
        &'a self,
        project_id: &'a str,
        connection_id: &'a str,
        channel_id: &'a str,
        message_ts: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let repository =
                AsyncSourceConnectionRepository::new(self.state.async_database.clone());
            let daemon_id = repository.daemon_id().await?;
            let credential = repository
                .credential(project_id, connection_id, &daemon_id)
                .await?
                .context("managed_source_connection_credential_missing")?;
            let keyring = agent_orchestrator::secret_key_lifecycle::load_keyring(
                &self.state.data_dir,
                &self.state.db_path,
            )?;
            let encryption =
                agent_orchestrator::secret_store_crypto::SecretEncryption::from_keyring(&keyring)?;
            let pairing = encryption.decrypt_source_connection_credential(
                project_id,
                connection_id,
                &credential.pairing_secret_ciphertext,
            )?;
            let response = self
                .client
                .permalink(
                    &credential.installation_id,
                    &daemon_id,
                    credential.generation,
                    &pairing,
                    channel_id,
                    message_ts,
                )
                .await?;
            if response.generation != credential.generation {
                bail!("credential_generation_stale");
            }
            validate_permalink(&response.permalink, channel_id)?;
            Ok(response.permalink)
        })
    }
}

fn validate_permalink(value: &str, channel_id: &str) -> Result<()> {
    let parsed = Url::parse(value).context("slack_permalink_invalid")?;
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() != "https"
        || !(host == "slack.com" || host.ends_with(".slack.com"))
        || !parsed
            .path_segments()
            .is_some_and(|mut segments| segments.any(|segment| segment == channel_id))
    {
        bail!("slack_permalink_rejected");
    }
    Ok(())
}

impl std::fmt::Debug for SlackGatewayClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SlackGatewayClient")
            .field("origin", &self.origin.as_str())
            .field("enrollment_key", &"[REDACTED]")
            .finish()
    }
}

impl SlackGatewayClient {
    /// Builds a strict HTTPS, no-redirect Gateway client.
    pub(crate) fn new(origin: &str, enrollment_key: String) -> Result<Self> {
        let parsed = Url::parse(origin).context("invalid Slack Gateway URL")?;
        let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
        if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
            bail!("Slack Gateway URL must use HTTPS outside loopback tests");
        }
        if parsed.host_str().is_none()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            bail!("Slack Gateway URL must be an origin without path, query, or fragment");
        }
        if enrollment_key.len() < 32 {
            bail!("Slack Gateway enrollment key must contain at least 32 bytes");
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .context("failed to build Slack Gateway client")?;
        Ok(Self {
            client,
            origin: parsed,
            enrollment_key,
        })
    }

    pub(crate) fn origin(&self) -> &str {
        self.origin.as_str().trim_end_matches('/')
    }

    pub(crate) async fn capabilities(&self) -> Result<GatewayCapabilities> {
        self.get_json("v1/capabilities", None).await
    }

    pub(crate) async fn create_intent(
        &self,
        daemon_id: &str,
        project_id: &str,
        actor_id: &str,
    ) -> Result<GatewayIntentCreated> {
        self.post_json(
            "v1/oauth/intents",
            Some(&self.enrollment_key),
            &serde_json::json!({
                "daemon_id": daemon_id,
                "project_id": project_id,
                "actor_id": actor_id,
                "requested_scopes": ["reactions:read"]
            }),
        )
        .await
    }

    pub(crate) async fn create_dedicated_import_slot(
        &self,
        connection_id: &str,
        daemon_id: &str,
        project_id: &str,
        manifest_version: &str,
        manifest_digest: &str,
    ) -> Result<GatewayDedicatedImportSlot> {
        self.post_json(
            "v1/dedicated/import-slots",
            Some(&self.enrollment_key),
            &serde_json::json!({
                "connection_id": connection_id,
                "daemon_id": daemon_id,
                "project_id": project_id,
                "manifest_version": manifest_version,
                "manifest_digest": manifest_digest
            }),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn import_dedicated_app(
        &self,
        connection_id: &str,
        daemon_id: &str,
        project_id: &str,
        actor_id: &str,
        manifest_digest: &str,
        import_secret: &str,
        credentials: &GatewayDedicatedCredentials<'_>,
    ) -> Result<GatewayDedicatedImport> {
        let response: GatewayDedicatedImport = self
            .post_json(
                "v1/dedicated/import",
                Some(import_secret),
                &serde_json::json!({
                    "connection_id": connection_id,
                    "daemon_id": daemon_id,
                    "project_id": project_id,
                    "actor_id": actor_id,
                    "credentials": {
                        "app_id": credentials.app_id,
                        "client_id": credentials.client_id,
                        "client_secret": credentials.client_secret,
                        "signing_secret": credentials.signing_secret
                    }
                }),
            )
            .await?;
        if response.connection_id != connection_id || response.credential_generation < 1 {
            bail!("dedicated_import_receipt_identity_mismatch");
        }
        let payload = format!(
            "{}:{}:{}:{}",
            response.connection_id,
            response.app_id_digest,
            response.credential_generation,
            manifest_digest
        );
        verify_dedicated_receipt(&self.enrollment_key, &payload, &response.receipt_signature)?;
        Ok(response)
    }

    pub(crate) async fn intent_status(
        &self,
        intent_id: &str,
        poll_secret: &str,
    ) -> Result<GatewayIntentStatus> {
        self.get_json(&format!("v1/oauth/intents/{intent_id}"), Some(poll_secret))
            .await
    }

    pub(crate) async fn cancel_intent(&self, intent_id: &str, poll_secret: &str) -> Result<()> {
        let url = self.endpoint(&format!("v1/oauth/intents/{intent_id}"))?;
        let response = self
            .client
            .delete(url)
            .bearer_auth(poll_secret)
            .send()
            .await
            .context("Slack Gateway request failed")?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(());
        }
        Err(provider_error(response).await)
    }

    pub(crate) async fn claim(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
        after_cursor: i64,
    ) -> Result<Vec<GatewayDelivery>> {
        let response: GatewayClaimResponse = self
            .post_json(
                "v1/deliveries/claim",
                Some(pairing_secret),
                &serde_json::json!({
                    "installation_id": installation_id,
                    "daemon_id": daemon_id,
                    "after_cursor": after_cursor,
                    "limit": 50,
                    "lease_secs": 30
                }),
            )
            .await?;
        Ok(response.deliveries)
    }

    pub(crate) async fn acknowledge(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
        cursors: &[i64],
    ) -> Result<i64> {
        let response: GatewayAckResponse = self
            .post_json(
                "v1/deliveries/ack",
                Some(pairing_secret),
                &serde_json::json!({
                    "installation_id": installation_id,
                    "daemon_id": daemon_id,
                    "cursors": cursors
                }),
            )
            .await?;
        Ok(response.last_acked_cursor)
    }

    pub(crate) async fn permalink(
        &self,
        installation_id: &str,
        daemon_id: &str,
        generation: i64,
        pairing_secret: &str,
        channel_id: &str,
        message_ts: &str,
    ) -> Result<GatewayPermalinkResponse> {
        self.post_json(
            "v1/provider/permalink",
            Some(pairing_secret),
            &serde_json::json!({
                "installation_id": installation_id,
                "daemon_id": daemon_id,
                "generation": generation,
                "channel_id": channel_id,
                "message_ts": message_ts
            }),
        )
        .await
    }

    pub(crate) async fn disconnect(
        &self,
        installation_id: &str,
        daemon_id: &str,
        expected_version: i64,
        pairing_secret: &str,
    ) -> Result<GatewayInstallation> {
        self.post_json(
            "v1/installations/disconnect",
            Some(pairing_secret),
            &serde_json::json!({
                "installation_id": installation_id,
                "daemon_id": daemon_id,
                "expected_version": expected_version
            }),
        )
        .await
    }

    pub(crate) async fn transfer(
        &self,
        installation_id: &str,
        daemon_id: &str,
        expected_version: i64,
        target_daemon_id: &str,
        pairing_secret: &str,
    ) -> Result<GatewayTransferResponse> {
        self.post_json(
            "v1/installations/transfer",
            Some(pairing_secret),
            &serde_json::json!({
                "installation_id": installation_id,
                "daemon_id": daemon_id,
                "expected_version": expected_version,
                "target_daemon_id": target_daemon_id
            }),
        )
        .await
    }

    pub(crate) async fn claim_ownership_transfers(
        &self,
        daemon_id: &str,
    ) -> Result<Vec<GatewayOwnershipTransfer>> {
        let response: GatewayOwnershipTransferResponse = self
            .post_json(
                "v1/installations/transfers/claim",
                Some(&self.enrollment_key),
                &serde_json::json!({"daemon_id": daemon_id}),
            )
            .await?;
        Ok(response.transfers)
    }

    pub(crate) async fn acknowledge_ownership_transfer(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
    ) -> Result<()> {
        let request = self
            .client
            .post(self.endpoint("v1/installations/transfers/ack")?)
            .bearer_auth(pairing_secret)
            .json(&serde_json::json!({
                "installation_id": installation_id,
                "daemon_id": daemon_id
            }));
        let response = request
            .send()
            .await
            .context("Slack Gateway request failed")?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(());
        }
        Err(provider_error(response).await)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
    ) -> Result<T> {
        let mut request = self.client.get(self.endpoint(path)?);
        if let Some(secret) = bearer {
            request = request.bearer_auth(secret);
        }
        parse_json(
            request
                .send()
                .await
                .context("Slack Gateway request failed")?,
        )
        .await
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &serde_json::Value,
    ) -> Result<T> {
        let mut request = self.client.post(self.endpoint(path)?).json(body);
        if let Some(secret) = bearer {
            request = request.bearer_auth(secret);
        }
        parse_json(
            request
                .send()
                .await
                .context("Slack Gateway request failed")?,
        )
        .await
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.origin
            .join(path)
            .context("failed to build Slack Gateway endpoint")
    }
}

async fn parse_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        return Err(provider_error(response).await);
    }
    let bytes = response
        .bytes()
        .await
        .context("failed to read Slack Gateway response")?;
    if bytes.len() > MAX_GATEWAY_RESPONSE_BYTES {
        bail!("slack_gateway_response_too_large");
    }
    serde_json::from_slice(&bytes).context("Slack Gateway response contract mismatch")
}

async fn provider_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let bytes = response.bytes().await.unwrap_or_default();
    let code = if bytes.len() <= MAX_GATEWAY_RESPONSE_BYTES {
        serde_json::from_slice::<GatewayError>(&bytes)
            .ok()
            .map(|value| value.error)
            .filter(|value| valid_error_code(value))
            .unwrap_or_else(|| "slack_gateway_error".to_string())
    } else {
        "slack_gateway_response_too_large".to_string()
    };
    anyhow::anyhow!("{code} ({})", status.as_u16())
}

fn valid_error_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn verify_dedicated_receipt(key: &str, payload: &str, signature: &str) -> Result<()> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let provided = hex::decode(signature).context("dedicated_import_receipt_invalid")?;
    let mut mac = <Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(key.as_bytes())
        .map_err(|_| anyhow::anyhow!("dedicated_import_receipt_invalid"))?;
    mac.update(b"orchestrator-dedicated-app-receipt-v1:");
    mac.update(payload.as_bytes());
    mac.verify_slice(&provided)
        .map_err(|_| anyhow::anyhow!("dedicated_import_receipt_invalid"))
}

#[derive(Debug, Deserialize)]
struct GatewayError {
    error: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayCapabilities {
    pub(crate) protocol_version: u32,
    pub(crate) supported_modes: Vec<String>,
    pub(crate) max_delivery_batch: u32,
    pub(crate) permalink_proxy: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayIntentCreated {
    pub(crate) intent_id: String,
    pub(crate) authorize_url: String,
    pub(crate) poll_secret: String,
    pub(crate) expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayDedicatedImportSlot {
    pub(crate) connection_id: String,
    pub(crate) import_secret: String,
    pub(crate) expires_at: String,
    pub(crate) oauth_callback_url: String,
    pub(crate) events_url: String,
}

pub(crate) struct GatewayDedicatedCredentials<'a> {
    pub(crate) app_id: &'a str,
    pub(crate) client_id: &'a str,
    pub(crate) client_secret: &'a str,
    pub(crate) signing_secret: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayDedicatedImport {
    pub(crate) connection_id: String,
    pub(crate) app_id_digest: String,
    pub(crate) credential_generation: i64,
    pub(crate) receipt_signature: String,
    pub(crate) intent_id: String,
    pub(crate) authorize_url: String,
    pub(crate) poll_secret: String,
    pub(crate) expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayIntentStatus {
    pub(crate) intent_id: String,
    pub(crate) status: String,
    pub(crate) expires_at: String,
    pub(crate) error_code: Option<String>,
    pub(crate) installation: Option<GatewayInstallation>,
    pub(crate) pairing_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayInstallation {
    pub(crate) id: String,
    pub(crate) team_digest: String,
    pub(crate) enterprise_digest: Option<String>,
    pub(crate) owner_daemon_id: String,
    pub(crate) owner_project_id: String,
    pub(crate) provisioning_mode: String,
    pub(crate) app_connection_id: Option<String>,
    pub(crate) app_id_digest: Option<String>,
    pub(crate) manifest_version: Option<String>,
    pub(crate) generation: i64,
    pub(crate) version: i64,
    pub(crate) state: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) last_acked_cursor: i64,
    pub(crate) last_error_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayClaimResponse {
    pub(crate) deliveries: Vec<GatewayDelivery>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayDelivery {
    pub(crate) cursor: i64,
    pub(crate) delivery_id: String,
    pub(crate) event: GatewayEvent,
    pub(crate) lease_expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayEvent {
    pub(crate) external_event_id: String,
    pub(crate) event_type: String,
    pub(crate) installation_id: String,
    pub(crate) external_actor_id: Option<String>,
    pub(crate) reaction: Option<String>,
    pub(crate) channel_id: Option<String>,
    pub(crate) message_ts: Option<String>,
    pub(crate) event_ts: String,
    pub(crate) team_digest: String,
    pub(crate) enterprise_digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GatewayAckResponse {
    last_acked_cursor: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayPermalinkResponse {
    pub(crate) permalink: String,
    pub(crate) generation: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayTransferResponse {
    pub(crate) installation: GatewayInstallation,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GatewayOwnershipTransfer {
    pub(crate) installation: GatewayInstallation,
    pub(crate) pairing_secret: String,
}

#[derive(Debug, Deserialize)]
struct GatewayOwnershipTransferResponse {
    transfers: Vec<GatewayOwnershipTransfer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_rejects_non_tls_and_non_origin_urls() {
        let key = "0123456789abcdef0123456789abcdef".to_string();
        assert!(SlackGatewayClient::new("https://gateway.example", key.clone()).is_ok());
        assert!(SlackGatewayClient::new("http://gateway.example", key.clone()).is_err());
        assert!(SlackGatewayClient::new("https://gateway.example/private", key).is_err());
    }
}
