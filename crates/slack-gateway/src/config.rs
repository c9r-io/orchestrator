//! Process-wide gateway configuration.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use url::Url;

/// Runtime arguments for the Slack gateway.
#[derive(Debug, Clone, Args)]
pub struct GatewayConfig {
    /// Public HTTP bind address. Production deployments must terminate TLS
    /// before this listener or bind it directly behind a trusted proxy.
    #[arg(long, env = "SLACK_GATEWAY_BIND", default_value = "127.0.0.1:19440")]
    pub bind: SocketAddr,

    /// Public HTTPS origin used for OAuth callback and Events API URLs.
    #[arg(long, env = "SLACK_GATEWAY_PUBLIC_URL")]
    pub public_url: String,

    /// Gateway-owned SQLite database path.
    #[arg(long, env = "SLACK_GATEWAY_DATABASE")]
    pub database: PathBuf,

    /// Base64-encoded 32-byte gateway master key. This is supplied by the
    /// deployment secret backend and never persisted in SQLite.
    #[arg(long, env = "SLACK_GATEWAY_MASTER_KEY", hide_env_values = true)]
    pub master_key: String,

    /// Pre-install daemon enrollment credential. It can create OAuth intents
    /// but cannot access any installed workspace or provider proxy.
    #[arg(long, env = "SLACK_GATEWAY_ENROLLMENT_KEY", hide_env_values = true)]
    pub enrollment_key: String,

    /// Slack OAuth and Web API origin. Overrides are accepted only for
    /// loopback test servers.
    #[arg(
        long,
        env = "SLACK_GATEWAY_SLACK_API_BASE",
        default_value = "https://slack.com"
    )]
    pub slack_api_base: String,

    /// OAuth intent lifetime in seconds.
    #[arg(long, default_value_t = 600)]
    pub intent_ttl_secs: u64,

    /// Maximum delivery lease duration in seconds.
    #[arg(long, default_value_t = 60)]
    pub max_lease_secs: u64,

    /// HTTP timeout for Slack provider calls in seconds.
    #[arg(long, default_value_t = 10)]
    pub provider_timeout_secs: u64,
}

impl GatewayConfig {
    /// Validates public origins, test overrides, and bounded timeouts.
    pub fn validate(&self) -> Result<()> {
        let public = Url::parse(&self.public_url).context("invalid gateway public URL")?;
        if public.scheme() != "https" && !is_loopback_url(&public) {
            bail!("gateway public URL must use https outside loopback tests");
        }
        if public.host_str().is_none() || public.query().is_some() || public.fragment().is_some() {
            bail!("gateway public URL must be an origin without query or fragment");
        }

        let slack = Url::parse(&self.slack_api_base).context("invalid Slack API base URL")?;
        let official = slack.scheme() == "https" && slack.host_str() == Some("slack.com");
        if !official && !is_loopback_url(&slack) {
            bail!("Slack API base override is restricted to loopback tests");
        }
        if !(60..=900).contains(&self.intent_ttl_secs) {
            bail!("intent TTL must be between 60 and 900 seconds");
        }
        if !(1..=300).contains(&self.max_lease_secs) {
            bail!("maximum lease must be between 1 and 300 seconds");
        }
        if !(1..=30).contains(&self.provider_timeout_secs) {
            bail!("provider timeout must be between 1 and 30 seconds");
        }
        if self.enrollment_key.len() < 32 {
            bail!("gateway enrollment key must contain at least 32 bytes");
        }
        Ok(())
    }

    /// OAuth callback URL derived from the reviewed public origin.
    pub fn oauth_callback_url(&self) -> Result<String> {
        join_public(&self.public_url, "/slack/oauth/callback")
    }

    /// Slack Events API URL derived from the reviewed public origin.
    pub fn events_url(&self) -> Result<String> {
        join_public(&self.public_url, "/slack/events")
    }

    /// Provider request timeout.
    pub fn provider_timeout(&self) -> Duration {
        Duration::from_secs(self.provider_timeout_secs)
    }
}

fn join_public(base: &str, path: &str) -> Result<String> {
    let mut url = Url::parse(base).context("invalid gateway public URL")?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

fn is_loopback_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(public_url: &str, slack_api_base: &str) -> GatewayConfig {
        GatewayConfig {
            bind: "127.0.0.1:19440".parse().expect("bind"),
            public_url: public_url.into(),
            database: PathBuf::from("gateway.db"),
            master_key: "unused".into(),
            enrollment_key: "0123456789abcdef0123456789abcdef".into(),
            slack_api_base: slack_api_base.into(),
            intent_ttl_secs: 600,
            max_lease_secs: 60,
            provider_timeout_secs: 10,
        }
    }

    #[test]
    fn production_origins_require_https_and_official_slack_host() {
        assert!(
            config("https://gateway.example", "https://slack.com")
                .validate()
                .is_ok()
        );
        assert!(
            config("http://gateway.example", "https://slack.com")
                .validate()
                .is_err()
        );
        assert!(
            config("https://gateway.example", "https://evil.example")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn loopback_test_origins_are_explicitly_supported() {
        assert!(
            config("http://127.0.0.1:19440", "http://127.0.0.1:19441")
                .validate()
                .is_ok()
        );
    }
}
