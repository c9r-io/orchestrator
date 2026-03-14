//! Agent selection strategy and scoring weight types.

use serde::{Deserialize, Serialize};

/// Selection strategy for agent choosing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// Static cost-based sorting (default behavior)
    #[default]
    CostBased,
    /// Success rate weighted selection
    SuccessRateWeighted,
    /// Performance (latency) focused selection
    PerformanceFirst,
    /// Adaptive scoring with configurable weights
    Adaptive,
    /// Load-balanced selection
    LoadBalanced,
    /// Capability-aware health tracking
    CapabilityAware,
}

/// Weights for adaptive scoring
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionWeights {
    /// Weight for cost factor (0.0 - 1.0)
    pub cost: f32,
    /// Weight for success rate (0.0 - 1.0)
    pub success_rate: f32,
    /// Weight for performance (0.0 - 1.0)
    pub performance: f32,
    /// Weight for load balancing (0.0 - 1.0)
    pub load: f32,
}

impl Default for SelectionWeights {
    fn default() -> Self {
        Self {
            cost: 0.20,
            success_rate: 0.30,
            performance: 0.25,
            load: 0.25,
        }
    }
}
