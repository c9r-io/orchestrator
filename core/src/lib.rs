//! Public API surface for the agent orchestrator core crate.
//!
//! This crate exposes orchestration models, configuration loading, scheduling,
//! persistence helpers, and service-facing data transfer types used by the CLI
//! and daemon crates.
//!
//! # Examples
//!
//! ```rust
//! use agent_orchestrator::config::WorkflowLoopGuardConfig;
//!
//! let guard = WorkflowLoopGuardConfig::default();
//! assert!(guard.stop_when_no_unresolved);
//! ```
#![cfg_attr(
    not(any(test, feature = "test-harness")),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod agent_lifecycle;
/// Anomaly classification types for scheduler traces and runtime diagnostics.
pub mod anomaly;
/// Async SQLite access helpers backed by `tokio_rusqlite`.
pub mod async_database;
/// K8s-style declarative resource types shared by the CLI surface.
pub use orchestrator_config::cli_types;
pub mod collab;
/// Extension trait adding CRD-projected accessors to `OrchestratorConfig`.
pub mod config_ext;
pub use orchestrator_config::config;
/// Configuration loading, overlaying, and validation helpers.
pub mod config_load;
/// Custom resource definitions and resource store projections.
pub mod crd;
/// SQLite schema bootstrap and connection setup helpers.
pub mod db;
/// Database maintenance utilities: VACUUM and size reporting.
pub mod db_maintenance;
/// Serialized database write coordination for async callers.
pub mod db_write;
/// Data transfer objects returned by public task and event APIs.
pub mod dto;
pub mod dynamic_orchestration;
/// Environment resolution utilities for command execution.
pub use orchestrator_config::env_resolve;
/// Canonical error categories and error classification helpers.
pub mod error;
/// TTL-based event cleanup, archival, and statistics.
pub mod event_cleanup;
/// Event sink types and event-query helpers.
pub mod events;
/// Backfill helpers for reconstructing missing event streams.
pub mod events_backfill;
/// Health check models and endpoint support code.
pub mod health;
/// JSON extraction helpers used by dynamic orchestration and templating.
pub mod json_extract;
/// TTL-based log file cleanup for terminated tasks.
pub mod log_cleanup;
pub mod metrics;
/// Legacy migration entry points preserved for compatibility.
pub mod migration;
/// Logging and metrics bootstrap helpers for runtime observability.
pub mod observability;
/// Output capture utilities for spawned commands.
pub mod output_capture;
/// Structured output validation and diagnostics.
pub mod output_validation;
/// Persistence repositories and migration models.
pub mod persistence;
/// Prehook execution models and support helpers.
pub mod prehook;
/// QA document parsing and validation utilities.
pub mod qa_utils;
/// Declarative resource CRUD support and manifest rendering.
pub mod resource;
/// Command runner abstractions, policies, and spawn helpers.
pub mod runner;
/// Daemon lifecycle state and runtime snapshots.
pub mod runtime;
/// Sandbox network allowlist parsing and validation.
pub mod sandbox_network;
/// High-level scheduler service orchestration entry points.
pub mod scheduler_service;
/// Secret key audit reports and validation routines.
pub mod secret_key_audit;
/// Secret key rotation lifecycle primitives.
pub mod secret_key_lifecycle;
/// Secret-store encryption and decryption helpers.
pub mod secret_store_crypto;
/// Secure file and directory creation helpers.
pub mod secure_files;
/// Agent selection algorithms and resolution helpers.
pub mod selection;
/// Self-referential workspace safety policies.
pub mod self_referential_policy;
/// Service-layer handlers used by the daemon.
pub mod service;
/// Session persistence models and repository helpers.
pub mod session_store;
/// Shared daemon state and state transition helpers.
pub mod state;
pub mod store;
/// Auto-cleanup of terminated tasks and associated data.
pub mod task_cleanup;
/// High-level task mutation operations.
pub mod task_ops;
/// Task repository interfaces and SQLite implementations.
pub mod task_repository;
/// Ticket discovery, preview, and creation helpers.
pub mod ticket;
/// Trigger engine: cron scheduler and event-driven task creation.
pub mod trigger_engine;

/// Test utilities and fixtures for building isolated orchestrator state.
#[cfg(any(test, feature = "test-harness"))]
pub mod test_utils;

/// Re-export of the public workflow loop guard configuration type.
pub use config::WorkflowLoopGuardConfig;
