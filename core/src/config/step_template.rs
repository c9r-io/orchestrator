use serde::{Deserialize, Serialize};

/// Step template configuration (runtime representation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTemplateConfig {
    /// Prompt or command template body.
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional human-readable description of the template.
    pub description: Option<String>,
}
