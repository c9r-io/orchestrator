//! Bounded Slack Web API adapter used by source automation routing.

use futures::StreamExt;
use reqwest::header::{AUTHORIZATION, RETRY_AFTER};
use serde::Deserialize;
use std::fmt;
#[cfg(any(debug_assertions, feature = "dev-insecure"))]
use std::net::IpAddr;
use std::time::Duration;

const OFFICIAL_API_BASE: &str = "https://slack.com/api/";
const MAX_RESPONSE_BYTES: usize = 32 * 1024;
const MAX_RETRY_AFTER_SECS: u64 = 300;

#[cfg(test)]
static TEST_API_BASE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn set_test_api_base(value: Option<String>) {
    *TEST_API_BASE.lock().expect("test Slack API base lock") = value;
}

/// Successfully validated Slack permalink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackPermalink {
    /// Canonical HTTPS URL returned by Slack.
    pub url: String,
}

/// Stable provider failure returned to the durable router.
#[derive(Debug)]
pub struct SlackApiError {
    code: &'static str,
    retry_after: Option<Duration>,
    transient: bool,
}

impl SlackApiError {
    fn permanent(code: &'static str) -> Self {
        Self {
            code,
            retry_after: None,
            transient: false,
        }
    }

    fn transient(code: &'static str, retry_after: Option<Duration>) -> Self {
        Self {
            code,
            retry_after,
            transient: true,
        }
    }

    /// Stable, non-sensitive failure code suitable for persistence.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Whether a later bounded retry may succeed.
    pub fn is_transient(&self) -> bool {
        self.transient
    }

    /// Provider-requested retry delay, capped by the adapter.
    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }
}

impl fmt::Display for SlackApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for SlackApiError {}

/// Slack provider client with strict redirect, timeout, and response bounds.
#[derive(Clone)]
pub struct SlackApiClient {
    client: reqwest::Client,
    api_base: reqwest::Url,
}

impl SlackApiClient {
    /// Builds the production client. A loopback override is accepted only in
    /// debug builds or when the explicit `dev-insecure` feature is enabled.
    pub fn new() -> Result<Self, SlackApiError> {
        let api_base = api_base_url()?;
        Self::build(api_base, Duration::from_secs(8))
    }

    fn build(api_base: reqwest::Url, timeout: Duration) -> Result<Self, SlackApiError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(api_base.scheme() == "https")
            .build()
            .map_err(|_| SlackApiError::permanent("slack_client_configuration_invalid"))?;
        Ok(Self { client, api_base })
    }

    #[cfg(test)]
    fn for_test(api_base: &str, timeout: Duration) -> Self {
        Self::build(reqwest::Url::parse(api_base).expect("test URL"), timeout)
            .expect("test Slack client")
    }

    /// Resolves and validates a message permalink without exposing the token.
    pub async fn get_permalink(
        &self,
        token: &str,
        channel: &str,
        message_ts: &str,
    ) -> Result<SlackPermalink, SlackApiError> {
        if token.trim().is_empty() {
            return Err(SlackApiError::permanent("slack_credential_missing"));
        }
        if channel.trim().is_empty() || message_ts.trim().is_empty() {
            return Err(SlackApiError::permanent("slack_message_identity_invalid"));
        }
        let endpoint = self
            .api_base
            .join("chat.getPermalink")
            .map_err(|_| SlackApiError::permanent("slack_client_configuration_invalid"))?;
        let response = self
            .client
            .get(endpoint)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .query(&[("channel", channel), ("message_ts", message_ts)])
            .send()
            .await
            .map_err(classify_transport_error)?;

        if response.status().is_redirection() {
            return Err(SlackApiError::permanent("slack_redirect_rejected"));
        }
        if response.status().as_u16() == 429 {
            let retry_after = parse_retry_after(response.headers().get(RETRY_AFTER));
            return Err(SlackApiError::transient("slack_rate_limited", retry_after));
        }
        if !response.status().is_success() {
            let transient = response.status().is_server_error();
            return Err(if transient {
                SlackApiError::transient("slack_http_unavailable", None)
            } else {
                SlackApiError::permanent("slack_http_rejected")
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(SlackApiError::permanent("slack_response_too_large"));
        }
        let body = read_bounded_body(response).await?;
        let payload: GetPermalinkResponse = serde_json::from_slice(&body)
            .map_err(|_| SlackApiError::permanent("slack_response_invalid_json"))?;
        if !payload.ok {
            return Err(classify_slack_error(payload.error.as_deref()));
        }
        let permalink = payload
            .permalink
            .ok_or_else(|| SlackApiError::permanent("slack_permalink_missing"))?;
        validate_permalink(&permalink, channel)?;
        Ok(SlackPermalink { url: permalink })
    }
}

#[derive(Debug, Deserialize)]
struct GetPermalinkResponse {
    ok: bool,
    #[serde(default)]
    permalink: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

async fn read_bounded_body(response: reqwest::Response) -> Result<Vec<u8>, SlackApiError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_transport_error)?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(SlackApiError::permanent("slack_response_too_large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_transport_error(error: reqwest::Error) -> SlackApiError {
    if error.is_timeout() {
        SlackApiError::transient("slack_request_timeout", None)
    } else {
        SlackApiError::transient("slack_transport_unavailable", None)
    }
}

fn classify_slack_error(error: Option<&str>) -> SlackApiError {
    match error {
        Some("ratelimited") => SlackApiError::transient("slack_rate_limited", None),
        Some("internal_error") | Some("fatal_error") => {
            SlackApiError::transient("slack_api_unavailable", None)
        }
        Some("invalid_auth") | Some("not_authed") | Some("account_inactive") => {
            SlackApiError::permanent("slack_credential_rejected")
        }
        Some("message_not_found") => SlackApiError::permanent("slack_message_not_found"),
        Some("channel_not_found")
        | Some("missing_scope")
        | Some("not_in_channel")
        | Some("access_denied")
        | Some("restricted_action") => SlackApiError::permanent("slack_message_forbidden"),
        _ => SlackApiError::permanent("slack_api_rejected"),
    }
}

fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<Duration> {
    let seconds = value?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()?
        .min(MAX_RETRY_AFTER_SECS);
    Some(Duration::from_secs(seconds))
}

fn validate_permalink(value: &str, expected_channel: &str) -> Result<(), SlackApiError> {
    if value.len() > 2048 {
        return Err(SlackApiError::permanent("slack_permalink_invalid"));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| SlackApiError::permanent("slack_permalink_invalid"))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(SlackApiError::permanent("slack_permalink_invalid"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| SlackApiError::permanent("slack_permalink_invalid"))?;
    if host != "slack.com" && !host.ends_with(".slack.com") {
        return Err(SlackApiError::permanent("slack_permalink_host_rejected"));
    }
    let mut segments = url
        .path_segments()
        .ok_or_else(|| SlackApiError::permanent("slack_permalink_invalid"))?;
    if segments.next() != Some("archives") || segments.next() != Some(expected_channel) {
        return Err(SlackApiError::permanent("slack_permalink_channel_mismatch"));
    }
    Ok(())
}

fn api_base_url() -> Result<reqwest::Url, SlackApiError> {
    #[cfg(test)]
    if let Some(value) = TEST_API_BASE
        .lock()
        .expect("test Slack API base lock")
        .clone()
    {
        return reqwest::Url::parse(&value)
            .map_err(|_| SlackApiError::permanent("slack_client_configuration_invalid"));
    }
    let official = reqwest::Url::parse(OFFICIAL_API_BASE)
        .map_err(|_| SlackApiError::permanent("slack_client_configuration_invalid"))?;
    let Ok(value) = std::env::var("ORCHESTRATOR_SLACK_API_BASE_URL") else {
        return Ok(official);
    };
    #[cfg(any(debug_assertions, feature = "dev-insecure"))]
    {
        let url = reqwest::Url::parse(&value)
            .map_err(|_| SlackApiError::permanent("slack_client_configuration_invalid"))?;
        let host = url
            .host_str()
            .ok_or_else(|| SlackApiError::permanent("slack_client_configuration_invalid"))?;
        let loopback = host == "localhost"
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !loopback || url.scheme() != "http" {
            return Err(SlackApiError::permanent("slack_api_base_override_rejected"));
        }
        Ok(url)
    }
    #[cfg(not(any(debug_assertions, feature = "dev-insecure")))]
    {
        let _ = value;
        Err(SlackApiError::permanent("slack_api_base_override_rejected"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::Query;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Redirect;
    use axum::routing::get;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct QueryParams {
        channel: String,
        message_ts: String,
    }

    async fn spawn(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake Slack");
        let address = listener.local_addr().expect("fake Slack address");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("fake Slack server");
        });
        format!("http://{address}/api/")
    }

    #[test]
    fn validates_expected_slack_permalink() {
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

    #[test]
    fn retry_after_is_bounded() {
        let value = reqwest::header::HeaderValue::from_static("9999");
        assert_eq!(
            parse_retry_after(Some(&value)),
            Some(Duration::from_secs(MAX_RETRY_AFTER_SECS))
        );
    }

    #[test]
    fn classifies_visibility_and_credential_failures_as_stable_operator_codes() {
        for provider_code in [
            "channel_not_found",
            "missing_scope",
            "not_in_channel",
            "access_denied",
            "restricted_action",
        ] {
            let error = classify_slack_error(Some(provider_code));
            assert_eq!(error.code(), "slack_message_forbidden");
            assert!(!error.is_transient());
        }
        assert_eq!(
            classify_slack_error(Some("message_not_found")).code(),
            "slack_message_not_found"
        );
        assert_eq!(
            classify_slack_error(Some("invalid_auth")).code(),
            "slack_credential_rejected"
        );
    }

    #[tokio::test]
    async fn resolves_permalink_with_bearer_and_message_coordinates() {
        let base = spawn(Router::new().route(
            "/api/chat.getPermalink",
            get(
                |headers: HeaderMap, Query(query): Query<QueryParams>| async move {
                    assert_eq!(
                        headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()),
                        Some("Bearer xoxb-test")
                    );
                    assert_eq!(query.channel, "C123");
                    assert_eq!(query.message_ts, "171234.000100");
                    axum::Json(serde_json::json!({
                        "ok": true,
                        "permalink": "https://acme.slack.com/archives/C123/p171234000100"
                    }))
                },
            ),
        ))
        .await;
        let result = SlackApiClient::for_test(&base, Duration::from_secs(1))
            .get_permalink("xoxb-test", "C123", "171234.000100")
            .await
            .expect("permalink");
        assert_eq!(
            result.url,
            "https://acme.slack.com/archives/C123/p171234000100"
        );
    }

    #[tokio::test]
    async fn rejects_invalid_json_and_redirects() {
        let invalid =
            spawn(Router::new().route("/api/chat.getPermalink", get(|| async { "not-json" })))
                .await;
        let error = SlackApiClient::for_test(&invalid, Duration::from_secs(1))
            .get_permalink("token", "C123", "1.0")
            .await
            .expect_err("invalid JSON");
        assert_eq!(error.code(), "slack_response_invalid_json");

        let redirect = spawn(Router::new().route(
            "/api/chat.getPermalink",
            get(|| async { Redirect::temporary("https://evil.example/") }),
        ))
        .await;
        let error = SlackApiClient::for_test(&redirect, Duration::from_secs(1))
            .get_permalink("token", "C123", "1.0")
            .await
            .expect_err("redirect");
        assert_eq!(error.code(), "slack_redirect_rejected");
    }

    #[tokio::test]
    async fn classifies_rate_limit_and_provider_error_without_body_leakage() {
        let limited = spawn(Router::new().route(
            "/api/chat.getPermalink",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, [(RETRY_AFTER, "9999")]) }),
        ))
        .await;
        let error = SlackApiClient::for_test(&limited, Duration::from_secs(1))
            .get_permalink("token", "C123", "1.0")
            .await
            .expect_err("rate limit");
        assert_eq!(error.code(), "slack_rate_limited");
        assert_eq!(error.retry_after(), Some(Duration::from_secs(300)));

        let rejected = spawn(Router::new().route(
            "/api/chat.getPermalink",
            get(|| async { axum::Json(serde_json::json!({"ok": false, "error": "invalid_auth"})) }),
        ))
        .await;
        let error = SlackApiClient::for_test(&rejected, Duration::from_secs(1))
            .get_permalink("super-secret-token", "C123", "1.0")
            .await
            .expect_err("provider rejection");
        assert_eq!(error.code(), "slack_credential_rejected");
        assert!(!error.to_string().contains("super-secret-token"));
    }

    #[tokio::test]
    async fn timeout_is_bounded_and_transient() {
        let base = spawn(Router::new().route(
            "/api/chat.getPermalink",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                axum::Json(serde_json::json!({"ok": true}))
            }),
        ))
        .await;
        let error = SlackApiClient::for_test(&base, Duration::from_millis(20))
            .get_permalink("token", "C123", "1.0")
            .await
            .expect_err("timeout");
        assert_eq!(error.code(), "slack_request_timeout");
        assert!(error.is_transient());
    }
}
