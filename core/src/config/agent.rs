use crate::cli_types::AgentEnvEntry;
use crate::metrics::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};

/// How the rendered prompt reaches the agent process.
///
/// - `Stdin`: prompt written to child stdin fd (zero shell risk)
/// - `File`: prompt written to temp file, `{prompt_file}` placeholder in command (near-zero risk)
/// - `Env`: prompt passed as `ORCH_PROMPT` env var (low risk)
/// - `Arg`: legacy `{prompt}` substitution in shell command (requires shell_escape, default)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptDelivery {
    Stdin,
    File,
    Env,
    #[default]
    Arg,
}

impl PromptDelivery {
    pub fn is_default(&self) -> bool {
        *self == Self::Arg
    }
}

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
    /// Command to execute (must contain {prompt} placeholder)
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub selection: AgentSelectionConfig,
    /// Environment variable entries (direct values and store references).
    /// Resolution happens at runtime via `resolve_agent_env()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<AgentEnvEntry>>,
    /// How the rendered prompt is delivered to the agent process.
    #[serde(default, skip_serializing_if = "PromptDelivery::is_default")]
    pub prompt_delivery: PromptDelivery,
}

impl AgentConfig {
    pub fn new() -> Self {
        Self {
            metadata: AgentMetadata::default(),
            capabilities: Vec::new(),
            command: String::new(),
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
        }
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
        assert!(cfg.command.is_empty());
        assert_eq!(cfg.metadata.name, "");
        assert!(cfg.metadata.description.is_none());
        assert!(cfg.metadata.version.is_none());
        assert!(cfg.metadata.cost.is_none());
        assert_eq!(cfg.prompt_delivery, PromptDelivery::Arg);

        let cfg2 = AgentConfig::new();
        assert!(cfg2.capabilities.is_empty());
        assert_eq!(cfg2.prompt_delivery, PromptDelivery::Arg);
    }

    #[test]
    fn prompt_delivery_default_is_arg() {
        assert_eq!(PromptDelivery::default(), PromptDelivery::Arg);
        assert!(PromptDelivery::Arg.is_default());
        assert!(!PromptDelivery::Stdin.is_default());
        assert!(!PromptDelivery::File.is_default());
        assert!(!PromptDelivery::Env.is_default());
    }

    #[test]
    fn prompt_delivery_serde_roundtrip() {
        for (variant, expected_str) in [
            (PromptDelivery::Stdin, "\"stdin\""),
            (PromptDelivery::File, "\"file\""),
            (PromptDelivery::Env, "\"env\""),
            (PromptDelivery::Arg, "\"arg\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let deserialized: PromptDelivery = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn prompt_delivery_skip_serializing_default() {
        let cfg = AgentConfig::new();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            !json.contains("prompt_delivery"),
            "default Arg should be omitted"
        );

        let mut cfg2 = AgentConfig::new();
        cfg2.prompt_delivery = PromptDelivery::Stdin;
        let json2 = serde_json::to_string(&cfg2).unwrap();
        assert!(
            json2.contains("prompt_delivery"),
            "non-default should be present"
        );
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
    fn test_agent_command_field() {
        let mut agent = AgentConfig::new();
        agent.command = "glmcode -p \"{prompt}\"".to_string();
        assert!(agent.command.contains("{prompt}"));
    }

    #[test]
    fn test_agent_selection_config_default() {
        let cfg = AgentSelectionConfig::default();
        assert!(cfg.weights.is_none());
    }
}
