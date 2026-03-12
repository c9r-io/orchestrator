//! Agent Lifecycle Management Module
//!
//! Provides cordon, drain, and uncordon operations for agents at runtime.
//! Follows the same pattern as `health.rs` — free async functions that
//! acquire `InnerState` locks, mutate state, and emit events.

use crate::config::AgentConfig;
use crate::metrics::{AgentLifecycleState, AgentRuntimeState};
use crate::state::InnerState;
use chrono::Utc;
use std::collections::HashMap;

/// Cordon an agent — mark as not schedulable, but don't drain existing work.
pub async fn cordon_agent(state: &InnerState, agent_id: &str) -> Result<(), String> {
    let mut lifecycle = state.agent_lifecycle.write().await;
    let entry = lifecycle
        .entry(agent_id.to_string())
        .or_insert_with(AgentRuntimeState::default);

    match entry.lifecycle {
        AgentLifecycleState::Active => {
            entry.lifecycle = AgentLifecycleState::Cordoned;
            drop(lifecycle);
            state.emit_event(
                "",
                None,
                "agent_lifecycle_changed",
                serde_json::json!({
                    "agent_id": agent_id,
                    "state": "Cordoned",
                    "action": "cordon",
                }),
            );
            Ok(())
        }
        other => Err(format!(
            "cannot cordon agent '{}': current state is {}",
            agent_id,
            other.as_str()
        )),
    }
}

/// Uncordon an agent — restore to Active from Cordoned state.
pub async fn uncordon_agent(state: &InnerState, agent_id: &str) -> Result<(), String> {
    let mut lifecycle = state.agent_lifecycle.write().await;
    let entry = lifecycle
        .entry(agent_id.to_string())
        .or_insert_with(AgentRuntimeState::default);

    match entry.lifecycle {
        AgentLifecycleState::Cordoned | AgentLifecycleState::Drained => {
            entry.lifecycle = AgentLifecycleState::Active;
            entry.drain_requested_at = None;
            entry.drain_timeout_secs = None;
            drop(lifecycle);
            state.emit_event(
                "",
                None,
                "agent_lifecycle_changed",
                serde_json::json!({
                    "agent_id": agent_id,
                    "state": "Active",
                    "action": "uncordon",
                }),
            );
            Ok(())
        }
        AgentLifecycleState::Active => Ok(()),
        AgentLifecycleState::Draining => Err(format!(
            "cannot uncordon agent '{}': currently draining — wait for drain to complete or use uncordon after drained",
            agent_id
        )),
    }
}

/// Drain an agent — mark as draining (no new work), wait for in-flight to complete.
pub async fn drain_agent(
    state: &InnerState,
    agent_id: &str,
    timeout_secs: Option<u64>,
) -> Result<AgentLifecycleState, String> {
    let mut lifecycle = state.agent_lifecycle.write().await;
    let entry = lifecycle
        .entry(agent_id.to_string())
        .or_insert_with(AgentRuntimeState::default);

    match entry.lifecycle {
        AgentLifecycleState::Active | AgentLifecycleState::Cordoned => {
            if entry.in_flight_items == 0 {
                entry.lifecycle = AgentLifecycleState::Drained;
                let state_str = "Drained";
                drop(lifecycle);
                state.emit_event(
                    "",
                    None,
                    "agent_lifecycle_changed",
                    serde_json::json!({
                        "agent_id": agent_id,
                        "state": state_str,
                        "action": "drain",
                        "immediate": true,
                    }),
                );
                Ok(AgentLifecycleState::Drained)
            } else {
                entry.lifecycle = AgentLifecycleState::Draining;
                entry.drain_requested_at = Some(Utc::now());
                entry.drain_timeout_secs = timeout_secs;
                let in_flight = entry.in_flight_items;
                drop(lifecycle);
                state.emit_event(
                    "",
                    None,
                    "agent_lifecycle_changed",
                    serde_json::json!({
                        "agent_id": agent_id,
                        "state": "Draining",
                        "action": "drain",
                        "in_flight_items": in_flight,
                        "timeout_secs": timeout_secs,
                    }),
                );
                Ok(AgentLifecycleState::Draining)
            }
        }
        AgentLifecycleState::Draining => Ok(AgentLifecycleState::Draining),
        AgentLifecycleState::Drained => Ok(AgentLifecycleState::Drained),
    }
}

/// Increment in-flight count when an agent starts work on an item.
pub async fn increment_in_flight(state: &InnerState, agent_id: &str) {
    let mut lifecycle = state.agent_lifecycle.write().await;
    let entry = lifecycle
        .entry(agent_id.to_string())
        .or_insert_with(AgentRuntimeState::default);
    entry.in_flight_items += 1;
}

/// Decrement in-flight count and auto-transition Draining → Drained if zero.
pub async fn decrement_in_flight_and_check(state: &InnerState, agent_id: &str) {
    let mut lifecycle = state.agent_lifecycle.write().await;
    if let Some(entry) = lifecycle.get_mut(agent_id) {
        if entry.in_flight_items > 0 {
            entry.in_flight_items -= 1;
        }
        if entry.lifecycle == AgentLifecycleState::Draining && entry.in_flight_items == 0 {
            entry.lifecycle = AgentLifecycleState::Drained;
            drop(lifecycle);
            state.emit_event(
                "",
                None,
                "agent_lifecycle_changed",
                serde_json::json!({
                    "agent_id": agent_id,
                    "state": "Drained",
                    "action": "drain_completed",
                }),
            );
        }
    }
}

/// Sweep all draining agents and force-drain those past their timeout.
pub async fn drain_timeout_sweep(state: &InnerState) {
    let now = Utc::now();
    let mut force_drained = Vec::new();

    {
        let mut lifecycle = state.agent_lifecycle.write().await;
        for (agent_id, entry) in lifecycle.iter_mut() {
            if entry.lifecycle != AgentLifecycleState::Draining {
                continue;
            }
            if let (Some(requested_at), Some(timeout_secs)) =
                (entry.drain_requested_at, entry.drain_timeout_secs)
            {
                let elapsed = (now - requested_at).num_seconds();
                if elapsed >= timeout_secs as i64 {
                    entry.lifecycle = AgentLifecycleState::Drained;
                    force_drained.push((agent_id.clone(), entry.in_flight_items));
                    entry.in_flight_items = 0;
                }
            }
        }
    }

    for (agent_id, remaining) in &force_drained {
        state.emit_event(
            "",
            None,
            "agent_lifecycle_changed",
            serde_json::json!({
                "agent_id": agent_id,
                "state": "Drained",
                "action": "drain_timeout",
                "force_drained_remaining_items": remaining,
            }),
        );
    }
}

/// Check if cordoning/draining this agent would leave zero active agents
/// for any capability. Returns the list of capabilities that would be orphaned.
pub fn warn_if_last_active_agent(
    agent_id: &str,
    agents: &HashMap<String, AgentConfig>,
    lifecycle_map: &HashMap<String, AgentRuntimeState>,
) -> Vec<String> {
    let target_caps: Vec<String> = agents
        .get(agent_id)
        .map(|a| a.capabilities.clone())
        .unwrap_or_default();

    let mut orphaned = Vec::new();
    for cap in &target_caps {
        let remaining_active = agents
            .iter()
            .filter(|(id, cfg)| {
                if *id == agent_id {
                    return false;
                }
                if !cfg.enabled || !cfg.supports_capability(cap) {
                    return false;
                }
                lifecycle_map
                    .get(*id)
                    .map(|s| s.lifecycle.is_schedulable())
                    .unwrap_or(true)
            })
            .count();

        if remaining_active == 0 {
            orphaned.push(cap.clone());
        }
    }
    orphaned
}

/// Get the lifecycle state for a specific agent, defaulting to Active.
pub fn get_lifecycle_state(
    lifecycle_map: &HashMap<String, AgentRuntimeState>,
    agent_id: &str,
) -> AgentLifecycleState {
    lifecycle_map
        .get(agent_id)
        .map(|s| s.lifecycle)
        .unwrap_or(AgentLifecycleState::Active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestState;

    #[tokio::test]
    async fn cordon_active_agent_succeeds() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        cordon_agent(&state, "agent1").await.unwrap();

        let lifecycle = state.agent_lifecycle.read().await;
        assert_eq!(
            lifecycle.get("agent1").unwrap().lifecycle,
            AgentLifecycleState::Cordoned
        );
    }

    #[tokio::test]
    async fn cordon_already_cordoned_fails() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        cordon_agent(&state, "agent1").await.unwrap();
        let result = cordon_agent(&state, "agent1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn uncordon_cordoned_agent_succeeds() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        cordon_agent(&state, "agent1").await.unwrap();
        uncordon_agent(&state, "agent1").await.unwrap();

        let lifecycle = state.agent_lifecycle.read().await;
        assert_eq!(
            lifecycle.get("agent1").unwrap().lifecycle,
            AgentLifecycleState::Active
        );
    }

    #[tokio::test]
    async fn drain_with_no_inflight_goes_directly_to_drained() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        let result = drain_agent(&state, "agent1", None).await.unwrap();
        assert_eq!(result, AgentLifecycleState::Drained);
    }

    #[tokio::test]
    async fn drain_with_inflight_goes_to_draining() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        increment_in_flight(&state, "agent1").await;
        let result = drain_agent(&state, "agent1", Some(60)).await.unwrap();
        assert_eq!(result, AgentLifecycleState::Draining);
    }

    #[tokio::test]
    async fn decrement_inflight_completes_drain() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        increment_in_flight(&state, "agent1").await;
        drain_agent(&state, "agent1", None).await.unwrap();

        decrement_in_flight_and_check(&state, "agent1").await;

        let lifecycle = state.agent_lifecycle.read().await;
        assert_eq!(
            lifecycle.get("agent1").unwrap().lifecycle,
            AgentLifecycleState::Drained
        );
    }

    #[tokio::test]
    async fn drain_timeout_sweep_forces_drained() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        increment_in_flight(&state, "agent1").await;
        // Set drain with 0-second timeout (immediately expired)
        {
            let mut lifecycle = state.agent_lifecycle.write().await;
            let entry = lifecycle.get_mut("agent1").unwrap();
            entry.lifecycle = AgentLifecycleState::Draining;
            entry.drain_requested_at = Some(Utc::now() - chrono::Duration::seconds(10));
            entry.drain_timeout_secs = Some(1);
        }

        drain_timeout_sweep(&state).await;

        let lifecycle = state.agent_lifecycle.read().await;
        assert_eq!(
            lifecycle.get("agent1").unwrap().lifecycle,
            AgentLifecycleState::Drained
        );
    }

    #[test]
    fn warn_if_last_active_agent_detects_orphaned_capabilities() {
        let mut agents = HashMap::new();
        let mut cfg1 = AgentConfig::new();
        cfg1.capabilities = vec!["qa".to_string()];
        agents.insert("agent1".to_string(), cfg1);

        let lifecycle_map = HashMap::new();
        let orphaned = warn_if_last_active_agent("agent1", &agents, &lifecycle_map);
        assert_eq!(orphaned, vec!["qa".to_string()]);
    }

    #[test]
    fn warn_if_last_active_agent_no_orphan_when_backup_exists() {
        let mut agents = HashMap::new();
        let mut cfg1 = AgentConfig::new();
        cfg1.capabilities = vec!["qa".to_string()];
        agents.insert("agent1".to_string(), cfg1);

        let mut cfg2 = AgentConfig::new();
        cfg2.capabilities = vec!["qa".to_string()];
        agents.insert("agent2".to_string(), cfg2);

        let lifecycle_map = HashMap::new();
        let orphaned = warn_if_last_active_agent("agent1", &agents, &lifecycle_map);
        assert!(orphaned.is_empty());
    }

    #[test]
    fn get_lifecycle_state_defaults_to_active() {
        let lifecycle_map = HashMap::new();
        assert_eq!(
            get_lifecycle_state(&lifecycle_map, "unknown"),
            AgentLifecycleState::Active
        );
    }
}
