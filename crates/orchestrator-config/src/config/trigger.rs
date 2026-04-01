use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::cli_types::ConcurrencyPolicy;

/// Stored configuration for a Trigger resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TriggerConfig {
    /// Cron schedule (mutually exclusive with `event`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<TriggerCronConfig>,

    /// Event source (mutually exclusive with `cron`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<TriggerEventConfig>,

    /// Action to take when the trigger fires.
    pub action: TriggerActionConfig,

    /// Concurrency policy.
    #[serde(default)]
    pub concurrency_policy: ConcurrencyPolicy,

    /// Whether the trigger is suspended.
    #[serde(default)]
    pub suspend: bool,

    /// History retention limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<TriggerHistoryLimitConfig>,

    /// Throttle settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle: Option<TriggerThrottleConfig>,
}

/// Stored cron schedule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerCronConfig {
    /// Standard 5-field cron expression.
    pub schedule: String,
    /// IANA timezone name; defaults to UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Stored event source configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerEventConfig {
    /// Event source type (e.g. `task_completed`, `task_failed`, `webhook`, `filesystem`).
    pub source: String,
    /// Optional filter conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<TriggerEventFilterConfig>,
    /// Webhook-specific authentication configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<TriggerWebhookConfig>,
    /// Filesystem watcher configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<TriggerFilesystemConfig>,
}

/// Stored filesystem watcher configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerFilesystemConfig {
    /// Directories to watch (relative to Workspace root_path).
    pub paths: Vec<String>,
    /// Event types to subscribe to: "create", "modify", "delete".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    /// Debounce window in milliseconds.
    #[serde(default = "default_fs_debounce_ms")]
    pub debounce_ms: u64,
}

fn default_fs_debounce_ms() -> u64 {
    500
}

/// Webhook authentication configuration for per-trigger secret verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerWebhookConfig {
    /// SecretStore reference for signature verification.
    /// All values in the store are tried (supports key rotation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<TriggerSecretRef>,
    /// Custom HTTP header name for the signature (default: `X-Webhook-Signature`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_header: Option<String>,
    /// CRD kind name for plugin lookup. When set, the daemon resolves the CRD's
    /// plugins and executes interceptors/transformers in the webhook request path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crd_ref: Option<String>,
}

/// Reference to a SecretStore for webhook secret resolution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerSecretRef {
    /// Name of the SecretStore to resolve.
    pub from_ref: String,
}

/// Stored event filter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerEventFilterConfig {
    /// Match events from a specific workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// CEL expression for event matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Stored action configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerActionConfig {
    /// Target workflow name.
    pub workflow: String,
    /// Target workspace name.
    pub workspace: String,
    /// Optional arguments passed to the created task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<HashMap<String, Vec<String>>>,
    /// Whether to start the task immediately (default true).
    #[serde(default = "default_start")]
    pub start: bool,
}

fn default_start() -> bool {
    true
}

/// Stored history retention limits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerHistoryLimitConfig {
    /// Number of successful tasks to retain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successful: Option<u32>,
    /// Number of failed tasks to retain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<u32>,
}

/// Stored throttle configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerThrottleConfig {
    /// Minimum interval in seconds between trigger firings.
    #[serde(default)]
    pub min_interval: u64,
}
