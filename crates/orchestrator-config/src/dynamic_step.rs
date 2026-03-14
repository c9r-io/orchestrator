//! Dynamic step configuration data types.

use serde::{Deserialize, Serialize};

/// Configuration for a dynamic step in the pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicStepConfig {
    /// Unique identifier for this dynamic step
    pub id: String,
    /// Description for documentation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The step type
    pub step_type: String,
    /// Agent ID to use
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Template for the agent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// CEL trigger condition - when to consider this step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Priority (higher = more likely to be selected)
    #[serde(default)]
    pub priority: i32,
    /// Maximum times this step can be executed per item
    #[serde(default)]
    pub max_runs: Option<u32>,
}
