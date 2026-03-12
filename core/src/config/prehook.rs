use serde::{Deserialize, Serialize};

/// Step hook engine type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepHookEngine {
    /// Evaluate hooks with the CEL expression engine.
    #[default]
    Cel,
}

/// Prehook UI mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepPrehookUiMode {
    /// Use the visual builder representation.
    Visual,
    /// Use a raw CEL expression editor.
    Cel,
}

/// Prehook UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepPrehookUiConfig {
    /// Preferred UI editing mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<StepPrehookUiMode>,
    /// Optional preset identifier selected in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    /// Serialized UI expression payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<serde_json::Value>,
}

/// Step prehook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepPrehookConfig {
    /// Expression engine used to evaluate the prehook.
    #[serde(default)]
    pub engine: StepHookEngine,
    /// Expression that decides whether the step should run.
    pub when: String,
    /// Optional human-readable explanation shown when the hook matches.
    #[serde(default)]
    pub reason: Option<String>,
    /// UI metadata used by manifest editors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<StepPrehookUiConfig>,
    /// Enables extended context fields during evaluation.
    #[serde(default)]
    pub extended: bool,
}

/// Workflow finalize rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFinalizeRule {
    /// Stable rule identifier.
    pub id: String,
    /// Expression engine used to evaluate the rule.
    #[serde(default)]
    pub engine: StepHookEngine,
    /// Expression that decides whether the rule matches.
    pub when: String,
    /// Status written when the rule matches.
    pub status: String,
    /// Optional human-readable explanation for the outcome.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Workflow finalize configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowFinalizeConfig {
    /// Ordered finalize rules evaluated after task-item execution.
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
