//! Configuration models, loading, and validation for the Agent Orchestrator.
//!
//! This crate provides the pure data types and validation logic used by the
//! orchestrator core, CLI, and daemon.  It intentionally avoids runtime
//! dependencies (async, database, process spawning) so that configuration
//! changes do not trigger recompilation of the scheduler or persistence layers.
#![cfg_attr(
    not(any(test, feature = "test-harness")),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

/// Adaptive planner configuration data types.
pub mod adaptive;
/// K8s-style declarative resource types shared by the CLI surface.
pub mod cli_types;
/// Configuration model types.
pub mod config;
/// CRD scope enum.
pub mod crd_scope;
/// CRD data types (definitions, resources, manifests).
pub mod crd_types;
/// Dynamic step configuration data types.
pub mod dynamic_step;
/// Environment resolution utilities for command execution.
pub mod env_resolve;
/// Plugin security policy — controls which CRD plugin commands are permitted.
pub mod plugin_policy;
/// Unified resource store and apply-result types.
pub mod resource_store;
/// Agent selection strategy and scoring weight types.
pub mod selection;
