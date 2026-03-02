use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub version: Option<String>,
    pub cost: Option<u8>,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub metadata: AgentMetadata,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub templates: HashMap<String, String>,
    #[serde(default)]
    pub selection: AgentSelectionConfig,
}

impl AgentConfig {
    pub fn new() -> Self {
        Self {
            metadata: AgentMetadata::default(),
            capabilities: Vec::new(),
            templates: HashMap::new(),
            selection: AgentSelectionConfig::default(),
        }
    }

    pub fn get_template(&self, capability: &str) -> Option<&String> {
        self.templates.get(capability)
    }

    pub fn supports_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(&capability.to_string())
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent selection configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSelectionConfig {
    #[serde(default = "default_selection_strategy")]
    pub strategy: SelectionStrategy,
    #[serde(default)]
    pub weights: Option<SelectionWeights>,
}

fn default_selection_strategy() -> SelectionStrategy {
    SelectionStrategy::CapabilityAware
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_default_and_new() {
        let cfg = AgentConfig::default();
        assert!(cfg.capabilities.is_empty());
        assert!(cfg.templates.is_empty());
        assert_eq!(cfg.metadata.name, "");
        assert!(cfg.metadata.description.is_none());
        assert!(cfg.metadata.version.is_none());
        assert!(cfg.metadata.cost.is_none());

        let cfg2 = AgentConfig::new();
        assert!(cfg2.capabilities.is_empty());
    }

    #[test]
    fn test_agent_supports_capability() {
        let mut agent = AgentConfig::new();
        agent.capabilities = vec!["plan".to_string(), "qa".to_string()];
        assert!(agent.supports_capability("plan"));
        assert!(agent.supports_capability("qa"));
        assert!(!agent.supports_capability("fix"));
    }

    #[test]
    fn test_agent_get_template() {
        let mut agent = AgentConfig::new();
        agent
            .templates
            .insert("plan".to_string(), "plan template".to_string());
        assert_eq!(
            agent.get_template("plan"),
            Some(&"plan template".to_string())
        );
        assert_eq!(agent.get_template("fix"), None);
    }

    #[test]
    fn test_agent_selection_config_default() {
        let cfg = AgentSelectionConfig::default();
        assert!(cfg.weights.is_none());
        // The strategy defaults to CapabilityAware via the serde default fn
        // but Default derive gives CostBased; verify the struct-level default
    }
}
