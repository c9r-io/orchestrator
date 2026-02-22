//! Agent Selection Metrics Module
//!
//! Provides runtime metrics collection, agent scoring, and intelligent selection strategies
//! beyond simple cost-based sorting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Runtime metrics collected during agent execution
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMetrics {
    /// Total number of runs
    pub total_runs: u32,
    /// Number of successful runs
    pub successful_runs: u32,
    /// Number of failed runs
    pub failed_runs: u32,
    /// Average duration in milliseconds
    pub avg_duration_ms: u64,
    /// P95 duration in milliseconds
    pub p95_duration_ms: u64,
    /// Recent success rate (exponential moving average)
    pub recent_success_rate: f32,
    /// Recent average duration (exponential moving average)
    pub recent_avg_duration_ms: u64,
    /// Current load (number of concurrent executions)
    pub current_load: u32,
    /// Last used timestamp
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Health status for a specific capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityHealth {
    /// Number of successful executions for this capability
    pub success_count: u32,
    /// Number of failed executions for this capability
    pub failure_count: u32,
    /// Last error timestamp for this capability
    pub last_error_at: Option<DateTime<Utc>>,
}

impl CapabilityHealth {
    /// Calculate success rate for this capability
    pub fn success_rate(&self) -> f32 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 0.5; // Neutral default
        }
        self.success_count as f32 / total as f32
    }
}

/// Extended health state with capability-level tracking
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentHealthState {
    /// When the agent becomes healthy again (None = always healthy)
    pub diseased_until: Option<DateTime<Utc>>,
    /// Consecutive error count
    pub consecutive_errors: u32,
    /// Total lifetime errors
    pub total_lifetime_errors: u32,
    /// Per-capability health tracking
    pub capability_health: HashMap<String, CapabilityHealth>,
}

/// Selection strategy for agent choosing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStrategy {
    /// Static cost-based sorting (legacy behavior)
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Requirements for agent selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionRequirement {
    /// The capability being requested
    pub capability: String,
    /// Selected strategy
    pub strategy: SelectionStrategy,
    /// Weights for adaptive strategy
    pub weights: SelectionWeights,
    /// Maximum load allowed per agent
    pub max_load: u32,
    /// Consider health state
    pub consider_health: bool,
    /// Consider capability-specific health
    pub capability_aware: bool,
}

impl Default for SelectionRequirement {
    fn default() -> Self {
        Self {
            capability: String::new(),
            strategy: SelectionStrategy::Adaptive,
            weights: SelectionWeights::default(),
            max_load: 5,
            consider_health: true,
            capability_aware: true,
        }
    }
}

/// Result of agent scoring
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentScore {
    pub agent_id: String,
    pub total_score: f32,
    pub cost_score: f32,
    pub success_rate_score: f32,
    pub performance_score: f32,
    pub load_penalty: f32,
    pub health_penalty: f32,
}

/// Metrics collector for tracking agent performance
pub struct MetricsCollector;

impl MetricsCollector {
    /// Create a new metrics entry for a fresh agent
    pub fn new_agent_metrics() -> AgentMetrics {
        AgentMetrics {
            total_runs: 0,
            successful_runs: 0,
            failed_runs: 0,
            avg_duration_ms: 0,
            p95_duration_ms: 0,
            recent_success_rate: 0.5,
            recent_avg_duration_ms: 0,
            current_load: 0,
            last_used_at: None,
        }
    }

    /// Update metrics after a successful execution
    pub fn record_success(metrics: &mut AgentMetrics, duration_ms: u64) {
        metrics.total_runs += 1;
        metrics.successful_runs += 1;
        metrics.last_used_at = Some(Utc::now());

        // Update average duration with exponential moving average
        if metrics.total_runs == 1 {
            metrics.avg_duration_ms = duration_ms;
            metrics.recent_avg_duration_ms = duration_ms;
        } else {
            // EMA with alpha = 0.3
            let alpha = 0.3;
            metrics.avg_duration_ms = ((1.0 - alpha) * metrics.avg_duration_ms as f64
                + alpha * duration_ms as f64) as u64;
            metrics.recent_avg_duration_ms = ((1.0 - alpha) * metrics.recent_avg_duration_ms as f64
                + alpha * duration_ms as f64) as u64;
        }

        // Update success rate with EMA
        let alpha = 0.3;
        metrics.recent_success_rate = (1.0 - alpha) * metrics.recent_success_rate + alpha * 1.0;
    }

    /// Update metrics after a failed execution
    pub fn record_failure(metrics: &mut AgentMetrics) {
        metrics.total_runs += 1;
        metrics.failed_runs += 1;
        metrics.last_used_at = Some(Utc::now());

        // Update success rate with EMA
        let alpha = 0.3;
        metrics.recent_success_rate = (1.0 - alpha) * metrics.recent_success_rate + alpha * 0.0;
    }

    /// Increment load when task starts
    pub fn increment_load(metrics: &mut AgentMetrics) {
        metrics.current_load += 1;
    }

    /// Decrement load when task ends
    pub fn decrement_load(metrics: &mut AgentMetrics) {
        if metrics.current_load > 0 {
            metrics.current_load -= 1;
        }
    }
}

/// Calculate agent score based on multiple factors
pub fn calculate_agent_score(
    agent_id: &str,
    cost: Option<u32>,
    metrics: &Option<AgentMetrics>,
    health: &Option<AgentHealthState>,
    requirement: &SelectionRequirement,
) -> AgentScore {
    // Base cost score (0-100, lower cost = higher score)
    let cost_score = 100.0 - (cost.unwrap_or(50) as f32);

    // Success rate score
    let success_rate_score = if let Some(m) = metrics {
        if m.total_runs > 0 {
            (m.successful_runs as f32 / m.total_runs as f32) * 100.0
        } else {
            m.recent_success_rate * 100.0
        }
    } else {
        50.0 // Default neutral
    };

    // Performance score (inverse of duration)
    // Assume 60s = 0 score, 10s = 100 score
    let performance_score = if let Some(m) = metrics {
        if m.avg_duration_ms > 0 {
            (60000.0 / m.avg_duration_ms as f32).min(100.0)
        } else {
            50.0
        }
    } else {
        50.0
    };

    // Load penalty (0 to -50)
    let load_penalty = if let Some(m) = metrics {
        -(m.current_load as f32 * 10.0).min(50.0)
    } else {
        0.0
    };

    // Health penalty
    let health_penalty = if let Some(h) = health {
        if !is_agent_globally_healthy(h) {
            -100.0
        } else if h.consecutive_errors > 0 {
            -(h.consecutive_errors as f32 * 15.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Weighted total
    let total_score = match requirement.strategy {
        SelectionStrategy::CostBased => cost_score * 1.0,
        SelectionStrategy::SuccessRateWeighted => cost_score * 0.2 + success_rate_score * 0.8,
        SelectionStrategy::PerformanceFirst => {
            cost_score * 0.2 + performance_score * 0.6 + success_rate_score * 0.2
        }
        SelectionStrategy::Adaptive => {
            cost_score * requirement.weights.cost
                + success_rate_score * requirement.weights.success_rate
                + performance_score * requirement.weights.performance
                + load_penalty * requirement.weights.load
                + health_penalty
        }
        SelectionStrategy::LoadBalanced => {
            cost_score * 0.2 + success_rate_score * 0.3 + load_penalty.abs() * 0.5
        }
        SelectionStrategy::CapabilityAware => {
            // Similar to adaptive but health is more important
            cost_score * 0.15
                + success_rate_score * 0.35
                + performance_score * 0.2
                + health_penalty.max(-50.0) // Don't completely exclude
        }
    };

    AgentScore {
        agent_id: agent_id.to_string(),
        total_score,
        cost_score,
        success_rate_score,
        performance_score,
        load_penalty,
        health_penalty,
    }
}

/// Check if agent is globally healthy
fn is_agent_globally_healthy(health: &AgentHealthState) -> bool {
    match health.diseased_until {
        None => true,
        Some(until) => Utc::now() >= until,
    }
}

/// Check if agent is healthy for a specific capability
#[allow(dead_code)]
pub fn is_capability_healthy(
    health: &Option<AgentHealthState>,
    _agent_id: &str,
    capability: &str,
) -> bool {
    // First check global health
    if let Some(h) = health {
        if !is_agent_globally_healthy(h) {
            // Check if we should use capability-specific health
            if let Some(cap_health) = h.capability_health.get(capability) {
                // If capability-specific health is good, allow it
                if cap_health.success_rate() >= 0.5 {
                    return true;
                }
            }
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_agent_metrics() {
        let metrics = MetricsCollector::new_agent_metrics();
        assert_eq!(metrics.total_runs, 0);
        assert_eq!(metrics.recent_success_rate, 0.5);
    }

    #[test]
    fn test_record_success() {
        let mut metrics = AgentMetrics::default();
        MetricsCollector::record_success(&mut metrics, 1000);
        assert_eq!(metrics.total_runs, 1);
        assert_eq!(metrics.successful_runs, 1);
        assert_eq!(metrics.avg_duration_ms, 1000);
    }

    #[test]
    fn test_record_failure() {
        let mut metrics = AgentMetrics::default();
        MetricsCollector::record_failure(&mut metrics);
        assert_eq!(metrics.total_runs, 1);
        assert_eq!(metrics.failed_runs, 1);
    }

    #[test]
    fn test_capability_health_rate() {
        let cap_health = CapabilityHealth {
            success_count: 8,
            failure_count: 2,
            last_error_at: None,
        };
        assert!((cap_health.success_rate() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_agent_score_calculation() {
        let cost = Some(30);
        let metrics = Some(AgentMetrics {
            total_runs: 10,
            successful_runs: 8,
            failed_runs: 2,
            avg_duration_ms: 5000,
            p95_duration_ms: 8000,
            recent_success_rate: 0.8,
            recent_avg_duration_ms: 5000,
            current_load: 1,
            last_used_at: None,
        });
        let health = Some(AgentHealthState::default());

        let req = SelectionRequirement::default();
        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &req);

        assert!(score.total_score > 0.0);
        assert_eq!(score.cost_score, 70.0); // 100 - 30
    }
}
