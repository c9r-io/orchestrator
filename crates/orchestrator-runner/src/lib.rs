//! Command runner, sandbox, output capture, and network allowlist.
//!
//! This crate provides the execution engine used by the agent orchestrator
//! for spawning commands inside optional sandbox profiles, capturing and
//! sanitizing output streams, and validating network allowlists.

#![cfg_attr(not(test), deny(clippy::panic, clippy::unwrap_used, clippy::expect_used))]
#![deny(missing_docs)]

/// Command runner abstractions, policies, and spawn helpers.
pub mod runner;
/// Output capture utilities for spawned commands.
pub mod output_capture;
/// Sandbox network allowlist parsing and validation.
pub mod sandbox_network;
