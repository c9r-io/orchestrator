use crate::config::{AgentConfig, PromptDelivery};
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
) -> Result<(String, String, PromptDelivery)> {
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

    scored.sort_by(|a, b| {
        b.2.partial_cmp(&a.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });

    let top_count = std::cmp::min(3, scored.len());
    let top_slice = &scored[..top_count];

    let idx = rand::thread_rng().gen_range(0..top_slice.len());
    let (agent_id, config, _score) = top_slice[idx];

    Ok((
        agent_id.clone(),
        config.command.clone(),
        config.prompt_delivery,
    ))
}

pub fn select_agent_by_preference(
    agents: &HashMap<String, AgentConfig>,
) -> Result<(String, String, PromptDelivery)> {
    if agents.is_empty() {
        return Err(anyhow!("no agents configured"));
    }

    for (id, cfg) in agents {
        if cfg.capabilities.is_empty() || cfg.metadata.name == "default_agent" {
            let command = if cfg.command.is_empty() {
                "echo default".to_string()
            } else {
                cfg.command.clone()
            };
            return Ok((id.clone(), command, cfg.prompt_delivery));
        }
    }

    let idx = rand::thread_rng().gen_range(0..agents.len());
    let (agent_id, config) = agents
        .iter()
        .nth(idx)
        .ok_or_else(|| anyhow!("failed to select agent at random index {}", idx))?;
    let command = if config.command.is_empty() {
        "echo default".to_string()
    } else {
        config.command.clone()
    };

    Ok((agent_id.clone(), command, config.prompt_delivery))
}

/// Resolve effective agents for a task: project-scoped agents are **exclusive**
/// when the project has agents registered. No fallback to global agents.
/// This enforces project isolation — if a project lacks an agent with the
/// required capability, selection will fail with a clear error rather than
/// silently falling back to global agents (which may invoke paid AI services).
pub fn resolve_effective_agents<'a>(
    project_id: &str,
    config: &'a crate::config::OrchestratorConfig,
    _capability: Option<&str>,
) -> &'a HashMap<String, AgentConfig> {
    if !project_id.is_empty() {
        if let Some(project) = config.projects.get(project_id) {
            if !project.agents.is_empty() {
                return &project.agents;
            }
        }
    }
    &config.agents
}

/// Resolve an agent by ID. When a project has agents registered, only look
/// in the project scope — never fall back to global agents.
pub fn resolve_agent_by_id<'a>(
    project_id: &str,
    config: &'a crate::config::OrchestratorConfig,
    agent_id: &str,
) -> Option<&'a AgentConfig> {
    if !project_id.is_empty() {
        if let Some(project) = config.projects.get(project_id) {
            if !project.agents.is_empty() {
                return project.agents.get(agent_id);
            }
        }
    }
    config.agents.get(agent_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_agent(id: &str, capability: &str, cost: u8) -> (String, AgentConfig) {
        let mut cfg = AgentConfig::new();
        cfg.metadata.name = id.to_string();
        cfg.metadata.cost = Some(cost);
        cfg.capabilities = vec![capability.to_string()];
        cfg.command = format!("echo {}", id);
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
        let (agent_id, template, _) = result.expect("qa agent should be selected");
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
        let (agent_id, _, _) = result.expect("remaining agent should be selected");
        assert_eq!(agent_id, "agent2");
    }

    #[test]
    fn test_select_agent_by_preference_returns_default() {
        let mut agents = HashMap::new();
        let mut cfg = AgentConfig::new();
        cfg.metadata.name = "default_agent".to_string();
        cfg.command = "echo default template".to_string();
        agents.insert("default_agent".to_string(), cfg);

        let result = select_agent_by_preference(&agents);
        assert!(result.is_ok());
        let (agent_id, command, _) = result.expect("default agent should be returned");
        assert_eq!(agent_id, "default_agent");
        assert_eq!(command, "echo default template");
    }

    #[test]
    fn test_select_agent_by_preference_random_fallback() {
        let mut agents = HashMap::new();
        let mut cfg1 = AgentConfig::new();
        cfg1.metadata.name = "agent1".to_string();
        cfg1.capabilities = vec!["qa".to_string()];
        cfg1.command = "echo qa1".to_string();

        let mut cfg2 = AgentConfig::new();
        cfg2.metadata.name = "agent2".to_string();
        cfg2.capabilities = vec!["fix".to_string()];
        cfg2.command = "echo fix2".to_string();

        agents.insert("agent1".to_string(), cfg1);
        agents.insert("agent2".to_string(), cfg2);

        let result = select_agent_by_preference(&agents);
        assert!(result.is_ok());
        let (agent_id, _command, _) = result.expect("one agent should be selected");
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

    #[test]
    fn test_cost_differential_lower_cost_scores_higher() {
        use crate::metrics::SelectionStrategy;
        // 4 agents all with "qa". Costs: 1, 30, 60, 99.
        // With CapabilityAware, cost_score = 100 - cost, weighted 0.15.
        // Without metrics, success_rate=50 and perf=50 are equal for all agents,
        // so cost is the only differentiator.
        // Agent with cost=99 (total ~9.45) should never make the top-3 cut
        // because agents with cost 1/30/60 all score higher.
        let mut agents = HashMap::new();
        for (name, cost) in [("best", 1u8), ("mid1", 30), ("mid2", 60), ("worst", 99)] {
            let (id, mut cfg) = make_test_agent(name, "qa", cost);
            cfg.selection.strategy = SelectionStrategy::CapabilityAware;
            agents.insert(id, cfg);
        }

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        for _ in 0..20 {
            let (agent_id, _, _) =
                select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded)
                    .expect("should select an agent");
            assert_ne!(
                agent_id, "worst",
                "highest-cost agent should be excluded from top-3"
            );
        }
    }

    #[test]
    fn test_metrics_impact_high_success_rate_preferred() {
        use crate::metrics::SelectionStrategy;
        // 4 agents with same cost=50 and capability "qa", strategy=CapabilityAware.
        // Agent "good" has high success rate + low duration.
        // Agent "bad" has terrible metrics and should rank last (outside top-3).
        let mut agents = HashMap::new();
        for name in ["good", "neutral1", "neutral2", "bad"] {
            let (id, mut cfg) = make_test_agent(name, "qa", 50);
            cfg.selection.strategy = SelectionStrategy::CapabilityAware;
            agents.insert(id, cfg);
        }

        let health_map = HashMap::new();
        let mut metrics_map = HashMap::new();
        metrics_map.insert(
            "good".to_string(),
            AgentMetrics {
                total_runs: 100,
                successful_runs: 99,
                avg_duration_ms: 1000,
                ..Default::default()
            },
        );
        metrics_map.insert(
            "bad".to_string(),
            AgentMetrics {
                total_runs: 100,
                successful_runs: 5,
                avg_duration_ms: 59000,
                ..Default::default()
            },
        );
        let excluded = HashSet::new();

        for _ in 0..20 {
            let (agent_id, _, _) =
                select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded)
                    .expect("should select an agent");
            assert_ne!(
                agent_id, "bad",
                "agent with terrible metrics should be excluded from top-3"
            );
        }
    }

    #[test]
    fn test_health_penalty_consecutive_errors_lowers_score() {
        use crate::metrics::SelectionStrategy;
        // 4 agents all with "qa" and same cost=30, strategy=CapabilityAware.
        // "sick" has consecutive_errors=5 (penalty = -75, capped at -50 in CapabilityAware).
        // This should push "sick" below the other 3 agents in score, excluding it from top-3.
        let mut agents = HashMap::new();
        for name in ["healthy1", "healthy2", "healthy3", "sick"] {
            let (id, mut cfg) = make_test_agent(name, "qa", 30);
            cfg.selection.strategy = SelectionStrategy::CapabilityAware;
            agents.insert(id, cfg);
        }

        let mut health_map = HashMap::new();
        health_map.insert(
            "sick".to_string(),
            AgentHealthState {
                consecutive_errors: 5,
                ..Default::default()
            },
        );
        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        for _ in 0..20 {
            let (agent_id, _, _) =
                select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded)
                    .expect("should select an agent");
            assert_ne!(
                agent_id, "sick",
                "agent with consecutive errors should be excluded from top-3"
            );
        }
    }

    #[test]
    fn test_all_candidates_excluded_returns_error() {
        let mut agents = HashMap::new();
        let (id_a, cfg_a) = make_test_agent("agent1", "qa", 30);
        let (id_b, cfg_b) = make_test_agent("agent2", "qa", 40);
        agents.insert(id_a.clone(), cfg_a);
        agents.insert(id_b.clone(), cfg_b);

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let mut excluded = HashSet::new();
        excluded.insert(id_a);
        excluded.insert(id_b);

        let result = select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded);
        assert!(result.is_err());
        assert!(result
            .expect_err("should fail when all excluded")
            .to_string()
            .contains("No healthy agent"));
    }

    #[test]
    fn test_single_candidate_deterministic() {
        let mut agents = HashMap::new();
        let (id, cfg) = make_test_agent("solo_agent", "qa", 50);
        agents.insert(id, cfg);

        let health_map = HashMap::new();
        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        for _ in 0..10 {
            let (agent_id, command, _) =
                select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded)
                    .expect("should select the only agent");
            assert_eq!(agent_id, "solo_agent");
            assert_eq!(command, "echo solo_agent");
        }
    }

    #[test]
    fn test_preference_empty_capabilities_non_default_name() {
        let mut agents = HashMap::new();
        let mut cfg = AgentConfig::new();
        cfg.metadata.name = "custom_blank".to_string();
        cfg.capabilities = vec![];
        cfg.command = "echo custom_blank".to_string();
        agents.insert("custom_blank".to_string(), cfg);

        let result = select_agent_by_preference(&agents);
        assert!(result.is_ok());
        let (agent_id, command, _) = result.expect("empty-capabilities agent should match");
        assert_eq!(agent_id, "custom_blank");
        assert_eq!(command, "echo custom_blank");
    }

    #[test]
    fn test_diseased_agent_filtered_from_candidates() {
        use chrono::{Duration, Utc};

        let mut agents = HashMap::new();
        let (id_sick, cfg_sick) = make_test_agent("sick_agent", "qa", 30);
        let (id_ok, cfg_ok) = make_test_agent("ok_agent", "qa", 30);
        agents.insert(id_sick.clone(), cfg_sick);
        agents.insert(id_ok.clone(), cfg_ok);

        let mut health_map = HashMap::new();
        health_map.insert(
            id_sick,
            AgentHealthState {
                diseased_until: Some(Utc::now() + Duration::hours(1)),
                capability_health: HashMap::new(), // no capability data → is_capability_healthy returns false
                ..Default::default()
            },
        );

        let metrics_map = HashMap::new();
        let excluded = HashSet::new();

        for _ in 0..10 {
            let (agent_id, _, _) =
                select_agent_advanced("qa", &agents, &health_map, &metrics_map, &excluded)
                    .expect("healthy agent should be selected");
            assert_eq!(
                agent_id, "ok_agent",
                "diseased agent should be filtered out"
            );
        }
    }

    // ── resolve_effective_agents tests ─────────────────────────────────

    fn make_config_with_project_agents() -> crate::config::OrchestratorConfig {
        let mut config = crate::config::OrchestratorConfig::default();

        // Global agents
        let (id_global, cfg_global) = make_test_agent("global_qa", "qa", 30);
        config.agents.insert(id_global, cfg_global);
        let (id_fix, cfg_fix) = make_test_agent("global_fix", "fix", 40);
        config.agents.insert(id_fix, cfg_fix);

        // Project agents (only qa capability)
        let mut project = crate::config::ProjectConfig {
            description: None,
            workspaces: HashMap::new(),
            agents: HashMap::new(),
            workflows: HashMap::new(),
        };
        let (id_proj, cfg_proj) = make_test_agent("proj_qa", "qa", 20);
        project.agents.insert(id_proj, cfg_proj);
        config.projects.insert("my-project".to_string(), project);

        config
    }

    #[test]
    fn resolve_effective_agents_returns_project_agents_when_capability_matches() {
        let config = make_config_with_project_agents();
        let agents = resolve_effective_agents("my-project", &config, Some("qa"));
        assert!(agents.contains_key("proj_qa"));
        assert!(!agents.contains_key("global_qa"));
    }

    #[test]
    fn resolve_effective_agents_stays_in_project_even_when_capability_missing() {
        let config = make_config_with_project_agents();
        // Project has no "fix" agent — but should NOT fall back to global.
        // Returns project agents; selection will fail with a clear error.
        let agents = resolve_effective_agents("my-project", &config, Some("fix"));
        assert!(agents.contains_key("proj_qa"));
        assert!(!agents.contains_key("global_fix"));
    }

    #[test]
    fn resolve_effective_agents_returns_global_for_empty_project_id() {
        let config = make_config_with_project_agents();
        let agents = resolve_effective_agents("", &config, Some("qa"));
        assert!(agents.contains_key("global_qa"));
        assert!(!agents.contains_key("proj_qa"));
    }

    #[test]
    fn resolve_effective_agents_returns_project_agents_when_no_capability_required() {
        let config = make_config_with_project_agents();
        // No capability filter — project has agents, use them
        let agents = resolve_effective_agents("my-project", &config, None);
        assert!(agents.contains_key("proj_qa"));
        assert!(!agents.contains_key("global_qa"));
    }

    #[test]
    fn resolve_effective_agents_returns_global_for_unknown_project() {
        let config = make_config_with_project_agents();
        let agents = resolve_effective_agents("no-such-project", &config, Some("qa"));
        assert!(agents.contains_key("global_qa"));
    }

    #[test]
    fn resolve_effective_agents_returns_global_for_empty_project_agents() {
        let mut config = make_config_with_project_agents();
        // Create project with no agents
        config.projects.insert(
            "empty-proj".to_string(),
            crate::config::ProjectConfig {
                description: None,
                workspaces: HashMap::new(),
                agents: HashMap::new(),
                workflows: HashMap::new(),
            },
        );
        let agents = resolve_effective_agents("empty-proj", &config, Some("qa"));
        assert!(agents.contains_key("global_qa"));
    }
}
