use crate::metrics::{AgentHealthState, CapabilityHealth};
use crate::state::{write_agent_health, InnerState};
use chrono::Utc;
use std::collections::HashMap;

const DISEASE_DURATION_HOURS: i64 = 5;

pub fn is_agent_healthy(health_map: &HashMap<String, AgentHealthState>, agent_id: &str) -> bool {
    match health_map.get(agent_id) {
        None => true,
        Some(state) => match state.diseased_until {
            None => true,
            Some(until) => Utc::now() >= until,
        },
    }
}

pub fn is_capability_healthy(
    health_map: &HashMap<String, AgentHealthState>,
    agent_id: &str,
    capability: &str,
) -> bool {
    match health_map.get(agent_id) {
        None => true,
        Some(state) => {
            if let Some(until) = state.diseased_until {
                if Utc::now() < until {
                    if let Some(cap_health) = state.capability_health.get(capability) {
                        return cap_health.success_rate() >= 0.5;
                    }
                    return false;
                }
            }
            true
        }
    }
}

pub fn mark_agent_diseased(state: &InnerState, agent_id: &str) {
    let mut health = write_agent_health(state);
    let entry = health
        .entry(agent_id.to_string())
        .or_insert(AgentHealthState {
            diseased_until: None,
            consecutive_errors: 0,
            total_lifetime_errors: 0,
            capability_health: std::collections::HashMap::new(),
        });
    entry.diseased_until = Some(Utc::now() + chrono::Duration::hours(DISEASE_DURATION_HOURS));
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

pub fn increment_consecutive_errors(state: &InnerState, agent_id: &str) -> u32 {
    let mut health = write_agent_health(state);
    let entry = health
        .entry(agent_id.to_string())
        .or_insert(AgentHealthState {
            diseased_until: None,
            consecutive_errors: 0,
            total_lifetime_errors: 0,
            capability_health: std::collections::HashMap::new(),
        });
    entry.consecutive_errors += 1;
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

pub fn reset_consecutive_errors(state: &InnerState, agent_id: &str) {
    let mut health = write_agent_health(state);
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

pub fn update_capability_health(
    state: &InnerState,
    agent_id: &str,
    capability: Option<&str>,
    success: bool,
) {
    if let Some(cap) = capability {
        let mut health = write_agent_health(state);
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
        assert!(is_capability_healthy(&map, "unknown", "qa"));
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
        assert!(is_capability_healthy(&map, "agent1", "qa"));
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
        assert!(is_capability_healthy(&map, "agent1", "qa"));
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
        assert!(!is_capability_healthy(&map, "agent1", "qa"));
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
        assert!(!is_capability_healthy(&map, "agent1", "qa"));
    }

    #[test]
    fn health_operations_with_test_state() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Initially healthy
        assert!(is_agent_healthy(
            &crate::state::read_agent_health(&state),
            "test_agent"
        ));

        // Increment errors
        let count = increment_consecutive_errors(&state, "test_agent");
        assert_eq!(count, 1);
        let count = increment_consecutive_errors(&state, "test_agent");
        assert_eq!(count, 2);

        // Reset errors
        reset_consecutive_errors(&state, "test_agent");
        let health = crate::state::read_agent_health(&state);
        assert_eq!(
            health
                .get("test_agent")
                .expect("test_agent should exist after increments")
                .consecutive_errors,
            0
        );
        drop(health);

        // Mark diseased
        mark_agent_diseased(&state, "test_agent");
        let health = crate::state::read_agent_health(&state);
        assert!(!is_agent_healthy(&health, "test_agent"));
        drop(health);

        // Update capability health
        update_capability_health(&state, "test_agent", Some("qa"), true);
        update_capability_health(&state, "test_agent", Some("qa"), true);
        update_capability_health(&state, "test_agent", Some("qa"), false);

        let health = crate::state::read_agent_health(&state);
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

    #[test]
    fn reset_consecutive_errors_noop_when_already_zero() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        // Reset on non-existent agent - should be a no-op
        reset_consecutive_errors(&state, "nonexistent");
        let health = crate::state::read_agent_health(&state);
        assert!(health.get("nonexistent").is_none());
    }

    #[test]
    fn update_capability_health_none_capability_is_noop() {
        let mut fixture = crate::test_utils::TestState::new();
        let state = fixture.build();

        update_capability_health(&state, "agent1", None, true);
        let health = crate::state::read_agent_health(&state);
        assert!(health.get("agent1").is_none());
    }
}
