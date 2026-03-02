use serde::{Deserialize, Serialize};

/// Step hook engine type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepHookEngine {
    #[default]
    Cel,
}

/// Prehook UI mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepPrehookUiMode {
    Visual,
    Cel,
}

/// Prehook UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepPrehookUiConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<StepPrehookUiMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<serde_json::Value>,
}

/// Step prehook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepPrehookConfig {
    #[serde(default)]
    pub engine: StepHookEngine,
    pub when: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<StepPrehookUiConfig>,
    #[serde(default)]
    pub extended: bool,
}

/// Workflow finalize rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFinalizeRule {
    pub id: String,
    #[serde(default)]
    pub engine: StepHookEngine,
    pub when: String,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Workflow finalize configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowFinalizeConfig {
    #[serde(default)]
    pub rules: Vec<WorkflowFinalizeRule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_prehook_ui_config_default() {
        let cfg = StepPrehookUiConfig::default();
        assert!(cfg.mode.is_none());
        assert!(cfg.preset_id.is_none());
        assert!(cfg.expr.is_none());
    }

    #[test]
    fn test_step_hook_engine_default() {
        let engine = StepHookEngine::default();
        assert!(matches!(engine, StepHookEngine::Cel));
    }

    #[test]
    fn test_workflow_finalize_config_default_empty() {
        let cfg = WorkflowFinalizeConfig::default();
        assert!(cfg.rules.is_empty());
    }

    #[test]
    fn test_workflow_finalize_config_serde_round_trip() {
        let cfg = super::super::default_workflow_finalize_config();
        let json = serde_json::to_string(&cfg).expect("workflow finalize config should serialize");
        let cfg2: WorkflowFinalizeConfig =
            serde_json::from_str(&json).expect("workflow finalize config should deserialize");
        assert_eq!(cfg2.rules.len(), cfg.rules.len());
        assert_eq!(cfg2.rules[0].id, cfg.rules[0].id);
    }
}
