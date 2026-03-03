use serde::{Deserialize, Serialize};

/// Step template configuration (runtime representation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTemplateConfig {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
