use serde::{Deserialize, Serialize};

use super::{GenerateItemsAction, SpawnTaskAction, SpawnTasksAction, WorkflowStepConfig};

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
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct StepBehavior {
    /// Action to apply when the step returns a failure status.
    #[serde(default)]
    pub on_failure: OnFailureAction,
    /// Action to apply when the step succeeds.
    #[serde(default)]
    pub on_success: OnSuccessAction,
    /// Variables to capture from the step result.
    #[serde(default)]
    pub captures: Vec<CaptureDecl>,
    /// Follow-up actions triggered after the step completes.
    #[serde(default)]
    pub post_actions: Vec<PostAction>,
    /// Explicit execution mode chosen for the step.
    #[serde(default)]
    pub execution: ExecutionMode,
    /// Whether runner artifacts should be persisted for the step.
    #[serde(default)]
    pub collect_artifacts: bool,
}

/// What to do when a step fails.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnFailureAction {
    /// Continue the workflow without changing status.
    #[default]
    Continue,
    /// Overwrite the task or task-item status and continue processing.
    SetStatus {
        /// Status value to persist.
        status: String,
    },
    /// Set a terminal status and return early from the current segment.
    EarlyReturn {
        /// Status value to persist before returning.
        status: String,
    },
}

/// What to do when a step succeeds.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnSuccessAction {
    /// Continue the workflow with no extra side effects.
    #[default]
    Continue,
    /// Overwrite the task or task-item status after success.
    SetStatus {
        /// Status value to persist.
        status: String,
    },
}

/// A single capture declaration: what to extract from a step result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureDecl {
    /// Pipeline variable to write.
    pub var: String,
    /// Output channel that populates the variable.
    pub source: CaptureSource,
    /// Optional JSON path to extract from stdout/stderr content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_path: Option<String>,
}

/// Source of a captured value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    /// Capture standard output text.
    Stdout,
    /// Capture standard error text.
    Stderr,
    /// Capture the numeric exit code.
    ExitCode,
    /// Capture whether the step was marked as failed.
    FailedFlag,
    /// Capture whether the step was marked as successful.
    SuccessFlag,
}

/// Post-step action to run after a step completes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PostAction {
    /// Create tickets from a failing QA step.
    CreateTicket,
    /// Re-scan active tickets after a step completes.
    ScanTickets,
    /// WP02: Spawn a single child task.
    SpawnTask(SpawnTaskAction),
    /// WP02: Spawn multiple child tasks from a JSON array.
    SpawnTasks(SpawnTasksAction),
    /// WP03: Generate dynamic task items from step output.
    GenerateItems(GenerateItemsAction),
    /// WP01: Write a pipeline variable to a workflow store.
    StorePut {
        /// Workflow store resource name.
        store: String,
        /// Entry key to update.
        key: String,
        /// Pipeline variable whose value should be written.
        from_var: String,
    },
}

/// How a step is executed.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ExecutionMode {
    /// Execute the step by selecting an agent with the required capability.
    #[default]
    Agent,
    /// Execute one builtin step implementation.
    Builtin {
        /// Builtin step name.
        name: String,
    },
    /// Execute a sequence of child steps inside one chain step.
    Chain,
}

/// Resolved semantic meaning for a workflow step after applying defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepSemanticKind {
    /// A builtin step resolved by name.
    Builtin {
        /// Builtin implementation name.
        name: String,
    },
    /// An agent-backed step resolved by capability.
    Agent {
        /// Capability required from the selected agent.
        capability: String,
    },
    /// A command-backed builtin step.
    Command,
    /// A chain step containing nested child steps.
    Chain,
}

/// Preference used when selecting between cost, quality, and speed tradeoffs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CostPreference {
    /// Favor lower latency or higher throughput.
    Performance,
    /// Favor higher output quality even if slower.
    Quality,
    #[default]
    /// Balance quality and performance heuristics.
    Balance,
}

/// Framework builtin step names — these have Rust implementations in the
/// scheduler and form a security boundary: only names in this list may be
/// dispatched as `ExecutionMode::Builtin`.
const KNOWN_BUILTIN_STEP_NAMES: &[&str] = &[
    "init_once",
    "loop_guard",
    "ticket_scan",
    "self_test",
    "self_restart",
    "item_select",
];

/// Accepts any non-empty step type string.
///
/// The framework no longer maintains a whitelist of known step IDs.
/// Custom step IDs are legal and resolve to `Agent { capability = step_id }`
/// via the universal fallback rule.
pub fn validate_step_type(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err("step type cannot be empty".to_string())
    } else {
        Ok(value.to_string())
    }
}

/// Returns `true` when a builtin step name is recognized by the scheduler.
pub fn is_known_builtin_step_name(value: &str) -> bool {
    KNOWN_BUILTIN_STEP_NAMES.contains(&value)
}

/// Resolves the semantic step kind after applying convention-registry defaults.
///
/// Resolution priority:
/// 1. chain_steps → Chain
/// 2. command → Command
/// 3. explicit builtin (validated against KNOWN_BUILTIN_STEP_NAMES) → Builtin
/// 4. explicit required_capability → Agent
/// 5. convention-registry builtin → Builtin
/// 6. universal fallback → Agent { capability = step_id }
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

    // Convention-registry lookup: check if this step ID maps to a framework builtin.
    if let Some(builtin_name) = super::CONVENTIONS.builtin_name(&step.id) {
        return Ok(StepSemanticKind::Builtin { name: builtin_name });
    }

    // Universal fallback: any step dispatches to an agent whose capability
    // matches the step ID.  This replaces the former hardcoded capability table.
    Ok(StepSemanticKind::Agent {
        capability: step.id.clone(),
    })
}

/// Normalizes the execution mode and default selectors for one workflow step.
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

/// Returns the default execution scope for a step ID.
/// Delegates to the convention registry; falls back to Task scope.
pub fn default_scope_for_step_id(step_id: &str) -> StepScope {
    super::CONVENTIONS.default_scope(step_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_decl_deserializes_without_json_path() {
        let capture: CaptureDecl = serde_yaml::from_str(
            r#"
var: score
source: stdout
"#,
        )
        .expect("capture should deserialize");

        assert_eq!(capture.var, "score");
        assert_eq!(capture.source, CaptureSource::Stdout);
        assert_eq!(capture.json_path, None);
    }

    #[test]
    fn capture_decl_deserializes_with_json_path() {
        let capture: CaptureDecl = serde_yaml::from_str(
            r#"
var: score
source: stdout
json_path: $.total_score
"#,
        )
        .expect("capture should deserialize");

        assert_eq!(capture.var, "score");
        assert_eq!(capture.source, CaptureSource::Stdout);
        assert_eq!(capture.json_path.as_deref(), Some("$.total_score"));
    }

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
            "self_restart",
            "smoke_chain",
        ] {
            assert!(validate_step_type(id).is_ok(), "expected valid for {}", id);
        }
    }

    #[test]
    fn test_validate_step_type_accepts_custom_ids() {
        // Custom step IDs are now accepted — no whitelist restriction.
        let result = validate_step_type("my_custom_step");
        assert!(result.is_ok(), "custom step IDs should be accepted");
        assert_eq!(result.unwrap(), "my_custom_step");
    }

    #[test]
    fn test_validate_step_type_rejects_empty() {
        assert!(validate_step_type("").is_err());
        assert!(validate_step_type("  ").is_err());
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
    fn test_unknown_step_scope_defaults_to_task() {
        assert_eq!(
            default_scope_for_step_id("my_custom_step"),
            StepScope::Task
        );
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

    #[test]
    fn test_post_action_store_put_serde_round_trip() {
        let action = PostAction::StorePut {
            store: "metrics".to_string(),
            key: "bench_result".to_string(),
            from_var: "qa_score".to_string(),
        };
        let json = serde_json::to_string(&action).expect("serialize StorePut");
        assert!(json.contains("\"type\":\"store_put\""));
        assert!(json.contains("\"store\":\"metrics\""));
        assert!(json.contains("\"key\":\"bench_result\""));
        assert!(json.contains("\"from_var\":\"qa_score\""));

        let deserialized: PostAction = serde_json::from_str(&json).expect("deserialize StorePut");
        match deserialized {
            PostAction::StorePut {
                store,
                key,
                from_var,
            } => {
                assert_eq!(store, "metrics");
                assert_eq!(key, "bench_result");
                assert_eq!(from_var, "qa_score");
            }
            _ => panic!("expected StorePut variant"),
        }
    }
}
