use serde::{Deserialize, Serialize};

/// Exact provider-neutral reaction match owned by a source task binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceTaskBindingMatchConfig {
    /// Normalized source event kind. V1 supports `reaction_added`.
    pub event_kind: String,
    /// Exact normalized reaction name without surrounding colons.
    pub reaction: String,
    /// Exact normalized target kind. V1 supports `message`.
    pub target_kind: String,
    /// Explicit channel allowlist.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    /// Explicit opt-in to every channel in the authenticated installation.
    #[serde(default)]
    pub all_channels: bool,
}

/// Project-scoped rule mapping authenticated source evidence to a task template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceTaskBindingConfig {
    /// Referenced Slack webhook Trigger.
    pub trigger_ref: String,
    /// Exact event match rule.
    pub match_rule: SourceTaskBindingMatchConfig,
    /// Referenced SourceTaskTemplate.
    pub template_ref: String,
    /// Trusted roles allowed to select this binding.
    pub allowed_actor_roles: Vec<String>,
    /// Whether this binding is excluded from matching.
    #[serde(default)]
    pub suspend: bool,
}
