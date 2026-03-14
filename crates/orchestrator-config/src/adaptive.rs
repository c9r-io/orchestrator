//! Adaptive planner configuration data types.

use serde::{Deserialize, Serialize};

/// Configuration for agent-driven adaptive planning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdaptivePlannerConfig {
    /// Whether adaptive planning is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Agent id responsible for generating the adaptive plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_agent: Option<String>,
    /// Maximum number of history entries to include in the planning prompt.
    #[serde(default = "default_10")]
    pub max_history: usize,
    /// Temperature hint forwarded to the planner prompt.
    #[serde(default = "default_07")]
    pub temperature: f32,
    /// Planner failure handling policy.
    #[serde(default)]
    pub fallback_mode: AdaptiveFallbackMode,
}

/// Fallback behavior used when adaptive planning fails.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AdaptiveFallbackMode {
    /// Fall back to deterministic planning when the adaptive planner fails.
    #[default]
    SoftFallback,
    /// Treat adaptive planner failures as hard errors.
    FailClosed,
}

fn default_10() -> usize {
    10
}

fn default_07() -> f32 {
    0.7
}

impl Default for AdaptivePlannerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            planner_agent: None,
            max_history: 10,
            temperature: 0.7,
            fallback_mode: AdaptiveFallbackMode::SoftFallback,
        }
    }
}
