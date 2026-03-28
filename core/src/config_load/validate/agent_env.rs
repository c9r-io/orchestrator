use crate::config::{AgentConfig, EnvStoreConfig, OrchestratorConfig, SecretStoreConfig};
use anyhow::Result;
use std::collections::HashMap;

/// Validate env store refs for a set of agents within a single project.
fn validate_env_store_refs_for_agents(
    agents: &HashMap<String, AgentConfig>,
    env_stores: &HashMap<String, EnvStoreConfig>,
    secret_stores: &HashMap<String, SecretStoreConfig>,
    project_id: &str,
) -> Result<()> {
    for (agent_name, agent_cfg) in agents {
        if let Some(ref entries) = agent_cfg.env {
            for entry in entries {
                if let Some(ref store_name) = entry.from_ref {
                    if !env_stores.contains_key(store_name.as_str())
                        && !secret_stores.contains_key(store_name.as_str())
                    {
                        anyhow::bail!(
                            "agent '{}'(project '{}') env fromRef '{}' references unknown store",
                            agent_name,
                            project_id,
                            store_name
                        );
                    }
                }
                if let Some(ref rv) = entry.ref_value {
                    if !env_stores.contains_key(&rv.name) && !secret_stores.contains_key(&rv.name) {
                        anyhow::bail!(
                            "agent '{}'(project '{}') env refValue.name '{}' references unknown store",
                            agent_name,
                            project_id,
                            rv.name
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Validates that all agent env store references (fromRef, refValue.name) point to
/// existing entries in config.env_stores or config.secret_stores.
pub fn validate_agent_env_store_refs(config: &OrchestratorConfig) -> Result<()> {
    for (project_id, project) in &config.projects {
        validate_env_store_refs_for_agents(
            &project.agents,
            &project.env_stores,
            &project.secret_stores,
            project_id,
        )?;
    }
    Ok(())
}

/// Like `validate_agent_env_store_refs` but only validates agents in the given project.
pub fn validate_agent_env_store_refs_for_project(
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<()> {
    if let Some(project) = config.projects.get(project_id) {
        validate_env_store_refs_for_agents(
            &project.agents,
            &project.env_stores,
            &project.secret_stores,
            project_id,
        )?;
    }
    Ok(())
}

/// Validates all agent `command_rules` CEL expressions and prompt placeholders.
pub fn validate_agent_command_rules(config: &OrchestratorConfig) -> Result<()> {
    for project in config.projects.values() {
        for (agent_id, agent_cfg) in &project.agents {
            crate::prehook::validate_agent_command_rules(agent_id, &agent_cfg.command_rules)?;
        }
    }
    Ok(())
}

/// Like `validate_agent_command_rules` but scoped to a single project.
pub fn validate_agent_command_rules_for_project(
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<()> {
    if let Some(project) = config.projects.get(project_id) {
        for (agent_id, agent_cfg) in &project.agents {
            crate::prehook::validate_agent_command_rules(agent_id, &agent_cfg.command_rules)?;
        }
    }
    Ok(())
}
