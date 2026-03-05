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
            cost_score * 0.2 + success_rate_score * 0.3 + load_penalty * 0.5
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

    // Helper function to create test metrics
    fn create_test_metrics(
        total_runs: u32,
        successful_runs: u32,
        avg_duration_ms: u64,
        current_load: u32,
    ) -> AgentMetrics {
        AgentMetrics {
            total_runs,
            successful_runs,
            avg_duration_ms,
            current_load,
            ..Default::default()
        }
    }

    // ===== SelectionStrategy Tests =====

    #[test]
    fn test_selection_strategy_cost_based() {
        let cost = Some(25);
        let metrics = Some(create_test_metrics(10, 8, 5000, 1));
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::CostBased,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // CostBased: total_score = cost_score * 1.0 = (100 - 25) * 1.0 = 75.0
        assert!((score.total_score - 75.0).abs() < 0.01);
        assert!((score.cost_score - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_selection_strategy_success_rate_weighted() {
        let cost = Some(30);
        // success_rate = 8/10 = 0.8, success_rate_score = 80.0
        let metrics = Some(create_test_metrics(10, 8, 5000, 0));
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::SuccessRateWeighted,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // cost_score = 100 - 30 = 70.0
        // success_rate_score = 8/10 * 100 = 80.0
        // total = cost*0.2 + success*0.8 = 70*0.2 + 80*0.8 = 14 + 64 = 78.0
        let expected = 70.0 * 0.2 + 80.0 * 0.8;
        assert!((score.total_score - expected).abs() < 0.01);
    }

    #[test]
    fn test_selection_strategy_performance_first() {
        let cost = Some(20);
        // avg_duration = 3000ms, performance_score = 60000/3000 = 20.0
        let metrics = Some(create_test_metrics(10, 8, 3000, 0));
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::PerformanceFirst,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // cost_score = 100 - 20 = 80.0
        // success_rate_score = 8/10 * 100 = 80.0
        // performance_score = 60000/3000 = 20.0
        // total = cost*0.2 + perf*0.6 + success*0.2 = 80*0.2 + 20*0.6 + 80*0.2
        //       = 16 + 12 + 16 = 44.0
        let expected = 80.0 * 0.2 + 20.0 * 0.6 + 80.0 * 0.2;
        assert!((score.total_score - expected).abs() < 0.01);
    }

    #[test]
    fn test_selection_strategy_load_balanced() {
        let cost = Some(30);
        // current_load = 3, load_penalty = -(3 * 10.0) = -30.0
        let metrics = Some(create_test_metrics(10, 8, 5000, 3));
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::LoadBalanced,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // cost_score = 100 - 30 = 70.0
        // success_rate_score = 8/10 * 100 = 80.0
        // load_penalty = -30.0
        // total = cost*0.2 + success*0.3 + load*0.5 = 70*0.2 + 80*0.3 + (-30)*0.5
        //       = 14 + 24 - 15 = 23.0
        let expected = 70.0 * 0.2 + 80.0 * 0.3 + (-30.0) * 0.5;
        assert!((score.total_score - expected).abs() < 0.01);
    }

    #[test]
    fn test_load_balanced_low_load_scores_higher() {
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::LoadBalanced,
            ..Default::default()
        };

        // Agent A: low load (1)
        let metrics_a = Some(create_test_metrics(10, 8, 5000, 1));
        let score_a = calculate_agent_score("agent_a", Some(30), &metrics_a, &health, &requirement);

        // Agent B: high load (4)
        let metrics_b = Some(create_test_metrics(10, 8, 5000, 4));
        let score_b = calculate_agent_score("agent_b", Some(30), &metrics_b, &health, &requirement);

        assert!(
            score_a.total_score > score_b.total_score,
            "Low-load agent should score higher: a={}, b={}",
            score_a.total_score,
            score_b.total_score
        );
    }

    #[test]
    fn test_selection_strategy_capability_aware() {
        let cost = Some(40);
        let metrics = Some(create_test_metrics(10, 9, 4000, 1));
        // consecutive_errors = 2, health_penalty = -(2 * 15.0) = -30.0
        let health = Some(AgentHealthState {
            consecutive_errors: 2,
            ..Default::default()
        });
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::CapabilityAware,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // cost_score = 100 - 40 = 60.0
        // success_rate_score = 9/10 * 100 = 90.0
        // performance_score = 60000/4000 = 15.0
        // health_penalty = -30.0, max(-50) = -30.0
        // total = cost*0.15 + success*0.35 + perf*0.2 + health_penalty.max(-50)
        //       = 60*0.15 + 90*0.35 + 15*0.2 + (-30.0)
        //       = 9 + 31.5 + 3 - 30 = 13.5
        let expected = 60.0 * 0.15 + 90.0 * 0.35 + 15.0 * 0.2 + (-30.0_f32).max(-50.0);
        assert!((score.total_score - expected).abs() < 0.01);
    }

    // ===== EMA Convergence Tests =====

    #[test]
    fn test_ema_convergence_success() {
        let mut metrics = AgentMetrics::default();
        // Initial recent_success_rate = 0.5
        // After many successes, should converge toward 1.0

        for _ in 0..10 {
            MetricsCollector::record_success(&mut metrics, 1000);
        }

        // After 10 successes: rate should be > 0.95
        assert!(
            metrics.recent_success_rate > 0.95,
            "Expected rate > 0.95, got {}",
            metrics.recent_success_rate
        );
    }

    #[test]
    fn test_ema_convergence_failure() {
        let mut metrics = AgentMetrics::default();
        // Initial recent_success_rate = 0.5
        // After many failures, should converge toward 0.0

        for _ in 0..10 {
            MetricsCollector::record_failure(&mut metrics);
        }

        // After 10 failures: rate should be < 0.05
        assert!(
            metrics.recent_success_rate < 0.05,
            "Expected rate < 0.05, got {}",
            metrics.recent_success_rate
        );
    }

    #[test]
    fn test_ema_convergence_mixed() {
        let mut metrics = AgentMetrics::default();
        // Initial recent_success_rate = 0.5

        // Apply 5 successes
        for _ in 0..5 {
            MetricsCollector::record_success(&mut metrics, 1000);
        }
        let rate_after_success = metrics.recent_success_rate;

        // Apply 3 failures
        for _ in 0..3 {
            MetricsCollector::record_failure(&mut metrics);
        }

        // Rate should have decreased after failures
        assert!(
            metrics.recent_success_rate < rate_after_success,
            "Rate should decrease after failures: before={}, after={}",
            rate_after_success,
            metrics.recent_success_rate
        );

        // But should still be positive
        assert!(metrics.recent_success_rate > 0.0);
    }

    // ===== Boundary Condition Tests =====

    #[test]
    fn test_boundary_zero_total_runs() {
        let cost = Some(30);
        // total_runs = 0, so success_rate_score uses recent_success_rate * 100 = 0.5 * 100 = 50.0
        let metrics = Some(AgentMetrics {
            total_runs: 0,
            recent_success_rate: 0.5,
            ..Default::default()
        });
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::SuccessRateWeighted,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // success_rate_score = recent_success_rate * 100 = 50.0
        // cost_score = 70.0
        // total = 70*0.2 + 50*0.8 = 14 + 40 = 54.0
        let expected = 70.0 * 0.2 + 50.0 * 0.8;
        assert!((score.success_rate_score - 50.0).abs() < 0.01);
        assert!((score.total_score - expected).abs() < 0.01);
    }

    #[test]
    fn test_boundary_none_metrics() {
        let cost = Some(40);
        let metrics: Option<AgentMetrics> = None;
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement {
            strategy: SelectionStrategy::PerformanceFirst,
            ..Default::default()
        };

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // With None metrics:
        // success_rate_score = 50.0 (default neutral)
        // performance_score = 50.0 (default neutral)
        // cost_score = 60.0
        // total = 60*0.2 + 50*0.6 + 50*0.2 = 12 + 30 + 10 = 52.0
        assert!((score.success_rate_score - 50.0).abs() < 0.01);
        assert!((score.performance_score - 50.0).abs() < 0.01);
        let expected = 60.0 * 0.2 + 50.0 * 0.6 + 50.0 * 0.2;
        assert!((score.total_score - expected).abs() < 0.01);
    }

    #[test]
    fn test_boundary_none_health() {
        let cost = Some(30);
        let metrics = Some(create_test_metrics(10, 8, 5000, 0));
        let health: Option<AgentHealthState> = None;
        let requirement = SelectionRequirement::default();

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // With None health: health_penalty = 0.0
        assert!((score.health_penalty - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_boundary_max_load() {
        let cost = Some(30);
        // current_load = 10, but load_penalty is capped at -50.0
        let metrics = Some(create_test_metrics(10, 8, 5000, 10));
        let health = Some(AgentHealthState::default());
        let requirement = SelectionRequirement::default();

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // load_penalty = -(10 * 10.0).min(50.0) = -50.0
        assert!(
            (score.load_penalty - (-50.0)).abs() < 0.01,
            "Expected load_penalty -50.0, got {}",
            score.load_penalty
        );
    }

    // ===== Health Penalty Tests =====

    #[test]
    fn test_health_penalty_diseased_agent() {
        let cost = Some(30);
        let metrics = Some(create_test_metrics(10, 8, 5000, 0));
        // diseased_until is in the future
        let health = Some(AgentHealthState {
            diseased_until: Some(Utc::now() + chrono::Duration::seconds(3600)),
            ..Default::default()
        });
        let requirement = SelectionRequirement::default();

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // Agent is diseased, so health_penalty = -100.0
        assert!(
            (score.health_penalty - (-100.0)).abs() < 0.01,
            "Expected health_penalty -100.0, got {}",
            score.health_penalty
        );
    }

    #[test]
    fn test_health_penalty_consecutive_errors() {
        let cost = Some(30);
        let metrics = Some(create_test_metrics(10, 8, 5000, 0));
        // 3 consecutive errors
        let health = Some(AgentHealthState {
            consecutive_errors: 3,
            ..Default::default()
        });
        let requirement = SelectionRequirement::default();

        let score = calculate_agent_score("test_agent", cost, &metrics, &health, &requirement);

        // health_penalty = -(3 * 15.0) = -45.0
        assert!(
            (score.health_penalty - (-45.0)).abs() < 0.01,
            "Expected health_penalty -45.0, got {}",
            score.health_penalty
        );
    }

    // ===== CapabilityHealth Tests =====

    #[test]
    fn test_capability_health_zero_total() {
        let cap_health = CapabilityHealth {
            success_count: 0,
            failure_count: 0,
            last_error_at: None,
        };

        // When total is 0, success_rate should return 0.5 (neutral)
        assert!(
            (cap_health.success_rate() - 0.5).abs() < 0.001,
            "Expected success_rate 0.5, got {}",
            cap_health.success_rate()
        );
    }

    // ===== Load Operation Tests =====

    #[test]
    fn test_load_decrement_from_zero() {
        let mut metrics = AgentMetrics {
            current_load: 0,
            ..Default::default()
        };

        // Decrementing from 0 should stay at 0
        MetricsCollector::decrement_load(&mut metrics);
        assert_eq!(
            metrics.current_load, 0,
            "Expected load to stay at 0, got {}",
            metrics.current_load
        );
    }

    #[test]
    fn test_load_increment_decrement_cycle() {
        let mut metrics = AgentMetrics::default();

        // Start at 0
        assert_eq!(metrics.current_load, 0);

        // Increment to 1
        MetricsCollector::increment_load(&mut metrics);
        assert_eq!(metrics.current_load, 1);

        // Increment to 2
        MetricsCollector::increment_load(&mut metrics);
        assert_eq!(metrics.current_load, 2);

        // Decrement to 1
        MetricsCollector::decrement_load(&mut metrics);
        assert_eq!(metrics.current_load, 1);

        // Decrement to 0
        MetricsCollector::decrement_load(&mut metrics);
        assert_eq!(metrics.current_load, 0);

        // Decrement from 0 should stay at 0
        MetricsCollector::decrement_load(&mut metrics);
        assert_eq!(metrics.current_load, 0);
    }
}
