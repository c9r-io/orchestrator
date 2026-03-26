//! Task scheduler engine for the Agent Orchestrator.
//!
//! This crate contains the task scheduling core extracted from the
//! `agent-orchestrator` core crate.  Runner and prehook modules remain in
//! core **by design** — they are cross-cutting infrastructure used by
//! `config_load/validate`, `dynamic_orchestration`, `trigger_engine`, and
//! `output_capture`, not scheduler-specific concerns.  See Design Doc 60
//! (`docs/design_doc/orchestrator/60-core-crate-split-scheduler.md`) for the
//! full rationale.
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
