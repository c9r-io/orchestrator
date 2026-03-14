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
    /// Event source type (e.g. `task_completed`, `task_failed`).
    pub source: String,
    /// Optional filter conditions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<TriggerEventFilterConfig>,
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
