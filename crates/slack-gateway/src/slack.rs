//! Slack protocol boundary with bounded responses and privacy-safe errors.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use url::Url;

use crate::store::OfficialAppCredentials;

type HmacSha256 = Hmac<Sha256>;

const MAX_PROVIDER_RESPONSE_BYTES: usize = 64 * 1024;

/// Privacy-safe Slack provider failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackError {
    code: &'static str,
    retry_after: Option<u64>,
}

impl SlackError {
    /// Stable error code safe for logs, metrics, and Attention.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Provider-requested retry delay.
    pub fn retry_after(&self) -> Option<u64> {
        self.retry_after
    }

    fn new(code: &'static str) -> Self {
        Self {
            code,
            retry_after: None,
        }
    }
}

impl std::fmt::Display for SlackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}

impl std::error::Error for SlackError {}

/// Successful OAuth V2 installation response with credentials kept in memory only.
pub struct OAuthExchange {
    /// Slack team ID.
    pub team_id: String,
    /// Optional Slack Enterprise ID.
    pub enterprise_id: Option<String>,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// Bot token.
    pub bot_token: String,
}

impl std::fmt::Debug for OAuthExchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthExchange")
            .field("team_id", &"[REDACTED]")
            .field(
                "enterprise_id",
                &self.enterprise_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field("scopes", &self.scopes)
            .field("bot_token", &"[REDACTED]")
            .finish()
    }
}

/// Parsed and allowlisted Slack Events API input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackEvent {
    /// Slack URL verification challenge.
    UrlVerification { challenge: String },
    /// Message reaction event.
    ReactionAdded {
        app_id: String,
        event_id: String,
        team_id: String,
        enterprise_id: Option<String>,
        actor_id: String,
        reaction: String,
        channel_id: String,
        message_ts: String,
        event_ts: String,
    },
    /// App uninstall event.
    AppUninstalled {
        app_id: String,
        event_id: String,
        team_id: String,
        enterprise_id: Option<String>,
        event_ts: String,
    },
    /// Token revocation event.
    TokensRevoked {
        app_id: String,
        event_id: String,
        team_id: String,
        enterprise_id: Option<String>,
        event_ts: String,
    },
}

/// Result of an App Manifest create operation.
pub struct ManifestProvisionResult {
    /// Created app ID.
    pub app_id: String,
    /// Credentials returned by Slack and destined directly for encrypted storage.
    pub credentials: OfficialAppCredentials,
}

impl std::fmt::Debug for ManifestProvisionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManifestProvisionResult")
            .field("app_id", &self.app_id)
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}

/// Strict Slack HTTP client with redirects disabled.
#[derive(Clone)]
pub struct SlackClient {
    client: Client,
    base: Url,
}

impl std::fmt::Debug for SlackClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackClient")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

impl SlackClient {
    /// Creates a Slack client. The caller must validate any non-official base URL.
    pub fn new(base: &str, timeout: Duration) -> Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build Slack client")?;
        Ok(Self {
            client,
            base: Url::parse(base).context("invalid Slack base URL")?,
        })
    }

    /// Exchanges an OAuth code using official app credentials.
    pub async fn exchange_oauth(
        &self,
        code: &str,
        redirect_uri: &str,
        credentials: &OfficialAppCredentials,
    ) -> std::result::Result<OAuthExchange, SlackError> {
        if code.is_empty() || code.len() > 1024 {
            return Err(SlackError::new("oauth_code_invalid"));
        }
        let endpoint = self.endpoint("/api/oauth.v2.access")?;
        let response = self
            .client
            .post(endpoint)
            .form(&[
                ("code", code),
                ("client_id", credentials.client_id.as_str()),
                ("client_secret", credentials.client_secret.as_str()),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .map_err(|_| SlackError::new("slack_oauth_unavailable"))?;
        let retry_after = retry_after(&response);
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(SlackError {
                code: "slack_oauth_rate_limited",
                retry_after,
            });
        }
        if response.status().is_redirection() {
            return Err(SlackError::new("slack_redirect_rejected"));
        }
        if !response.status().is_success() {
            return Err(SlackError::new("slack_oauth_unavailable"));
        }
        let body = bounded_body(response).await?;
        let payload: OAuthResponse = serde_json::from_slice(&body)
            .map_err(|_| SlackError::new("slack_oauth_invalid_response"))?;
        if !payload.ok {
            return Err(classify_provider_error(payload.error.as_deref(), "oauth"));
        }
        let team_id = payload
            .team
            .and_then(|team| team.id)
            .filter(|value| valid_slack_id(value, 'T'))
            .ok_or_else(|| SlackError::new("slack_oauth_identity_missing"))?;
        let bot_token = payload
            .access_token
            .filter(|value| value.starts_with("xoxb-") && value.len() <= 4096)
            .ok_or_else(|| SlackError::new("slack_oauth_token_missing"))?;
        let scopes = payload
            .scope
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|scope| !scope.is_empty())
            .map(str::to_string)
            .collect();
        let enterprise_id = payload
            .enterprise
            .and_then(|enterprise| enterprise.id)
            .filter(|value| valid_slack_id(value, 'E'));
        Ok(OAuthExchange {
            team_id,
            enterprise_id,
            scopes,
            bot_token,
        })
    }

    /// Resolves one Slack message permalink using an installation token.
    pub async fn get_permalink(
        &self,
        bot_token: &str,
        channel: &str,
        message_ts: &str,
    ) -> std::result::Result<String, SlackError> {
        if !valid_slack_id(channel, 'C')
            && !valid_slack_id(channel, 'G')
            && !valid_slack_id(channel, 'D')
        {
            return Err(SlackError::new("slack_channel_invalid"));
        }
        if !valid_message_ts(message_ts) {
            return Err(SlackError::new("slack_message_ts_invalid"));
        }
        let mut endpoint = self.endpoint("/api/chat.getPermalink")?;
        endpoint
            .query_pairs_mut()
            .append_pair("channel", channel)
            .append_pair("message_ts", message_ts);
        let response = self
            .client
            .get(endpoint)
            .bearer_auth(bot_token)
            .send()
            .await
            .map_err(|_| SlackError::new("slack_proxy_unavailable"))?;
        let retry = retry_after(&response);
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(SlackError {
                code: "slack_rate_limited",
                retry_after: retry,
            });
        }
        if response.status().is_redirection() {
            return Err(SlackError::new("slack_redirect_rejected"));
        }
        if !response.status().is_success() {
            return Err(SlackError::new("slack_proxy_unavailable"));
        }
        let payload: PermalinkResponse = serde_json::from_slice(&bounded_body(response).await?)
            .map_err(|_| SlackError::new("slack_api_invalid_response"))?;
        if !payload.ok {
            return Err(classify_provider_error(payload.error.as_deref(), "proxy"));
        }
        let permalink = payload
            .permalink
            .ok_or_else(|| SlackError::new("slack_permalink_missing"))?;
        validate_permalink(&permalink, channel)?;
        Ok(permalink)
    }

    /// Validates a versioned JSON App Manifest using a short-lived configuration token.
    pub async fn validate_manifest(
        &self,
        configuration_token: &str,
        manifest: &serde_json::Value,
    ) -> std::result::Result<(), SlackError> {
        let response = self
            .client
            .post(self.endpoint("/api/apps.manifest.validate")?)
            .bearer_auth(configuration_token)
            .json(&serde_json::json!({"manifest": manifest.to_string()}))
            .send()
            .await
            .map_err(|_| SlackError::new("slack_manifest_unavailable"))?;
        parse_manifest_status(response).await.map(|_| ())
    }

    /// Creates the reviewed official app and returns credentials for direct encrypted storage.
    pub async fn provision_manifest(
        &self,
        configuration_token: &str,
        manifest: &serde_json::Value,
    ) -> std::result::Result<ManifestProvisionResult, SlackError> {
        let response = self
            .client
            .post(self.endpoint("/api/apps.manifest.create")?)
            .bearer_auth(configuration_token)
            .json(&serde_json::json!({"manifest": manifest.to_string()}))
            .send()
            .await
            .map_err(|_| SlackError::new("slack_manifest_unavailable"))?;
        let payload = parse_manifest_status(response).await?;
        let app_id = payload
            .app_id
            .filter(|value| valid_slack_id(value, 'A'))
            .ok_or_else(|| SlackError::new("slack_manifest_credentials_missing"))?;
        let credentials = payload
            .credentials
            .ok_or_else(|| SlackError::new("slack_manifest_credentials_missing"))?;
        let client_id = required_bounded(credentials.client_id)?;
        let client_secret = required_bounded(credentials.client_secret)?;
        let signing_secret = required_bounded(credentials.signing_secret)?;
        Ok(ManifestProvisionResult {
            app_id: app_id.clone(),
            credentials: OfficialAppCredentials {
                app_id,
                client_id,
                client_secret,
                signing_secret,
            },
        })
    }

    /// Exports the exact managed App before a governed update or deletion.
    pub async fn export_manifest(
        &self,
        configuration_token: &str,
        app_id: &str,
    ) -> std::result::Result<serde_json::Value, SlackError> {
        if !valid_slack_id(app_id, 'A') {
            return Err(SlackError::new("slack_manifest_app_identity_invalid"));
        }
        let response = self
            .client
            .post(self.endpoint("/api/apps.manifest.export")?)
            .bearer_auth(configuration_token)
            .json(&serde_json::json!({"app_id": app_id}))
            .send()
            .await
            .map_err(|_| SlackError::new("slack_manifest_unavailable"))?;
        parse_manifest_status(response)
            .await?
            .manifest
            .ok_or_else(|| SlackError::new("slack_manifest_invalid_response"))
    }

    /// Updates only the exact App ID whose current manifest was reviewed.
    pub async fn update_manifest(
        &self,
        configuration_token: &str,
        app_id: &str,
        manifest: &serde_json::Value,
    ) -> std::result::Result<bool, SlackError> {
        if !valid_slack_id(app_id, 'A') {
            return Err(SlackError::new("slack_manifest_app_identity_invalid"));
        }
        let response = self
            .client
            .post(self.endpoint("/api/apps.manifest.update")?)
            .bearer_auth(configuration_token)
            .json(&serde_json::json!({"app_id": app_id, "manifest": manifest.to_string()}))
            .send()
            .await
            .map_err(|_| SlackError::new("slack_manifest_unavailable"))?;
        Ok(parse_manifest_status(response)
            .await?
            .permissions_updated
            .unwrap_or(false))
    }

    /// Permanently deletes the exact manifest-created App after reviewed confirmation.
    pub async fn delete_manifest(
        &self,
        configuration_token: &str,
        app_id: &str,
    ) -> std::result::Result<(), SlackError> {
        if !valid_slack_id(app_id, 'A') {
            return Err(SlackError::new("slack_manifest_app_identity_invalid"));
        }
        let response = self
            .client
            .post(self.endpoint("/api/apps.manifest.delete")?)
            .bearer_auth(configuration_token)
            .json(&serde_json::json!({"app_id": app_id}))
            .send()
            .await
            .map_err(|_| SlackError::new("slack_manifest_unavailable"))?;
        parse_manifest_status(response).await.map(|_| ())
    }

    fn endpoint(&self, path: &str) -> std::result::Result<Url, SlackError> {
        self.base
            .join(path)
            .map_err(|_| SlackError::new("slack_endpoint_invalid"))
    }
}

/// Verifies Slack's HMAC signature against the raw, unparsed request body.
pub fn verify_request(
    signing_secret: &str,
    timestamp: &str,
    signature: &str,
    raw_body: &[u8],
    now: SystemTime,
) -> std::result::Result<(), SlackError> {
    let timestamp_seconds = timestamp
        .parse::<u64>()
        .map_err(|_| SlackError::new("slack_timestamp_invalid"))?;
    let now_seconds = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SlackError::new("clock_invalid"))?
        .as_secs();
    if now_seconds.abs_diff(timestamp_seconds) > 300 {
        return Err(SlackError::new("slack_timestamp_expired"));
    }
    let provided = signature
        .strip_prefix("v0=")
        .and_then(|value| hex::decode(value).ok())
        .ok_or_else(|| SlackError::new("slack_signature_invalid"))?;
    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes())
        .map_err(|_| SlackError::new("slack_signature_invalid"))?;
    mac.update(b"v0:");
    mac.update(timestamp.as_bytes());
    mac.update(b":");
    mac.update(raw_body);
    mac.verify_slice(&provided)
        .map_err(|_| SlackError::new("slack_signature_invalid"))
}

/// Parses only the Slack event kinds accepted by the managed adapter.
pub fn parse_event(raw_body: &[u8]) -> std::result::Result<SlackEvent, SlackError> {
    let envelope: EventEnvelope =
        serde_json::from_slice(raw_body).map_err(|_| SlackError::new("slack_event_invalid"))?;
    if envelope.kind == "url_verification" {
        let challenge = envelope
            .challenge
            .filter(|value| !value.is_empty() && value.len() <= 1024)
            .ok_or_else(|| SlackError::new("slack_challenge_invalid"))?;
        return Ok(SlackEvent::UrlVerification { challenge });
    }
    if envelope.kind != "event_callback" {
        return Err(SlackError::new("slack_event_unsupported"));
    }
    let event_id = envelope
        .event_id
        .filter(|value| valid_slack_id(value, 'E'))
        .ok_or_else(|| SlackError::new("slack_event_id_invalid"))?;
    let team_id = envelope
        .team_id
        .filter(|value| valid_slack_id(value, 'T'))
        .ok_or_else(|| SlackError::new("slack_team_invalid"))?;
    let app_id = envelope
        .api_app_id
        .filter(|value| valid_slack_id(value, 'A'))
        .ok_or_else(|| SlackError::new("slack_app_invalid"))?;
    let enterprise_id = envelope
        .enterprise_id
        .filter(|value| valid_slack_id(value, 'E'));
    let event = envelope
        .event
        .ok_or_else(|| SlackError::new("slack_event_missing"))?;
    match event.kind.as_str() {
        "reaction_added" => {
            let item = event
                .item
                .filter(|item| item.kind == "message")
                .ok_or_else(|| SlackError::new("slack_reaction_target_unsupported"))?;
            Ok(SlackEvent::ReactionAdded {
                app_id,
                event_id,
                team_id,
                enterprise_id,
                actor_id: event
                    .user
                    .filter(|value| valid_slack_id(value, 'U'))
                    .ok_or_else(|| SlackError::new("slack_actor_invalid"))?,
                reaction: event
                    .reaction
                    .filter(|value| valid_reaction(value))
                    .ok_or_else(|| SlackError::new("slack_reaction_invalid"))?,
                channel_id: item
                    .channel
                    .filter(|value| {
                        valid_slack_id(value, 'C')
                            || valid_slack_id(value, 'G')
                            || valid_slack_id(value, 'D')
                    })
                    .ok_or_else(|| SlackError::new("slack_channel_invalid"))?,
                message_ts: item
                    .ts
                    .filter(|value| valid_message_ts(value))
                    .ok_or_else(|| SlackError::new("slack_message_ts_invalid"))?,
                event_ts: event
                    .event_ts
                    .filter(|value| valid_message_ts(value))
                    .ok_or_else(|| SlackError::new("slack_event_ts_invalid"))?,
            })
        }
        "app_uninstalled" => Ok(SlackEvent::AppUninstalled {
            app_id,
            event_id,
            team_id,
            enterprise_id,
            event_ts: event_timestamp(event.event_ts, envelope.event_time)?,
        }),
        "tokens_revoked"
            if event
                .tokens
                .as_ref()
                .is_some_and(|tokens| !tokens.bot.is_empty()) =>
        {
            Ok(SlackEvent::TokensRevoked {
                app_id,
                event_id,
                team_id,
                enterprise_id,
                event_ts: event_timestamp(event.event_ts, envelope.event_time)?,
            })
        }
        _ => Err(SlackError::new("slack_event_unsupported")),
    }
}

fn event_timestamp(
    event_ts: Option<String>,
    event_time: Option<i64>,
) -> std::result::Result<String, SlackError> {
    if let Some(value) = event_ts.filter(|value| valid_message_ts(value)) {
        return Ok(value);
    }
    event_time
        .filter(|value| *value > 0)
        .map(|value| value.to_string())
        .ok_or_else(|| SlackError::new("slack_event_ts_invalid"))
}

async fn parse_manifest_status(
    response: reqwest::Response,
) -> std::result::Result<ManifestResponse, SlackError> {
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        return Err(SlackError {
            code: "slack_manifest_rate_limited",
            retry_after: retry_after(&response),
        });
    }
    if response.status().is_redirection() {
        return Err(SlackError::new("slack_redirect_rejected"));
    }
    if !response.status().is_success() {
        return Err(SlackError::new("slack_manifest_unavailable"));
    }
    let payload: ManifestResponse = serde_json::from_slice(&bounded_body(response).await?)
        .map_err(|_| SlackError::new("slack_manifest_invalid_response"))?;
    if !payload.ok {
        return Err(classify_provider_error(
            payload.error.as_deref(),
            "manifest",
        ));
    }
    Ok(payload)
}

async fn bounded_body(
    response: reqwest::Response,
) -> std::result::Result<bytes::Bytes, SlackError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_RESPONSE_BYTES as u64)
    {
        return Err(SlackError::new("slack_response_too_large"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| SlackError::new("slack_response_invalid"))?;
    if bytes.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(SlackError::new("slack_response_too_large"));
    }
    Ok(bytes)
}

fn classify_provider_error(error: Option<&str>, operation: &str) -> SlackError {
    match error {
        Some("invalid_auth" | "not_authed" | "token_revoked" | "account_inactive") => {
            SlackError::new("slack_credential_revoked")
        }
        Some("bad_redirect_uri") if operation == "oauth" => {
            SlackError::new("oauth_redirect_mismatch")
        }
        Some("invalid_scope" | "missing_scope" | "unapproved_scope") => {
            SlackError::new("oauth_scope_mismatch")
        }
        Some("access_denied") if operation == "oauth" => SlackError::new("oauth_denied"),
        Some("invalid_manifest") if operation == "manifest" => {
            SlackError::new("slack_manifest_invalid")
        }
        _ if operation == "manifest" => SlackError::new("slack_manifest_rejected"),
        _ if operation == "oauth" => SlackError::new("slack_oauth_rejected"),
        _ => SlackError::new("slack_api_rejected"),
    }
}

fn validate_permalink(value: &str, channel: &str) -> std::result::Result<(), SlackError> {
    if value.len() > 4096 {
        return Err(SlackError::new("slack_permalink_invalid"));
    }
    let url = Url::parse(value).map_err(|_| SlackError::new("slack_permalink_invalid"))?;
    if url.scheme() != "https" {
        return Err(SlackError::new("slack_permalink_invalid"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| SlackError::new("slack_permalink_invalid"))?;
    if host != "slack.com" && !host.ends_with(".slack.com") {
        return Err(SlackError::new("slack_permalink_host_rejected"));
    }
    let expected = format!("/archives/{channel}/");
    if !url.path().contains(&expected) {
        return Err(SlackError::new("slack_permalink_channel_mismatch"));
    }
    Ok(())
}

fn retry_after(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value <= 3600)
}

fn required_bounded(value: Option<String>) -> std::result::Result<String, SlackError> {
    value
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or_else(|| SlackError::new("slack_manifest_credentials_missing"))
}

fn valid_slack_id(value: &str, prefix: char) -> bool {
    value.len() >= 2
        && value.len() <= 64
        && value.starts_with(prefix)
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn valid_message_ts(value: &str) -> bool {
    let mut parts = value.split('.');
    let seconds = parts.next().unwrap_or_default();
    let micros = parts.next().unwrap_or_default();
    parts.next().is_none()
        && !seconds.is_empty()
        && !micros.is_empty()
        && seconds.len() <= 16
        && micros.len() <= 12
        && seconds.chars().all(|character| character.is_ascii_digit())
        && micros.chars().all(|character| character.is_ascii_digit())
}

fn valid_reaction(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '+')
        })
}

#[derive(Debug, Deserialize)]
struct OAuthResponse {
    ok: bool,
    error: Option<String>,
    access_token: Option<String>,
    scope: Option<String>,
    team: Option<IdentityObject>,
    enterprise: Option<IdentityObject>,
}

#[derive(Debug, Deserialize)]
struct IdentityObject {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PermalinkResponse {
    ok: bool,
    error: Option<String>,
    permalink: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    #[serde(rename = "type")]
    kind: String,
    challenge: Option<String>,
    team_id: Option<String>,
    enterprise_id: Option<String>,
    api_app_id: Option<String>,
    event_id: Option<String>,
    event_time: Option<i64>,
    event: Option<EventBody>,
}

#[derive(Debug, Deserialize)]
struct EventBody {
    #[serde(rename = "type")]
    kind: String,
    user: Option<String>,
    reaction: Option<String>,
    item: Option<EventItem>,
    event_ts: Option<String>,
    tokens: Option<RevokedTokens>,
}

#[derive(Debug, Deserialize)]
struct RevokedTokens {
    #[serde(default)]
    bot: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EventItem {
    #[serde(rename = "type")]
    kind: String,
    channel: Option<String>,
    ts: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestResponse {
    ok: bool,
    error: Option<String>,
    app_id: Option<String>,
    credentials: Option<ManifestCredentials>,
    manifest: Option<serde_json::Value>,
    permissions_updated: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ManifestCredentials {
    client_id: Option<String>,
    client_secret: Option<String>,
    signing_secret: Option<String>,
}

/// Stable manifest contract used by repository fixtures and operator checks.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewedManifestContract {
    /// OAuth bot scopes.
    pub bot_scopes: Vec<String>,
    /// Bot and app lifecycle event subscriptions.
    pub bot_events: Vec<String>,
    /// OAuth callback URL.
    pub redirect_url: String,
    /// Events API request URL.
    pub events_url: String,
}

/// Renders deployment-specific HTTPS endpoints into the reviewed manifest template.
///
/// Only endpoint fields are mutable at provisioning time. Scope and event drift is
/// still rejected by [`reviewed_manifest_contract`].
pub fn render_manifest_endpoints(
    manifest: &mut serde_json::Value,
    redirect_url: &str,
    events_url: &str,
) -> Result<()> {
    for (label, value) in [("OAuth callback", redirect_url), ("Events API", events_url)] {
        let parsed = Url::parse(value).with_context(|| format!("{label} URL is invalid"))?;
        if parsed.scheme() != "https" || parsed.host_str().is_none() {
            bail!("{label} URL must use public HTTPS");
        }
    }
    let redirect_urls = manifest
        .pointer_mut("/oauth_config/redirect_urls")
        .and_then(serde_json::Value::as_array_mut)
        .context("official manifest is missing redirect URLs")?;
    if redirect_urls.len() != 1 {
        bail!("official manifest must contain exactly one redirect URL");
    }
    redirect_urls[0] = serde_json::Value::String(redirect_url.to_string());
    let request_url = manifest
        .pointer_mut("/settings/event_subscriptions/request_url")
        .context("official manifest is missing Events API request URL")?;
    *request_url = serde_json::Value::String(events_url.to_string());
    Ok(())
}

/// Extracts and validates the security-sensitive portion of the official manifest.
pub fn reviewed_manifest_contract(
    manifest: &serde_json::Value,
) -> Result<ReviewedManifestContract> {
    let bot_scopes = string_array(manifest.pointer("/oauth_config/scopes/bot"), "bot scopes")?;
    let bot_events = string_array(
        manifest.pointer("/settings/event_subscriptions/bot_events"),
        "bot events",
    )?;
    let redirect_urls = string_array(
        manifest.pointer("/oauth_config/redirect_urls"),
        "redirect URLs",
    )?;
    if redirect_urls.len() != 1 {
        bail!("official manifest must contain exactly one redirect URL");
    }
    let events_url = manifest
        .pointer("/settings/event_subscriptions/request_url")
        .and_then(serde_json::Value::as_str)
        .context("official manifest is missing Events API request URL")?
        .to_string();
    let contract = ReviewedManifestContract {
        bot_scopes,
        bot_events,
        redirect_url: redirect_urls[0].clone(),
        events_url,
    };
    if contract.bot_scopes != ["reactions:read"] {
        bail!("official manifest bot scopes must be exactly reactions:read");
    }
    let mut bot_events = contract.bot_events.clone();
    bot_events.sort();
    if bot_events != ["app_uninstalled", "reaction_added", "tokens_revoked"] {
        bail!(
            "official manifest events must be app_uninstalled, reaction_added, and tokens_revoked"
        );
    }
    for value in [&contract.redirect_url, &contract.events_url] {
        let url = Url::parse(value).context("official manifest endpoint is invalid")?;
        if url.scheme() != "https" || url.host_str().is_none() {
            bail!("official manifest endpoints must use public HTTPS URLs");
        }
    }
    Ok(contract)
}

fn string_array(value: Option<&serde_json::Value>, label: &str) -> Result<Vec<String>> {
    value
        .and_then(serde_json::Value::as_array)
        .with_context(|| format!("official manifest is missing {label}"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .with_context(|| format!("official manifest {label} must contain strings"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, routing::post};
    use hmac::Mac;

    #[test]
    fn request_verification_uses_raw_body_and_rejects_replay() {
        let body = br#"{"type":"event_callback"}"#;
        let timestamp = "1700000000";
        let mut mac = HmacSha256::new_from_slice(b"signing-secret").expect("mac");
        mac.update(b"v0:1700000000:");
        mac.update(body);
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_100);
        assert!(verify_request("signing-secret", timestamp, &signature, body, now).is_ok());
        assert!(verify_request("signing-secret", timestamp, &signature, b"{}", now).is_err());
        assert!(
            verify_request(
                "signing-secret",
                timestamp,
                &signature,
                body,
                UNIX_EPOCH + Duration::from_secs(1_700_000_301),
            )
            .is_err()
        );
    }

    #[test]
    fn parser_accepts_only_allowlisted_message_reactions() {
        let event = parse_event(
            br#"{"type":"event_callback","team_id":"T123","api_app_id":"A123","event_id":"Ev123","event_time":1700000000,"event":{"type":"reaction_added","user":"U123","reaction":"agent-review","item":{"type":"message","channel":"C123","ts":"1700000000.000100"},"event_ts":"1700000001.000100"}}"#,
        )
        .expect("reaction");
        assert!(
            matches!(event, SlackEvent::ReactionAdded { reaction, .. } if reaction == "agent-review")
        );
        let unsupported = br#"{"type":"event_callback","team_id":"T123","api_app_id":"A123","event_id":"Ev123","event_time":1700000000,"event":{"type":"message","event_ts":"1700000001.000100"}}"#;
        assert_eq!(
            parse_event(unsupported).expect_err("unsupported").code(),
            "slack_event_unsupported"
        );
    }

    #[test]
    fn parser_accepts_only_bot_token_revocation() {
        let revoked = br#"{"type":"event_callback","team_id":"T123","api_app_id":"A123","event_id":"Ev123","event_time":1700000000,"event":{"type":"tokens_revoked","tokens":{"oauth":["xoxp-redacted"],"bot":["xoxb-redacted"]}}}"#;
        assert!(matches!(
            parse_event(revoked).expect("bot revocation"),
            SlackEvent::TokensRevoked { .. }
        ));
        let user_only = br#"{"type":"event_callback","team_id":"T123","api_app_id":"A123","event_id":"Ev123","event_time":1700000000,"event":{"type":"tokens_revoked","tokens":{"oauth":["xoxp-redacted"],"bot":[]}}}"#;
        assert_eq!(
            parse_event(user_only).expect_err("user token only").code(),
            "slack_event_unsupported"
        );
    }

    #[test]
    fn reviewed_manifest_contract_rejects_scope_or_endpoint_drift() {
        let mut manifest = serde_json::json!({
            "oauth_config": {
                "redirect_urls": ["https://gateway.example/slack/oauth/callback"],
                "scopes": {"bot": ["reactions:read"]}
            },
            "settings": {"event_subscriptions": {
                "request_url": "https://gateway.example/slack/events",
                "bot_events": ["reaction_added", "app_uninstalled", "tokens_revoked"]
            }}
        });
        assert!(reviewed_manifest_contract(&manifest).is_ok());
        manifest["oauth_config"]["scopes"]["bot"] =
            serde_json::json!(["reactions:read", "channels:history"]);
        assert!(reviewed_manifest_contract(&manifest).is_err());
    }

    #[test]
    fn manifest_template_renders_environment_endpoints_only() {
        let mut manifest = serde_json::json!({
            "oauth_config": {
                "redirect_urls": ["https://placeholder.example/slack/oauth/callback"],
                "scopes": {"bot": ["reactions:read"]}
            },
            "settings": {"event_subscriptions": {
                "request_url": "https://placeholder.example/slack/events",
                "bot_events": ["reaction_added", "app_uninstalled", "tokens_revoked"]
            }}
        });
        render_manifest_endpoints(
            &mut manifest,
            "https://gateway.example/slack/oauth/callback",
            "https://gateway.example/slack/events",
        )
        .expect("render endpoints");
        let contract = reviewed_manifest_contract(&manifest).expect("reviewed contract");
        assert_eq!(
            contract.redirect_url,
            "https://gateway.example/slack/oauth/callback"
        );
        assert_eq!(contract.events_url, "https://gateway.example/slack/events");
        assert!(
            render_manifest_endpoints(&mut manifest, "http://gateway", "https://ok.example")
                .is_err()
        );
    }

    #[test]
    fn permalink_validation_rejects_host_and_channel_confusion() {
        assert!(validate_permalink("https://acme.slack.com/archives/C123/p1", "C123").is_ok());
        assert_eq!(
            validate_permalink("https://evil.example/archives/C123/p1", "C123")
                .expect_err("host")
                .code(),
            "slack_permalink_host_rejected"
        );
        assert_eq!(
            validate_permalink("https://acme.slack.com/archives/C999/p1", "C123")
                .expect_err("channel")
                .code(),
            "slack_permalink_channel_mismatch"
        );
    }

    #[tokio::test]
    async fn manifest_client_covers_create_export_update_and_delete_contract() {
        async fn validate() -> Json<serde_json::Value> {
            Json(serde_json::json!({"ok": true}))
        }
        async fn create() -> Json<serde_json::Value> {
            Json(serde_json::json!({
                "ok": true,
                "app_id": "A12345678",
                "credentials": {
                    "client_id": "client-id",
                    "client_secret": "client-secret",
                    "signing_secret": "signing-secret"
                }
            }))
        }
        async fn export() -> Json<serde_json::Value> {
            Json(serde_json::json!({"ok": true, "manifest": {
                "display_information": {"name": "Orchestrator Dedicated"}
            }}))
        }
        async fn update() -> Json<serde_json::Value> {
            Json(serde_json::json!({"ok": true, "permissions_updated": true}))
        }
        let app = Router::new()
            .route("/api/apps.manifest.validate", post(validate))
            .route("/api/apps.manifest.create", post(create))
            .route("/api/apps.manifest.export", post(export))
            .route("/api/apps.manifest.update", post(update))
            .route("/api/apps.manifest.delete", post(validate));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake Slack");
        let origin = format!("http://{}", listener.local_addr().expect("local address"));
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve fake Slack") });
        let client = SlackClient::new(&origin, Duration::from_secs(2)).expect("client");
        let manifest = serde_json::json!({"display_information":{"name":"test"}});

        client
            .validate_manifest("xoxe.one-time", &manifest)
            .await
            .expect("validate");
        let created = client
            .provision_manifest("xoxe.one-time", &manifest)
            .await
            .expect("create");
        assert_eq!(created.app_id, "A12345678");
        assert!(!format!("{created:?}").contains("client-secret"));
        assert_eq!(
            client
                .export_manifest("xoxe.fresh", "A12345678")
                .await
                .expect("export")["display_information"]["name"],
            "Orchestrator Dedicated"
        );
        assert!(
            client
                .update_manifest("xoxe.fresh", "A12345678", &manifest)
                .await
                .expect("update requires OAuth")
        );
        client
            .delete_manifest("xoxe.fresh", "A12345678")
            .await
            .expect("delete");
        assert_eq!(
            client
                .delete_manifest("xoxe.fresh", "not-an-app")
                .await
                .expect_err("invalid exact App ID")
                .code(),
            "slack_manifest_app_identity_invalid"
        );
    }
}
