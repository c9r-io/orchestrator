use crate::config::AgentConfig;
use crate::health::is_capability_healthy;
use crate::metrics::{calculate_agent_score, AgentHealthState, AgentMetrics, SelectionRequirement};
use anyhow::{anyhow, Result};
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
    if agents.is_empty() {
        return Err(anyhow!("no agents configured"));
    }

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
    let (agent_id, config) = agents
        .iter()
        .nth(idx)
        .ok_or_else(|| anyhow!("failed to select agent at random index {}", idx))?;
    let template = config
        .templates
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| "echo default".to_string());

    Ok((agent_id.clone(), template))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_agent(id: &str, capability: &str, cost: u8) -> (String, AgentConfig) {
        let mut cfg = AgentConfig::new();
        cfg.metadata.name = id.to_string();
        cfg.metadata.cost = Some(cost);
        cfg.capabilities = vec![capability.to_string()];
        cfg.templates
            .insert(capability.to_string(), format!("echo {}", id));
        (id.to_string(), cfg)
    }

    #[test]
    fn test_select_agent_advanced_finds_matching_capability() {
        let mut agents = HashMap::new();
        let (id1, cfg1) = make_test_agent("agent1", "qa", 30);
        let (id2, cfg2) = make_test_agent("agent2", "fix", 50);
        agents.insert(id1.clone(), cfg1);
        agents.insert(id2.clone(), cfg2);

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        let result = select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded);
        assert!(result.is_ok());
        let (agent_id, template) = result.expect("qa agent should be selected");
        assert_eq!(agent_id, "agent1");
        assert_eq!(template, "echo agent1");
    }

    #[test]
    fn test_select_agent_advanced_returns_error_when_no_match() {
        let mut agents = HashMap::new();
        let (_, cfg) = make_test_agent("agent1", "qa", 30);
        agents.insert("agent1".to_string(), cfg);

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        let result = select_agent_advanced("fix", &agents, &health_map, &metrics_map, &excluded);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("No healthy agent"));
    }

    #[test]
    fn test_select_agent_advanced_excludes_agents() {
        let mut agents = HashMap::new();
        let (_, cfg1) = make_test_agent("agent1", "qa", 30);
        let (_, cfg2) = make_test_agent("agent2", "qa", 40);
        agents.insert("agent1".to_string(), cfg1);
        agents.insert("agent2".to_string(), cfg2);

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let mut excluded = HashSet::new();
        excluded.insert("agent1".to_string());

        let result = select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded);
        assert!(result.is_ok());
        let (agent_id, _) = result.expect("remaining agent should be selected");
        assert_eq!(agent_id, "agent2");
    }

    #[test]
    fn test_select_agent_by_preference_returns_default() {
        let mut agents = HashMap::new();
        let mut cfg = AgentConfig::new();
        cfg.metadata.name = "default_agent".to_string();
        cfg.templates
            .insert("default".to_string(), "echo default template".to_string());
        agents.insert("default_agent".to_string(), cfg);

        let result = select_agent_by_preference(&agents);
        assert!(result.is_ok());
        let (agent_id, template) = result.expect("default agent should be returned");
        assert_eq!(agent_id, "default_agent");
        assert_eq!(template, "echo default template");
    }

    #[test]
    fn test_select_agent_by_preference_random_fallback() {
        let mut agents = HashMap::new();
        let mut cfg1 = AgentConfig::new();
        cfg1.metadata.name = "agent1".to_string();
        cfg1.capabilities = vec!["qa".to_string()];
        cfg1.templates
            .insert("qa".to_string(), "echo qa1".to_string());

        let mut cfg2 = AgentConfig::new();
        cfg2.metadata.name = "agent2".to_string();
        cfg2.capabilities = vec!["fix".to_string()];
        cfg2.templates
            .insert("fix".to_string(), "echo fix2".to_string());

        agents.insert("agent1".to_string(), cfg1);
        agents.insert("agent2".to_string(), cfg2);

        let result = select_agent_by_preference(&agents);
        assert!(result.is_ok());
        let (agent_id, _template) = result.expect("one agent should be selected");
        assert!(agent_id == "agent1" || agent_id == "agent2");
    }

    #[test]
    fn test_select_agent_by_preference_rejects_empty_registry() {
        let agents = HashMap::new();

        let result = select_agent_by_preference(&agents);

        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("no agents configured"));
    }
}
