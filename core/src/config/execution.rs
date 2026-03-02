use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    default_scope_for_step_id, is_known_builtin_step_name, AgentConfig, CostPreference,
    ExecutionMode, OrchestratorConfig, PipelineVariables, SafetyConfig, StepBehavior,
    StepPrehookConfig, StepScope, WorkflowConfig, WorkflowFinalizeConfig, WorkflowLoopConfig,
};

fn default_true() -> bool {
    true
}

/// Task execution step (runtime representation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionStep {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub repeatable: bool,
    #[serde(default)]
    pub is_guard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_preference: Option<CostPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prehook: Option<StepPrehookConfig>,
    #[serde(default)]
    pub tty: bool,
    /// Named outputs this step produces (for pipeline variable passing)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// Pipe this step's output to the named step as input
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipe_to: Option<String>,
    /// Build command for builtin build/test/lint steps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Sub-steps to execute in sequence for smoke_chain step
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain_steps: Vec<TaskExecutionStep>,
    /// Execution scope override (defaults based on step type)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<StepScope>,
    /// Declarative step behavior (on_failure, captures, post_actions, etc.)
    #[serde(default)]
    pub behavior: StepBehavior,
}

impl TaskExecutionStep {
    /// Returns the resolved scope: explicit override or default based on step id,
    /// falling back to required_capability when the id is not a known step type.
    pub fn resolved_scope(&self) -> StepScope {
        self.scope.unwrap_or_else(|| {
            let scope = default_scope_for_step_id(&self.id);
            if scope == StepScope::Task {
                if let Some(ref cap) = self.required_capability {
                    let cap_scope = default_scope_for_step_id(cap);
                    if cap_scope == StepScope::Item {
                        return cap_scope;
                    }
                }
            }
            scope
        })
    }

    /// Returns the authoritative execution mode for this step.
    ///
    /// If `self.builtin` names a known builtin, this always returns
    /// `Builtin { name }` regardless of what `behavior.execution` says.
    /// This is the single consolidated entry point for dispatch decisions.
    ///
    /// Unlike [`renormalize_execution_mode`] which mutates stored state,
    /// this method is read-only and is always authoritative at dispatch time,
    /// even if renormalization hasn't run yet.
    pub fn effective_execution_mode(&self) -> std::borrow::Cow<'_, ExecutionMode> {
        if let Some(ref bname) = self.builtin {
            if is_known_builtin_step_name(bname) {
                return std::borrow::Cow::Owned(ExecutionMode::Builtin {
                    name: bname.clone(),
                });
            }
        }
        std::borrow::Cow::Borrowed(&self.behavior.execution)
    }

    /// Corrects `behavior.execution` when `builtin` disagrees with it.
    ///
    /// After deserializing from SQLite the `behavior.execution` field may carry
    /// the serde `#[default]` value (`ExecutionMode::Agent`) even though
    /// `self.builtin` names a known builtin step.  This method is the single
    /// source of truth for healing that mismatch:
    ///
    /// - If `self.builtin` names a known builtin, force `behavior.execution`
    ///   to `Builtin { name }` and clear `required_capability`.
    /// - Otherwise leave the step unchanged.
    pub fn renormalize_execution_mode(&mut self) {
        if let Some(ref name) = self.builtin.clone() {
            if is_known_builtin_step_name(name) {
                self.behavior.execution = ExecutionMode::Builtin { name: name.clone() };
                self.required_capability = None;
            }
        }
    }
}

/// Task execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionPlan {
    pub steps: Vec<TaskExecutionStep>,
    #[serde(rename = "loop")]
    pub loop_policy: WorkflowLoopConfig,
    #[serde(default)]
    pub finalize: WorkflowFinalizeConfig,
}

impl TaskExecutionPlan {
    /// Find step by string id
    pub fn step_by_id(&self, id: &str) -> Option<&TaskExecutionStep> {
        self.steps.iter().find(|step| step.id == id)
    }
}

/// Task runtime context
#[derive(Debug, Clone)]
pub struct TaskRuntimeContext {
    pub workspace_id: String,
    pub workspace_root: std::path::PathBuf,
    pub ticket_dir: String,
    pub execution_plan: TaskExecutionPlan,
    pub current_cycle: u32,
    pub init_done: bool,
    pub dynamic_steps: Vec<crate::dynamic_orchestration::DynamicStepConfig>,
    /// Pipeline variables accumulated across steps in the current cycle
    pub pipeline_vars: PipelineVariables,
    /// Safety configuration
    pub safety: SafetyConfig,
    /// Whether the workspace is self-referential
    pub self_referential: bool,
    /// Consecutive failure counter for auto-rollback
    pub consecutive_failures: u32,
}

/// Step prehook context for evaluation
#[derive(Debug, Clone, Serialize)]
pub struct StepPrehookContext {
    pub task_id: String,
    pub task_item_id: String,
    pub cycle: u32,
    pub step: String,
    pub qa_file_path: String,
    pub item_status: String,
    pub task_status: String,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub qa_failed: bool,
    pub fix_required: bool,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_has_changes: Option<bool>,
    pub upstream_artifacts: Vec<ArtifactSummary>,
    /// Number of build errors from the last build step
    pub build_error_count: i64,
    /// Number of test failures from the last test step
    pub test_failure_count: i64,
    /// Exit code of the last build step
    pub build_exit_code: Option<i64>,
    /// Exit code of the last test step
    pub test_exit_code: Option<i64>,
    /// Exit code of the last self_test step
    pub self_test_exit_code: Option<i64>,
    /// Whether the last self_test step passed
    pub self_test_passed: bool,
    /// Maximum number of cycles configured for this workflow
    pub max_cycles: u32,
    /// Whether this is the last cycle (cycle == max_cycles)
    pub is_last_cycle: bool,
}

/// Artifact summary
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactSummary {
    pub phase: String,
    pub kind: String,
    pub path: Option<String>,
}

/// Item finalize context
#[derive(Debug, Clone, Serialize)]
pub struct ItemFinalizeContext {
    pub task_id: String,
    pub task_item_id: String,
    pub cycle: u32,
    pub qa_file_path: String,
    pub item_status: String,
    pub task_status: String,
    pub qa_exit_code: Option<i64>,
    pub fix_exit_code: Option<i64>,
    pub retest_exit_code: Option<i64>,
    pub active_ticket_count: i64,
    pub new_ticket_count: i64,
    pub retest_new_ticket_count: i64,
    pub qa_failed: bool,
    pub fix_required: bool,
    pub qa_configured: bool,
    pub qa_observed: bool,
    pub qa_enabled: bool,
    pub qa_ran: bool,
    pub qa_skipped: bool,
    pub fix_configured: bool,
    pub fix_enabled: bool,
    pub fix_ran: bool,
    pub fix_success: bool,
    pub retest_enabled: bool,
    pub retest_ran: bool,
    pub retest_success: bool,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_confidence: Option<f32>,
    pub fix_quality_score: Option<f32>,
    pub total_artifacts: i64,
    pub has_ticket_artifacts: bool,
    pub has_code_change_artifacts: bool,
    pub is_last_cycle: bool,
}

/// Workflow finalize outcome
#[derive(Debug, Clone)]
pub struct WorkflowFinalizeOutcome {
    pub rule_id: String,
    pub status: String,
    pub reason: String,
}

/// Resolved workspace (with absolute paths)
#[derive(Debug, Clone)]
pub struct ResolvedWorkspace {
    pub root_path: std::path::PathBuf,
    pub qa_targets: Vec<String>,
    pub ticket_dir: String,
}

/// Resolved project
#[derive(Debug, Clone)]
pub struct ResolvedProject {
    pub workspaces: HashMap<String, ResolvedWorkspace>,
    pub agents: HashMap<String, AgentConfig>,
    pub workflows: HashMap<String, WorkflowConfig>,
}

/// Active configuration (runtime state)
#[derive(Debug, Clone)]
pub struct ActiveConfig {
    pub config: OrchestratorConfig,
    pub workspaces: HashMap<String, ResolvedWorkspace>,
    pub projects: HashMap<String, ResolvedProject>,
    pub default_project_id: String,
    pub default_workspace_id: String,
    pub default_workflow_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent_step(
        id: &str,
        builtin: Option<&str>,
        capability: Option<&str>,
    ) -> TaskExecutionStep {
        TaskExecutionStep {
            id: id.to_string(),
            required_capability: capability.map(|s| s.to_string()),
            builtin: builtin.map(|s| s.to_string()),
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
        }
    }

    #[test]
    fn test_resolved_scope_explicit_override() {
        let step = TaskExecutionStep {
            id: "qa".to_string(), // default would be Item
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: Some(StepScope::Task), // explicit override
            behavior: StepBehavior::default(),
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn test_resolved_scope_from_step_id() {
        let step = TaskExecutionStep {
            id: "plan".to_string(),
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn test_resolved_scope_unknown_id_defaults_to_task() {
        let step = TaskExecutionStep {
            id: "my_custom_step".to_string(),
            required_capability: None,
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }

    #[test]
    fn test_task_execution_plan_step_by_id_found() {
        let plan = TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                TaskExecutionStep {
                    id: "qa".to_string(),
                    required_capability: None,
                    builtin: None,
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
        };

        let found = plan.step_by_id("qa");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "qa");

        let found_plan = plan.step_by_id("plan");
        assert!(found_plan.is_some());
        assert_eq!(found_plan.unwrap().id, "plan");
    }

    #[test]
    fn test_task_execution_plan_step_by_id_not_found() {
        let plan = TaskExecutionPlan {
            steps: vec![],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
        };
        assert!(plan.step_by_id("fix").is_none());
    }

    #[test]
    fn renormalize_corrects_stale_agent_to_builtin() {
        let mut step = make_agent_step("self_test", Some("self_test"), None);
        // Precondition: execution defaults to Agent (serde default)
        assert_eq!(step.behavior.execution, ExecutionMode::Agent);
        step.renormalize_execution_mode();
        assert_eq!(
            step.behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
    }

    #[test]
    fn renormalize_clears_stale_required_capability() {
        let mut step = make_agent_step("self_test", Some("self_test"), Some("self_test"));
        step.renormalize_execution_mode();
        assert!(step.required_capability.is_none());
    }

    #[test]
    fn renormalize_noop_for_correct_builtin() {
        let mut step = make_agent_step("self_test", Some("self_test"), None);
        step.behavior.execution = ExecutionMode::Builtin {
            name: "self_test".to_string(),
        };
        step.renormalize_execution_mode();
        assert_eq!(
            step.behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
    }

    #[test]
    fn renormalize_noop_for_agent_step() {
        let mut step = make_agent_step("plan", None, Some("plan"));
        step.renormalize_execution_mode();
        // stays Agent, capability unchanged
        assert_eq!(step.behavior.execution, ExecutionMode::Agent);
        assert_eq!(step.required_capability, Some("plan".to_string()));
    }

    #[test]
    fn renormalize_handles_all_known_builtins() {
        for name in &["init_once", "loop_guard", "ticket_scan", "self_test"] {
            let mut step = make_agent_step(name, Some(name), None);
            // Starts as Agent (default)
            assert_eq!(
                step.behavior.execution,
                ExecutionMode::Agent,
                "name={}",
                name
            );
            step.renormalize_execution_mode();
            assert_eq!(
                step.behavior.execution,
                ExecutionMode::Builtin {
                    name: name.to_string()
                },
                "name={}",
                name
            );
        }
    }
}
