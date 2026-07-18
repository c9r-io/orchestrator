//! Slack integration gateway process and operator tooling.

#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use orchestrator_slack_gateway::config::GatewayConfig;
use orchestrator_slack_gateway::crypto::GatewayCrypto;
use orchestrator_slack_gateway::slack::{
    SlackClient, render_manifest_endpoints, reviewed_manifest_contract,
};
use orchestrator_slack_gateway::store::GatewayStore;
use orchestrator_slack_gateway::{GatewayState, router};

#[derive(Debug, Parser)]
#[command(name = "orchestrator-slack-gateway", version)]
struct Args {
    #[command(flatten)]
    config: GatewayConfig,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate or provision the official Slack App Manifest.
    Manifest {
        #[command(subcommand)]
        command: ManifestCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ManifestCommand {
    /// Validate the reviewed manifest locally and through Slack.
    Validate {
        /// Versioned JSON manifest path.
        #[arg(long)]
        manifest: PathBuf,
        /// Read the short-lived Slack Configuration Token from stdin.
        #[arg(long)]
        config_token_stdin: bool,
    },
    /// Create the official Slack App and store returned credentials encrypted.
    Provision {
        /// Versioned JSON manifest path.
        #[arg(long)]
        manifest: PathBuf,
        /// Read the short-lived Slack Configuration Token from stdin.
        #[arg(long)]
        config_token_stdin: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    args.config.validate()?;
    init_tracing()?;
    let crypto = GatewayCrypto::from_base64(&args.config.master_key)?;
    let store = GatewayStore::open(&args.config.database, crypto)?;
    let slack = SlackClient::new(&args.config.slack_api_base, args.config.provider_timeout())?;

    if let Some(command) = args.command {
        return handle_command(command, &args.config, &store, &slack).await;
    }

    let bind = args.config.bind;
    let state = GatewayState::new(args.config, store, slack);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .context("failed to bind Slack gateway")?;
    tracing::info!(%bind, "Slack integration gateway ready");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Slack gateway server failed")
}

async fn handle_command(
    command: Command,
    config: &GatewayConfig,
    store: &GatewayStore,
    slack: &SlackClient,
) -> Result<()> {
    match command {
        Command::Manifest { command } => {
            let (manifest_path, config_token_stdin, provision) = match command {
                ManifestCommand::Validate {
                    manifest,
                    config_token_stdin,
                } => (manifest, config_token_stdin, false),
                ManifestCommand::Provision {
                    manifest,
                    config_token_stdin,
                } => (manifest, config_token_stdin, true),
            };
            if !config_token_stdin {
                bail!("--config-token-stdin is required; tokens are never accepted in argv");
            }
            let manifest_bytes = std::fs::read(&manifest_path)
                .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
            let mut manifest: serde_json::Value =
                serde_json::from_slice(&manifest_bytes).context("manifest must be valid JSON")?;
            render_manifest_endpoints(
                &mut manifest,
                &config.oauth_callback_url()?,
                &config.events_url()?,
            )?;
            let contract = reviewed_manifest_contract(&manifest)?;
            debug_assert_eq!(contract.redirect_url, config.oauth_callback_url()?);
            debug_assert_eq!(contract.events_url, config.events_url()?);
            let token = read_secret_stdin()?;
            slack
                .validate_manifest(&token, &manifest)
                .await
                .map_err(anyhow::Error::new)?;
            if provision {
                let result = slack
                    .provision_manifest(&token, &manifest)
                    .await
                    .map_err(anyhow::Error::new)?;
                store.put_official_app_credentials(&result.credentials)?;
                println!(
                    "{}",
                    serde_json::json!({"status":"provisioned","app_id":result.app_id})
                );
            } else {
                println!("{}", serde_json::json!({"status":"valid"}));
            }
        }
    }
    Ok(())
}

fn read_secret_stdin() -> Result<String> {
    let mut value = String::new();
    std::io::stdin()
        .take(8193)
        .read_to_string(&mut value)
        .context("failed to read Configuration Token from stdin")?;
    let value = value.trim().to_string();
    if value.is_empty() || value.len() > 8192 {
        bail!("Configuration Token stdin must contain 1-8192 characters");
    }
    Ok(value)
}

fn init_tracing() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_target(false)
        .with_env_filter(filter)
        .try_init()
        .map_err(|_| anyhow::anyhow!("failed to initialize gateway logging"))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
