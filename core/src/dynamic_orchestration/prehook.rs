use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Extended prehook decision that supports dynamic orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", content = "data")]
#[derive(Default)]
pub enum PrehookDecision {
    /// Execute the step (default behavior)
    #[default]
    Run,
    /// Skip the step with a reason
    Skip {
        #[serde(default)]
        reason: String,
    },
    /// Branch to a different step
    Branch {
        /// Target step ID to jump to
        target: String,
        /// Context to pass to the target step
        #[serde(default)]
        context: HashMap<String, serde_json::Value>,
    },
    /// Dynamically add new steps to the execution plan
    DynamicAdd {
        /// Steps to add
        steps: Vec<DynamicStepInstance>,
    },
    /// Transform/replace the template for subsequent steps
    Transform {
        /// New template content
        template: String,
        /// Which step types to apply the transform to
        #[serde(default)]
        target_steps: Vec<String>,
    },
}

impl From<bool> for PrehookDecision {
    fn from(should_run: bool) -> Self {
        if should_run {
            Self::Run
        } else {
            Self::Skip {
                reason: "Condition evaluated to false".to_string(),
            }
        }
    }
}

impl PrehookDecision {
    /// Returns true if the step should be executed
    pub fn should_run(&self) -> bool {
        matches!(
            self,
            Self::Run | Self::DynamicAdd { .. } | Self::Transform { .. }
        )
    }

    /// Returns true if this decision involves branching
    pub fn is_branch(&self) -> bool {
        matches!(self, Self::Branch { .. })
    }

    /// Returns true if this decision adds dynamic steps
    pub fn is_dynamic_add(&self) -> bool {
        matches!(self, Self::DynamicAdd { .. })
    }

    /// Returns true if this decision transforms templates
    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }
}

/// A dynamic step instance created at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicStepInstance {
    /// Unique identifier for this step instance
    pub id: String,
    /// Reference to the dynamic step definition
    pub source_id: String,
    /// The step type
    pub step_type: String,
    /// Agent ID to use (if applicable)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Template to execute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Additional context for this step
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prehook_decision_from_bool() {
        assert!(PrehookDecision::from(true).should_run());
        assert!(!PrehookDecision::from(false).should_run());
    }

    #[test]
    fn test_prehook_decision_branch() {
        let decision = PrehookDecision::Branch {
            target: "fix".to_string(),
            context: HashMap::new(),
        };
        assert!(decision.is_branch());
        assert!(!decision.should_run());
    }

    #[test]
    fn test_prehook_decision_dynamic_add() {
        let step = DynamicStepInstance {
            id: "dynamic_fix_1".to_string(),
            source_id: "quick_fix".to_string(),
            step_type: "fix".to_string(),
            agent_id: Some("quick_fixer".to_string()),
            template: Some("fix {rel_path}".to_string()),
            context: HashMap::new(),
        };
        let decision = PrehookDecision::DynamicAdd { steps: vec![step] };
        assert!(decision.is_dynamic_add());
        assert!(decision.should_run());
    }

    #[test]
    fn test_prehook_decision_transform() {
        let decision = PrehookDecision::Transform {
            template: "new_template {rel_path}".to_string(),
            target_steps: vec!["fix".to_string()],
        };
        assert!(decision.is_transform());
        assert!(decision.should_run());
    }

    #[test]
    fn test_prehook_decision_default_is_run() {
        let decision = PrehookDecision::default();
        assert!(decision.should_run());
        assert!(!decision.is_branch());
        assert!(!decision.is_dynamic_add());
        assert!(!decision.is_transform());
    }

    #[test]
    fn test_prehook_decision_skip_does_not_run() {
        let decision = PrehookDecision::Skip {
            reason: "test reason".to_string(),
        };
        assert!(!decision.should_run());
        assert!(!decision.is_branch());
        assert!(!decision.is_dynamic_add());
        assert!(!decision.is_transform());
    }

    #[test]
    fn test_prehook_decision_serde_run() {
        let decision = PrehookDecision::Run;
        let json = serde_json::to_string(&decision).unwrap();
        let parsed: PrehookDecision = serde_json::from_str(&json).unwrap();
        assert!(parsed.should_run());
    }

    #[test]
    fn test_prehook_decision_serde_skip() {
        let json = r#"{"action":"Skip","data":{"reason":"no need"}}"#;
        let decision: PrehookDecision = serde_json::from_str(json).unwrap();
        assert!(!decision.should_run());
    }

    #[test]
    fn test_prehook_decision_serde_branch() {
        let json = r#"{"action":"Branch","data":{"target":"fix","context":{}}}"#;
        let decision: PrehookDecision = serde_json::from_str(json).unwrap();
        assert!(decision.is_branch());
    }
}
