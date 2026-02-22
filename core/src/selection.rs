use crate::config::AgentConfig;
use crate::health::is_capability_healthy;
use crate::metrics::{
    calculate_agent_score, AgentHealthState, AgentMetrics, SelectionRequirement, SelectionWeights,
};
use crate::state::InnerState;
use anyhow::Result;
use rand::Rng;
use std::collections::{HashMap, HashSet};

pub fn select_agent_advanced(
    capability: &str,
    agents: &HashMap<String, AgentConfig>,
    health_map: &HashMap<String, AgentHealthState>,
    metrics_map: &HashMap<String, AgentMetrics>,
    excluded_agents: &HashSet<String>,
) -> Result<(String, String)> {
    let candidates: Vec<_> = agents
        .iter()
        .filter(|(id, cfg)| {
            if excluded_agents.contains(*id) {
                return false;
            }
            if !cfg.supports_capability(capability) {
                return false;
            }
            is_capability_healthy(health_map, id, capability)
        })
        .collect();

    if candidates.is_empty() {
        anyhow::bail!("No healthy agent found with capability: {}", capability);
    }

    let mut scored: Vec<_> = candidates
        .iter()
        .map(|(id, cfg)| {
            let health = health_map.get(*id);
            let metrics = metrics_map.get(*id);
            let cost_u32 = cfg.metadata.cost.map(|c| c as u32);

            let (strategy, weights) = {
                let sel = &cfg.selection;
                let s = sel.strategy;
                let w = sel.weights.clone().unwrap_or_default();
                (s, w)
            };

            let requirement = SelectionRequirement {
                capability: capability.to_string(),
                strategy,
                weights,
                max_load: 5,
                consider_health: true,
                capability_aware: true,
            };

            let score = calculate_agent_score(
                id,
                cost_u32,
                &metrics.cloned(),
                &health.cloned(),
                &requirement,
            );
            (*id, cfg, score.total_score)
        })
        .collect();

    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let top_count = std::cmp::min(3, scored.len());
    let top_slice = &scored[..top_count];

    let idx = rand::thread_rng().gen_range(0..top_slice.len());
    let (agent_id, config, _score) = top_slice[idx];

    let template = config
        .get_template(capability)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Agent {} has capability {} but no template",
                agent_id,
                capability
            )
        })?
        .clone();

    Ok((agent_id.clone(), template))
}

pub fn select_agent_by_preference(
    agents: &HashMap<String, AgentConfig>,
) -> Result<(String, String)> {
    for (id, cfg) in agents {
        if cfg.capabilities.is_empty() || cfg.metadata.name == "default_agent" {
            let template = cfg
                .templates
                .get("default")
                .cloned()
                .unwrap_or_else(|| "echo default".to_string());
            return Ok((id.clone(), template));
        }
    }

    let idx = rand::thread_rng().gen_range(0..agents.len());
    let (agent_id, config) = agents.iter().nth(idx).unwrap();
    let template = config
        .templates
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| "echo default".to_string());

    Ok((agent_id.clone(), template))
}
