use crate::metrics::{AgentHealthState, CapabilityHealth};
use crate::state::InnerState;
use chrono::Utc;
use std::collections::HashMap;
use tauri::AppHandle;

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

pub fn mark_agent_diseased(state: &InnerState, app: Option<&AppHandle>, agent_id: &str) {
    let mut health = state.agent_health.write().unwrap();
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
    if let Some(app) = app {
        crate::events::emit_event(
            app,
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
}

pub fn increment_consecutive_errors(
    state: &InnerState,
    app: Option<&AppHandle>,
    agent_id: &str,
) -> u32 {
    let mut health = state.agent_health.write().unwrap();
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
    if let Some(app) = app {
        crate::events::emit_event(
            app,
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
    }
    consecutive_errors
}

pub fn reset_consecutive_errors(state: &InnerState, app: Option<&AppHandle>, agent_id: &str) {
    let mut health = state.agent_health.write().unwrap();
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
        if let Some(app) = app {
            crate::events::emit_event(
                app,
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
}

pub fn update_capability_health(
    state: &InnerState,
    agent_id: &str,
    capability: Option<&str>,
    success: bool,
) {
    if let Some(cap) = capability {
        let mut health = state.agent_health.write().unwrap();
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
