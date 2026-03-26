use crate::cli_types::AgentEnvEntry;
use crate::selection::{SelectionStrategy, SelectionWeights};
use serde::{Deserialize, Serialize};

/// Configurable health/disease policy for an agent.
///
/// Controls how aggressively the scheduler marks agents as "diseased"
/// (temporarily unhealthy) after consecutive infrastructure failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthPolicyConfig {
    /// Hours to keep an agent in "diseased" state after threshold is hit.
    /// Set to 0 to disable disease entirely (agent always stays healthy).
    #[serde(default = "default_disease_duration_hours")]
    pub disease_duration_hours: u64,

    /// Number of consecutive infrastructure failures before marking diseased.
    #[serde(default = "default_disease_threshold")]
    pub disease_threshold: u32,

    /// Minimum per-capability success rate to remain schedulable while diseased.
    #[serde(default = "default_capability_success_threshold")]
    pub capability_success_threshold: f64,
}

fn default_disease_duration_hours() -> u64 {
    5
}

fn default_disease_threshold() -> u32 {
    2
}

fn default_capability_success_threshold() -> f64 {
    0.5
}

impl Default for HealthPolicyConfig {
    fn default() -> Self {
        Self {
            disease_duration_hours: default_disease_duration_hours(),
            disease_threshold: default_disease_threshold(),
            capability_success_threshold: default_capability_success_threshold(),
        }
    }
}

impl HealthPolicyConfig {
    /// Returns `true` when all fields match the global defaults.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How the rendered prompt reaches the agent process.
///
/// - `Stdin`: prompt written to child stdin fd (zero shell risk)
/// - `File`: prompt written to temp file, `{prompt_file}` placeholder in command (near-zero risk)
/// - `Env`: prompt passed as `ORCH_PROMPT` env var (low risk)
/// - `Arg`: `{prompt}` substitution in shell command (requires shell_escape, default)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptDelivery {
    /// Write the rendered prompt to stdin.
    Stdin,
    /// Write the rendered prompt to a temporary file.
    File,
    /// Pass the rendered prompt via environment variable.
    Env,
    /// Substitute the rendered prompt into the command arguments.
    #[default]
    Arg,
}

impl PromptDelivery {
    /// Returns `true` when this is the default prompt-delivery mode.
    pub fn is_default(&self) -> bool {
        *self == Self::Arg
    }
}

/// Agent metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMetadata {
    /// Stable agent name.
    pub name: String,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional agent version string.
    pub version: Option<String>,
    /// Optional static cost hint.
    pub cost: Option<u8>,
}

/// A conditional command rule evaluated via CEL at step execution time.
///
/// When an agent has `command_rules`, each rule is evaluated in order before
/// falling back to the default `command`. The first rule whose `when` expression
/// evaluates to `true` provides the command template for that step invocation.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AgentCommandRule {
    /// CEL expression that must evaluate to `true` for this rule to match.
    pub when: String,
    /// Command template to use when the rule matches.
    pub command: String,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    /// Metadata that describes the agent.
    pub metadata: AgentMetadata,
    /// Whether this agent is enabled for scheduling.
    /// Disabled agents are skipped during task dispatch.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    /// Capabilities advertised by the agent.
    pub capabilities: Vec<String>,
    /// Command to execute (must contain {prompt} placeholder)
    #[serde(default)]
    pub command: String,
    /// Conditional command rules evaluated in order via CEL.
    /// First matching rule's command is used; falls back to `command` if none match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_rules: Vec<AgentCommandRule>,
    #[serde(default)]
    /// Agent-selection policy and weights.
    pub selection: AgentSelectionConfig,
    /// Environment variable entries (direct values and store references).
    /// Resolution happens at runtime via `resolve_agent_env()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<AgentEnvEntry>>,
    /// How the rendered prompt is delivered to the agent process.
    #[serde(default, skip_serializing_if = "PromptDelivery::is_default")]
    pub prompt_delivery: PromptDelivery,
    /// Health/disease policy overrides for this agent.
    #[serde(default, skip_serializing_if = "HealthPolicyConfig::is_default")]
    pub health_policy: HealthPolicyConfig,
}

fn default_true() -> bool {
    true
}

impl AgentConfig {
    /// Creates an empty enabled agent configuration with defaults.
    pub fn new() -> Self {
        Self {
            metadata: AgentMetadata::default(),
            enabled: true,
            capabilities: Vec::new(),
            command: String::new(),
            command_rules: Vec::new(),
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
            health_policy: HealthPolicyConfig::default(),
        }
    }

    /// Returns `true` when the agent advertises the requested capability.
    pub fn supports_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(&capability.to_string())
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent selection configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSelectionConfig {
    /// Candidate-selection strategy.
    #[serde(default = "default_selection_strategy")]
    pub strategy: SelectionStrategy,
    /// Optional scoring weights for adaptive selection.
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

    #[test]
    fn command_rules_default_empty() {
        let cfg = AgentConfig::new();
        assert!(cfg.command_rules.is_empty());
    }

    #[test]
    fn command_rules_serde_roundtrip() {
        let mut cfg = AgentConfig::new();
        cfg.command_rules = vec![AgentCommandRule {
            when: "vars.loop_session_id != \"\"".to_string(),
            command: "claude --resume {loop_session_id} -p \"{prompt}\"".to_string(),
        }];
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("command_rules"));
        assert!(json.contains("loop_session_id"));

        let deserialized: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.command_rules.len(), 1);
        assert_eq!(deserialized.command_rules[0].when, cfg.command_rules[0].when);
        assert_eq!(
            deserialized.command_rules[0].command,
            cfg.command_rules[0].command
        );
    }

    #[test]
    fn command_rules_omitted_when_empty() {
        let cfg = AgentConfig::new();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            !json.contains("command_rules"),
            "empty command_rules should be omitted"
        );
    }
}
