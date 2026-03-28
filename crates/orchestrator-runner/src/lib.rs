//! Command runner, sandbox, output capture, and network allowlist.
//!
//! This crate provides the execution engine used by the agent orchestrator
//! for spawning commands inside optional sandbox profiles, capturing and
//! sanitizing output streams, and validating network allowlists.

#![deny(missing_docs)]

/// Command runner abstractions, policies, and spawn helpers.
pub mod runner;
/// Output capture utilities for spawned commands.
pub mod output_capture;
/// Sandbox network allowlist parsing and validation.
pub mod sandbox_network;
