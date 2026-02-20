use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityCategory {
    Execution,
    Analysis,
    Guard,
    Utility,
}

impl Default for CapabilityCategory {
    fn default() -> Self {
        Self::Execution
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDef {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub category: CapabilityCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
}

impl CapabilityDef {
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            category: CapabilityCategory::Execution,
            input_schema: None,
            output_schema: None,
        }
    }
}

pub fn default_capabilities() -> Vec<CapabilityDef> {
    vec![
        CapabilityDef::new("init_once", "Task initialization"),
        CapabilityDef::new("qa", "Execute QA tests"),
        CapabilityDef::new("ticket_scan", "Scan tickets"),
        CapabilityDef::new("fix", "Fix issues"),
        CapabilityDef::new("retest", "Re-run tests"),
        CapabilityDef::new("loop_guard", "Loop guard decision"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_capabilities() {
        let caps = default_capabilities();
        assert!(caps.iter().any(|c| c.id == "qa"));
        assert!(caps.iter().any(|c| c.id == "fix"));
    }

    #[test]
    fn test_capability_def_new() {
        let cap = CapabilityDef::new("test_cap", "A test capability");
        assert_eq!(cap.id, "test_cap");
    }
}
