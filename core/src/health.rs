use crate::config::HealthPolicyConfig;
use crate::metrics::{AgentHealthState, CapabilityHealth};
use crate::state::InnerState;
use chrono::Utc;
use std::collections::HashMap;

/// Returns a snapshot of an agent's health state for CLI/API display.
/// `diseased_until` is `Some(rfc3339)` only if the agent is currently diseased.
pub fn agent_health_summary(
    health_map: &HashMap<String, AgentHealthState>,
    agent_id: &str,
) -> (bool, Option<String>, u32) {
    let is_healthy = is_agent_healthy(health_map, agent_id);
    match health_map.get(agent_id) {
        None => (true, None, 0),
        Some(state) => {
            let until = state
                .diseased_until
                .filter(|dt| *dt > Utc::now())
                .map(|dt| dt.to_rfc3339());
            (is_healthy, until, state.consecutive_errors)
        }
    }
}

/// Returns whether an agent is currently considered healthy.
pub fn is_agent_healthy(health_map: &HashMap<String, AgentHealthState>, agent_id: &str) -> bool {
    match health_map.get(agent_id) {
        None => true,
        Some(state) => match state.diseased_until {
            None => true,
            Some(until) => Utc::now() >= until,
        },
    }
}

/// Returns whether an agent is healthy enough for a specific capability.
///
/// `success_threshold` controls the minimum per-capability success rate
/// required while the agent is diseased.  Defaults to 0.5 when called
/// with `HealthPolicyConfig::default().capability_success_threshold`.
pub fn is_capability_healthy(
    health_map: &HashMap<String, AgentHealthState>,
    agent_id: &str,
    capability: &str,
    success_threshold: f64,
) -> bool {
    match health_map.get(agent_id) {
        None => true,
        Some(state) => {
            if let Some(until) = state.diseased_until {
                if Utc::now() < until {
                    if let Some(cap_health) = state.capability_health.get(capability) {
                        return cap_health.success_rate() >= success_threshold as f32;
                    }
                    return false;
                }
            }
            true
        }
    }
}

/// Marks an agent as diseased for the configured cooldown window.
///
/// If `policy.disease_duration_hours == 0`, the call is a no-op
/// (disease is disabled for this agent).
pub async fn mark_agent_diseased(state: &InnerState, agent_id: &str, policy: &HealthPolicyConfig) {
    if policy.disease_duration_hours == 0 {
        return;
    }
    let mut health = state.agent_health.write().await;
    let entry = health
        .entry(agent_id.to_string())
        .or_insert(AgentHealthState {
            diseased_until: None,
            consecutive_errors: 0,
            total_lifetime_errors: 0,
            capability_health: std::collections::HashMap::new(),
        });
    entry.diseased_until =
        Some(Utc::now() + chrono::Duration::hours(policy.disease_duration_hours as i64));
    let diseased_until = entry.diseased_until;
    let consecutive_errors = entry.consecutive_errors;
    drop(health);
    state.emit_event(
        "",
        None,
        "agent_health_changed",
        serde_json::json!({
            "agent_id": agent_id,
            "healthy": false,
            "diseased_until": diseased_until.map(|d| d.to_rfc3339()),
            "consecutive_errors": consecutive_errors
        }),
    );
}

/// Increments consecutive error counters for an agent and returns the new count.
pub async fn increment_consecutive_errors(state: &InnerState, agent_id: &str) -> u32 {
    let mut health = state.agent_health.write().await;
    let entry = health
        .entry(agent_id.to_string())
        .or_insert(AgentHealthState {
            diseased_until: None,
            consecutive_errors: 0,
            total_lifetime_errors: 0,
            capability_health: std::collections::HashMap::new(),
        });
    entry.consecutive_errors += 1;
    entry.total_lifetime_errors += 1;
    let consecutive_errors = entry.consecutive_errors;
    let diseased_until = entry.diseased_until;
    let healthy = match diseased_until {
        None => true,
        Some(until) => Utc::now() >= until,
    };
    drop(health);
    state.emit_event(
        "",
        None,
        "agent_health_changed",
        serde_json::json!({
            "agent_id": agent_id,
            "healthy": healthy,
            "diseased_until": diseased_until.map(|d| d.to_rfc3339()),
            "consecutive_errors": consecutive_errors
        }),
    );
    consecutive_errors
}

/// Resets the consecutive-error counter for an agent.
pub async fn reset_consecutive_errors(state: &InnerState, agent_id: &str) {
    let mut health = state.agent_health.write().await;
    if let Some(entry) = health.get_mut(agent_id) {
        if entry.consecutive_errors == 0 {
            return;
        }
        entry.consecutive_errors = 0;
        let diseased_until = entry.diseased_until;
        let healthy = match diseased_until {
            None => true,
            Some(until) => Utc::now() >= until,
        };
        drop(health);
        state.emit_event(
            "",
            None,
            "agent_health_changed",
            serde_json::json!({
                "agent_id": agent_id,
                "healthy": healthy,
                "diseased_until": diseased_until.map(|d| d.to_rfc3339()),
                "consecutive_errors": 0
            }),
        );
    }
}

/// Updates per-capability health statistics for an agent after a step completes.
pub async fn update_capability_health(
    state: &InnerState,
    agent_id: &str,
    capability: Option<&str>,
    success: bool,
) {
    if let Some(cap) = capability {
        let mut health = state.agent_health.write().await;
        let entry = health
            .entry(agent_id.to_string())
            .or_insert_with(|| AgentHealthState {
                diseased_until: None,
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: std::collections::HashMap::new(),
            });

        let cap_health = entry
            .capability_health
            .entry(cap.to_string())
            .or_insert_with(|| CapabilityHealth {
                success_count: 0,
                failure_count: 0,
                last_error_at: None,
            });

        if success {
            cap_health.success_count += 1;
        } else {
            cap_health.failure_count += 1;
            cap_health.last_error_at = Some(Utc::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_agent_healthy_unknown_agent_is_healthy() {
        let map = HashMap::new();
        assert!(is_agent_healthy(&map, "unknown"));
    }

    #[test]
    fn is_agent_healthy_no_disease_is_healthy() {
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: None,
                consecutive_errors: 3,
                total_lifetime_errors: 10,
                capability_health: HashMap::new(),
            },
        );
        assert!(is_agent_healthy(&map, "agent1"));
    }

    #[test]
    fn is_agent_healthy_diseased_in_future_is_unhealthy() {
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() + chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: HashMap::new(),
            },
        );
        assert!(!is_agent_healthy(&map, "agent1"));
    }

    #[test]
    fn is_agent_healthy_diseased_in_past_is_healthy() {
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() - chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: HashMap::new(),
            },
        );
        assert!(is_agent_healthy(&map, "agent1"));
    }

    #[test]
    fn is_capability_healthy_unknown_agent_is_healthy() {
        let map = HashMap::new();
        assert!(is_capability_healthy(&map, "unknown", "qa", 0.5));
    }

    #[test]
    fn is_capability_healthy_when_not_diseased() {
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: None,
                consecutive_errors: 5,
                total_lifetime_errors: 10,
                capability_health: HashMap::new(),
            },
        );
        assert!(is_capability_healthy(&map, "agent1", "qa", 0.5));
    }

    #[test]
    fn is_capability_healthy_diseased_with_good_capability_rate() {
        let mut cap_health = HashMap::new();
        cap_health.insert(
            "qa".to_string(),
            CapabilityHealth {
                success_count: 8,
                failure_count: 2,
                last_error_at: None,
            },
        );
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() + chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: cap_health,
            },
        );
        // 80% success rate >= 0.5 threshold
        assert!(is_capability_healthy(&map, "agent1", "qa", 0.5));
    }

    #[test]
    fn is_capability_healthy_diseased_with_bad_capability_rate() {
        let mut cap_health = HashMap::new();
        cap_health.insert(
            "qa".to_string(),
            CapabilityHealth {
                success_count: 1,
                failure_count: 9,
                last_error_at: None,
            },
        );
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() + chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: cap_health,
            },
        );
        // 10% success rate < 0.5 threshold
        assert!(!is_capability_healthy(&map, "agent1", "qa", 0.5));
    }

    #[test]
    fn is_capability_healthy_diseased_no_capability_data() {
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() + chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: HashMap::new(),
            },
        );
        // Diseased with no capability data -> unhealthy
        assert!(!is_capability_healthy(&map, "agent1", "qa", 0.5));
    }

    #[test]
    fn is_capability_healthy_custom_threshold() {
        let mut cap_health = HashMap::new();
        cap_health.insert(
            "qa".to_string(),
            CapabilityHealth {
                success_count: 3,
                failure_count: 7,
                last_error_at: None,
            },
        );
        let mut map = HashMap::new();
        map.insert(
            "agent1".to_string(),
            AgentHealthState {
                diseased_until: Some(Utc::now() + chrono::Duration::hours(1)),
                consecutive_errors: 0,
                total_lifetime_errors: 0,
                capability_health: cap_health,
            },
        );
        // 30% success rate: fails at default 0.5 threshold
        assert!(!is_capability_healthy(&map, "agent1", "qa", 0.5));
        // 30% success rate: passes at 0.3 threshold
        assert!(is_capability_healthy(&map, "agent1", "qa", 0.3));
    }

    #[tokio::test]
    async fn health_operations_with_test_state() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        let default_policy = HealthPolicyConfig::default();

        // Initially healthy
        let health = state.agent_health.read().await;
        assert!(is_agent_healthy(&health, "test_agent"));
        drop(health);

        // Increment errors
        let count = increment_consecutive_errors(&state, "test_agent").await;
        assert_eq!(count, 1);
        let count = increment_consecutive_errors(&state, "test_agent").await;
        assert_eq!(count, 2);

        // Reset errors
        reset_consecutive_errors(&state, "test_agent").await;
        let health = state.agent_health.read().await;
        assert_eq!(
            health
                .get("test_agent")
                .expect("test_agent should exist after increments")
                .consecutive_errors,
            0
        );
        drop(health);

        // Mark diseased
        mark_agent_diseased(&state, "test_agent", &default_policy).await;
        let health = state.agent_health.read().await;
        assert!(!is_agent_healthy(&health, "test_agent"));
        drop(health);

        // Update capability health
        update_capability_health(&state, "test_agent", Some("qa"), true).await;
        update_capability_health(&state, "test_agent", Some("qa"), true).await;
        update_capability_health(&state, "test_agent", Some("qa"), false).await;

        let health = state.agent_health.read().await;
        let cap = health
            .get("test_agent")
            .expect("test_agent should exist for capability tracking")
            .capability_health
            .get("qa")
            .expect("qa capability should exist");
        assert_eq!(cap.success_count, 2);
        assert_eq!(cap.failure_count, 1);
        assert!(cap.last_error_at.is_some());
    }

    #[tokio::test]
    async fn reset_consecutive_errors_noop_when_already_zero() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Reset on non-existent agent - should be a no-op
        reset_consecutive_errors(&state, "nonexistent").await;
        let health = state.agent_health.read().await;
        assert!(health.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_total_lifetime_errors_incremented() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        increment_consecutive_errors(&state, "test_agent").await;
        increment_consecutive_errors(&state, "test_agent").await;
        increment_consecutive_errors(&state, "test_agent").await;

        let health = state.agent_health.read().await;
        let entry = health.get("test_agent").expect("agent should exist");
        assert_eq!(entry.total_lifetime_errors, 3);
        assert_eq!(entry.consecutive_errors, 3);

        // Reset consecutive, but lifetime should persist
        drop(health);
        reset_consecutive_errors(&state, "test_agent").await;
        let health = state.agent_health.read().await;
        let entry = health.get("test_agent").expect("agent should exist");
        assert_eq!(entry.consecutive_errors, 0);
        assert_eq!(entry.total_lifetime_errors, 3);
    }

    #[tokio::test]
    async fn update_capability_health_none_capability_is_noop() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        update_capability_health(&state, "agent1", None, true).await;
        let health = state.agent_health.read().await;
        assert!(health.get("agent1").is_none());
    }

    #[tokio::test]
    async fn mark_agent_diseased_zero_duration_is_noop() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        let policy = HealthPolicyConfig {
            disease_duration_hours: 0,
            ..Default::default()
        };

        mark_agent_diseased(&state, "test_agent", &policy).await;
        let health = state.agent_health.read().await;
        // Agent should not have been created in the health map at all
        assert!(health.get("test_agent").is_none());
    }

    #[tokio::test]
    async fn mark_agent_diseased_custom_duration() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();
        let policy = HealthPolicyConfig {
            disease_duration_hours: 1,
            ..Default::default()
        };

        mark_agent_diseased(&state, "test_agent", &policy).await;
        let health = state.agent_health.read().await;
        let entry = health.get("test_agent").expect("agent should exist");
        let until = entry.diseased_until.expect("should be diseased");
        // Should be approximately 1 hour from now (not 5)
        let diff = until - Utc::now();
        assert!(diff.num_minutes() <= 60);
        assert!(diff.num_minutes() >= 59);
    }
}
