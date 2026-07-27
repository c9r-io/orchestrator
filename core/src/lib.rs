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

/// Canonical control-plane mutation audit envelope and query repository.
pub mod action_audit;
pub mod agent_lifecycle;
/// Anomaly classification types for scheduler traces and runtime diagnostics.
pub mod anomaly;
/// Re-export of the writer/reader `AsyncDatabase` connection pair.
///
/// The pair itself lives in `orchestrator-persistence` (FR-130 Phase A). This
/// module is the re-export plus the tests that drive it through core's schema
/// bootstrap.
pub mod async_database;
/// Persistent human-attention queue models and repository operations.
pub mod attention;
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
/// Re-export of the admin facade, plus the two entry points that take daemon state.
///
/// Project-scoped task queries, audit-record insertion, execution-metrics
/// sampling and database reset live in `orchestrator-persistence` (FR-130
/// Phase A). What stays here is `reset_db` and `reset_project_data`, which take
/// `&state::InnerState` and delegate to the `_by_path` forms below that layer.
pub mod db;
/// Database maintenance utilities: VACUUM and size reporting.
/// Backlinks from an audited row to the request that produced it.
pub use orchestrator_persistence::audit_links;
pub use orchestrator_persistence::db_maintenance;
/// The handoff and resume tables and the statements over them.
pub use orchestrator_persistence::handoff_store;
/// Serialized database write coordination for async callers (**async write layer**).
///
/// Wraps `AsyncSqliteTaskRepository` behind a `DbWriteCoordinator` that
/// serializes event insertion, command-run updates, and phase-result
/// persistence through the single-writer connection.
pub mod db_write;
/// Data transfer objects returned by public task and event APIs.
///
/// They are the row and read-model shapes the repositories produce, so they
/// moved with the repositories (FR-130 Phase A) and are re-exported here.
pub use orchestrator_persistence::dto;
/// The reads and writes the scheduler makes about task and item state.
pub use orchestrator_persistence::scheduler_state;
/// The `session_control_actions` table: idempotency envelopes for session control.
pub use orchestrator_persistence::session_control_audit;
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
/// Immutable handoff snapshots and safe logical resume planning.
pub mod handoff;
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
/// Provider-neutral agent driver contracts and registry.
pub use orchestrator_runner::driver;
/// Output capture utilities for spawned commands.
pub use orchestrator_runner::output_capture;
/// Structured output validation and diagnostics.
pub mod output_validation;
/// Persistence infrastructure, re-exported from `orchestrator-persistence`.
///
/// Connection management, schema migrations, and the session, scheduler,
/// workflow-store and daemon-metadata repositories all moved below core in
/// FR-130 Phase A. `ConfigRepository` is the one that stayed: it reads `crd`,
/// which calls back into `db`, so sinking it would close a cycle. It moves in
/// Phase B with `crd`.
pub mod persistence;
/// Prehook execution models and support helpers.
pub mod prehook;
/// Local, privacy-safe Process Console operational metrics.
pub mod process_metrics;
/// QA doctor observability queries for `task_execution_metrics`.
pub mod qa_doctor;
/// QA document parsing and validation utilities.
pub mod qa_utils;
/// Declarative resource CRUD support and manifest rendering.
pub mod resource;
/// Command runner abstractions, policies, and spawn helpers.
pub use orchestrator_runner::runner;
/// Daemon lifecycle state and runtime snapshots.
pub mod runtime;
/// Sandbox network allowlist parsing and validation.
pub use orchestrator_runner::sandbox_network;
/// Scheduler port: [`TaskEnqueuer`](scheduler_port::TaskEnqueuer) trait for
/// cross-crate task enqueue dispatch (see module docs).
pub mod scheduler_port;
/// Secret key audit reports and validation routines.
pub use orchestrator_security::secret_key_audit;
/// Secret key rotation lifecycle primitives.
pub use orchestrator_security::secret_key_lifecycle;
/// Secret-store encryption and decryption helpers.
pub use orchestrator_security::secret_store_crypto;
/// Opaque handle to the SecretStore tables; the way core and the daemon reach them since FR-141.
pub use orchestrator_security::secret_store_session;
/// Secure file and directory creation helpers.
pub use orchestrator_security::secure_files;
/// Agent selection algorithms and resolution helpers.
pub mod selection;
/// Self-referential workspace safety policies.
pub mod self_referential_policy;
/// Service-layer handlers used by the daemon.
pub mod service;
/// Re-export of the session persistence models and repository helpers.
pub mod session_store;
/// Provider-neutral external source events, bindings, and routing persistence.
pub mod source;
/// Durable idempotency and provenance for source-triggered task automation.
pub mod source_automation;
/// Durable provider connection lifecycle and safe projections.
pub mod source_connection;
/// Deterministic source reaction binding validation and matching.
pub mod source_task_binding;
/// Governed source-to-task template validation and rendering.
pub mod source_task_template;
/// Shared daemon state and state transition helpers.
pub mod state;
pub mod store;
pub mod stream_json;
/// Auto-cleanup of terminated tasks and associated data.
pub mod task_cleanup;
/// High-level task mutation operations.
pub mod task_ops;
/// Re-export of the task-execution persistence abstraction.
///
/// The seven sub-traits composing `TaskRepository` and their synchronous and
/// asynchronous SQLite implementations live in `orchestrator-persistence`
/// (FR-130 Phase A). This module is the re-export plus the tests that drive
/// them through real task creation.
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
