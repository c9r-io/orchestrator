//! Scheduler engine for the Agent Orchestrator.
//!
//! This crate contains the task scheduling core extracted from the
//! `agent-orchestrator` core crate. Runner, prehook, and shared type modules
//! remain in core and are accessed via `agent_orchestrator::`.
#![cfg_attr(
    not(any(test, feature = "test-harness")),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

/// Task scheduling, guard evaluation, and trace rendering.
pub mod scheduler;
/// Service-layer handlers for task operations.
pub mod service;
