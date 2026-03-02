use serde::{Deserialize, Serialize};

use super::WorkflowStepConfig;

/// Execution scope for a workflow step
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepScope {
    /// Runs once per cycle (plan, implement, self_test, align_tests, doc_governance)
    Task,
    /// Runs per item/QA file (qa_testing, ticket_fix)
    #[default]
    Item,
}

// ── Step Behavior declarations ─────────────────────────────────────

/// Declarative behavior attached to each workflow step.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepBehavior {
    #[serde(default)]
    pub on_failure: OnFailureAction,
    #[serde(default)]
    pub on_success: OnSuccessAction,
    #[serde(default)]
    pub captures: Vec<CaptureDecl>,
    #[serde(default)]
    pub post_actions: Vec<PostAction>,
    #[serde(default)]
    pub execution: ExecutionMode,
    #[serde(default)]
    pub collect_artifacts: bool,
}

/// What to do when a step fails.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnFailureAction {
    #[default]
    Continue,
    SetStatus {
        status: String,
    },
    EarlyReturn {
        status: String,
    },
}

/// What to do when a step succeeds.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnSuccessAction {
    #[default]
    Continue,
    SetStatus {
        status: String,
    },
}

/// A single capture declaration: what to extract from a step result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureDecl {
    pub var: String,
    pub source: CaptureSource,
}

/// Source of a captured value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    Stdout,
    Stderr,
    ExitCode,
    FailedFlag,
    SuccessFlag,
}

/// Post-step action to run after a step completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PostAction {
    CreateTicket,
    ScanTickets,
}

/// How a step is executed.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExecutionMode {
    #[default]
    Agent,
    Builtin {
        name: String,
    },
    Chain,
}

/// Resolved semantic meaning for a workflow step after applying defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSemanticKind {
    Builtin { name: String },
    Agent { capability: String },
    Command,
    Chain,
}

/// Cost preference enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CostPreference {
    Performance,
    Quality,
    #[default]
    Balance,
}

/// Known workflow step IDs
const KNOWN_STEP_IDS: &[&str] = &[
    "init_once",
    "plan",
    "qa",
    "ticket_scan",
    "fix",
    "retest",
    "loop_guard",
    "build",
    "test",
    "lint",
    "implement",
    "review",
    "git_ops",
    "qa_doc_gen",
    "qa_testing",
    "ticket_fix",
    "doc_governance",
    "align_tests",
    "self_test",
    "smoke_chain",
];

const KNOWN_BUILTIN_STEP_NAMES: &[&str] = &["init_once", "loop_guard", "ticket_scan", "self_test"];

/// Validate that a step type string is a known step ID.
pub fn validate_step_type(value: &str) -> Result<String, String> {
    if KNOWN_STEP_IDS.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!("unknown workflow step type: {}", value))
    }
}

pub fn is_known_builtin_step_name(value: &str) -> bool {
    KNOWN_BUILTIN_STEP_NAMES.contains(&value)
}

pub fn default_builtin_for_step_id(step_id: &str) -> Option<&'static str> {
    match step_id {
        "init_once" => Some("init_once"),
        "loop_guard" => Some("loop_guard"),
        "ticket_scan" => Some("ticket_scan"),
        "self_test" => Some("self_test"),
        _ => None,
    }
}

pub fn default_required_capability_for_step_id(step_id: &str) -> Option<&'static str> {
    match step_id {
        "qa" => Some("qa"),
        "fix" => Some("fix"),
        "retest" => Some("retest"),
        "plan" => Some("plan"),
        "build" => Some("build"),
        "test" => Some("test"),
        "lint" => Some("lint"),
        "implement" => Some("implement"),
        "review" => Some("review"),
        "git_ops" => Some("git_ops"),
        "qa_doc_gen" => Some("qa_doc_gen"),
        "qa_testing" => Some("qa_testing"),
        "ticket_fix" => Some("ticket_fix"),
        "doc_governance" => Some("doc_governance"),
        "align_tests" => Some("align_tests"),
        "smoke_chain" => Some("smoke_chain"),
        _ => None,
    }
}

pub fn resolve_step_semantic_kind(step: &WorkflowStepConfig) -> Result<StepSemanticKind, String> {
    if step.builtin.is_some() && step.required_capability.is_some() {
        return Err(format!(
            "step '{}' cannot define both builtin and required_capability",
            step.id
        ));
    }

    if !step.chain_steps.is_empty() {
        return Ok(StepSemanticKind::Chain);
    }

    if step.command.is_some() {
        return Ok(StepSemanticKind::Command);
    }

    if let Some(ref builtin) = step.builtin {
        if !is_known_builtin_step_name(builtin) {
            return Err(format!(
                "step '{}' uses unknown builtin '{}'",
                step.id, builtin
            ));
        }
        return Ok(StepSemanticKind::Builtin {
            name: builtin.clone(),
        });
    }

    if let Some(ref capability) = step.required_capability {
        return Ok(StepSemanticKind::Agent {
            capability: capability.clone(),
        });
    }

    if let Some(builtin) = default_builtin_for_step_id(&step.id) {
        return Ok(StepSemanticKind::Builtin {
            name: builtin.to_string(),
        });
    }

    if let Some(capability) = default_required_capability_for_step_id(&step.id) {
        return Ok(StepSemanticKind::Agent {
            capability: capability.to_string(),
        });
    }

    Err(format!(
        "step '{}' is missing builtin, required_capability, command, or chain_steps",
        step.id
    ))
}

pub fn normalize_step_execution_mode(step: &mut WorkflowStepConfig) -> Result<(), String> {
    match resolve_step_semantic_kind(step)? {
        StepSemanticKind::Builtin { name } => {
            step.builtin = Some(name.clone());
            step.required_capability = None;
            step.behavior.execution = ExecutionMode::Builtin { name };
        }
        StepSemanticKind::Agent { capability } => {
            step.required_capability = Some(capability);
            step.behavior.execution = ExecutionMode::Agent;
        }
        StepSemanticKind::Command => {
            step.behavior.execution = ExecutionMode::Builtin {
                name: step.id.clone(),
            };
        }
        StepSemanticKind::Chain => {
            step.behavior.execution = ExecutionMode::Chain;
        }
    }
    Ok(())
}

/// Returns true if a step ID produces structured output for pipeline variables
pub fn has_structured_output(step_id: &str) -> bool {
    matches!(
        step_id,
        "build" | "test" | "lint" | "qa_testing" | "self_test" | "smoke_chain"
    )
}

/// Returns the default execution scope for a step ID.
/// Task-scoped steps run once per cycle; item-scoped steps fan-out per QA file.
pub fn default_scope_for_step_id(step_id: &str) -> StepScope {
    match step_id {
        // Item-scoped: fan-out per QA file
        "qa" | "qa_testing" | "ticket_fix" | "ticket_scan" | "fix" | "retest" => StepScope::Item,
        // Everything else defaults to task-scoped
        _ => StepScope::Task,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_step_type_known_ids() {
        for id in &[
            "init_once",
            "plan",
            "qa",
            "ticket_scan",
            "fix",
            "retest",
            "loop_guard",
            "build",
            "test",
            "lint",
            "implement",
            "review",
            "git_ops",
            "qa_doc_gen",
            "qa_testing",
            "ticket_fix",
            "doc_governance",
            "align_tests",
            "self_test",
            "smoke_chain",
        ] {
            assert!(validate_step_type(id).is_ok(), "expected valid for {}", id);
        }
    }

    #[test]
    fn test_validate_step_type_unknown_id() {
        let result = validate_step_type("my_custom_step");
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .contains("unknown workflow step type"));
    }

    #[test]
    fn test_has_structured_output() {
        assert!(has_structured_output("build"));
        assert!(has_structured_output("test"));
        assert!(has_structured_output("lint"));
        assert!(has_structured_output("qa_testing"));
        assert!(has_structured_output("self_test"));
        assert!(has_structured_output("smoke_chain"));

        assert!(!has_structured_output("plan"));
        assert!(!has_structured_output("fix"));
        assert!(!has_structured_output("implement"));
        assert!(!has_structured_output("review"));
        assert!(!has_structured_output("qa"));
        assert!(!has_structured_output("doc_governance"));
    }

    #[test]
    fn test_default_scope_task_steps() {
        let task_scoped = vec![
            "plan",
            "qa_doc_gen",
            "implement",
            "self_test",
            "align_tests",
            "doc_governance",
            "review",
            "build",
            "test",
            "lint",
            "git_ops",
            "smoke_chain",
            "loop_guard",
            "init_once",
        ];
        for id in task_scoped {
            assert_eq!(
                default_scope_for_step_id(id),
                StepScope::Task,
                "expected Task for {}",
                id
            );
        }
    }

    #[test]
    fn test_default_scope_item_steps() {
        let item_scoped = vec![
            "qa",
            "qa_testing",
            "ticket_fix",
            "ticket_scan",
            "fix",
            "retest",
        ];
        for id in item_scoped {
            assert_eq!(
                default_scope_for_step_id(id),
                StepScope::Item,
                "expected Item for {}",
                id
            );
        }
    }

    #[test]
    fn test_step_scope_default() {
        let scope = StepScope::default();
        assert_eq!(scope, StepScope::Item);
    }

    #[test]
    fn test_cost_preference_default() {
        let pref = CostPreference::default();
        assert_eq!(pref, CostPreference::Balance);
    }

    #[test]
    fn test_cost_preference_serde_round_trip() {
        for pref_str in &["\"performance\"", "\"quality\"", "\"balance\""] {
            let pref: CostPreference =
                serde_json::from_str(pref_str).expect("deserialize cost preference");
            let json = serde_json::to_string(&pref).expect("serialize cost preference");
            assert_eq!(&json, pref_str);
        }
    }

    #[test]
    fn test_step_scope_serde_round_trip() {
        for scope_str in &["\"task\"", "\"item\""] {
            let scope: StepScope = serde_json::from_str(scope_str).expect("deserialize step scope");
            let json = serde_json::to_string(&scope).expect("serialize step scope");
            assert_eq!(&json, scope_str);
        }
    }
}
