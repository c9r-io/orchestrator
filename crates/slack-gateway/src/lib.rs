//! Managed Slack integration gateway.
//!
//! The gateway terminates Slack OAuth and signed Events API requests, owns
//! installation credentials, and exposes installation-scoped delivery and
//! provider proxy APIs to local orchestrator daemons. It never performs task
//! routing or task mutation.

#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

/// HTTP API and request policy.
pub mod api;
/// Gateway configuration.
pub mod config;
/// Credential encryption and stable identity digests.
pub mod crypto;
/// Provider-safe domain contracts.
pub mod domain;
/// Slack OAuth, Events API, and Web API client boundary.
pub mod slack;
/// Gateway persistence and forward-only migrations.
pub mod store;

pub use api::{GatewayState, router};
pub use config::GatewayConfig;
