use crate::config::{
    default_builtin_for_step_id, default_required_capability_for_step_id,
    normalize_step_execution_mode, CaptureDecl, CaptureSource, OrchestratorConfig, PostAction,
    StepBehavior, WorkflowConfig, WorkflowStepConfig,
};
use anyhow::Result;
use std::collections::HashSet;

pub fn normalize_workflow_config(workflow: &mut WorkflowConfig) {
    let had_ticket_scan_step = workflow.steps.iter().any(|step| step.id == "ticket_scan");
    if workflow.steps.is_empty() {
        workflow.steps = crate::config::default_workflow_steps(
            workflow.qa.as_deref(),
            false,
            workflow.fix.as_deref(),
            workflow.retest.as_deref(),
        );
    }
    let mut normalized: Vec<WorkflowStepConfig> = Vec::new();

    // Preserve original YAML order: apply defaults to each step in place,
    // then add missing standard steps as disabled placeholders.
    let mut seen_ids: HashSet<String> = HashSet::new();
    for mut step in workflow.steps.drain(..) {
        seen_ids.insert(step.id.clone());

        if step.required_capability.is_none() && step.builtin.is_none() && step.command.is_none() {
            if let Some(builtin) = default_builtin_for_step_id(&step.id) {
                step.builtin = Some(builtin.to_string());
            } else if let Some(capability) = default_required_capability_for_step_id(&step.id) {
                step.required_capability = Some(capability.to_string());
            }
        }

        if step
            .builtin
            .as_deref()
            .or_else(|| default_builtin_for_step_id(&step.id))
            == Some("loop_guard")
        {
            step.is_guard = true;
        }

        apply_default_step_behavior(&mut step);
        let _ = normalize_step_execution_mode_recursive(&mut step);
        normalized.push(step);
    }

    // Add missing standard steps as disabled placeholders (except LoopGuard)
    let standard_step_ids = ["init_once", "plan", "qa", "ticket_scan", "fix", "retest"];
    for step_id in &standard_step_ids {
        if !seen_ids.contains(*step_id) {
            let mut placeholder = WorkflowStepConfig {
                id: step_id.to_string(),
                description: None,
                required_capability: None,
                builtin: None,
                enabled: false,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: Vec::new(),
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            };
            apply_default_step_behavior(&mut placeholder);
            let _ = normalize_step_execution_mode_recursive(&mut placeholder);
            normalized.push(placeholder);
        }
    }
    workflow.steps = normalized;
    let qa_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "qa" && step.enabled);
    let fix_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "fix" && step.enabled);
    let retest_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "retest" && step.enabled);
    if !had_ticket_scan_step && !qa_enabled && fix_enabled && !retest_enabled {
        if let Some(scan_step) = workflow
            .steps
            .iter_mut()
            .find(|step| step.id == "ticket_scan")
        {
            scan_step.enabled = true;
        }
    }
    workflow.qa = None;
    workflow.fix = None;
    workflow.retest = None;
    if workflow.finalize.rules.is_empty() {
        workflow.finalize = crate::config::default_workflow_finalize_config();
    }
    workflow.loop_policy.guard.agent_template = None;
}

/// Apply sensible default behavior to well-known step types when the user
/// hasn't configured explicit captures or collect_artifacts.
pub(crate) fn apply_default_step_behavior(step: &mut WorkflowStepConfig) {
    let key = step
        .builtin
        .as_deref()
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    let has_capture = |var: &str| step.behavior.captures.iter().any(|c| c.var == var);

    let has_post_action = |pa: &PostAction| step.behavior.post_actions.iter().any(|a| a == pa);

    match key {
        "qa" | "qa_testing" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("qa_failed") {
                step.behavior.captures.push(CaptureDecl {
                    var: "qa_failed".to_string(),
                    source: CaptureSource::FailedFlag,
                });
            }
            if !has_post_action(&PostAction::CreateTicket) {
                step.behavior.post_actions.push(PostAction::CreateTicket);
            }
        }
        "fix" | "ticket_fix" => {
            if !has_capture("fix_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "fix_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        "retest" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("retest_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "retest_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        _ => {}
    }
}

pub(crate) fn normalize_step_execution_mode_recursive(step: &mut WorkflowStepConfig) -> Result<()> {
    normalize_step_execution_mode(step).map_err(|e| anyhow::anyhow!(e))?;
    for chain_step in &mut step.chain_steps {
        normalize_step_execution_mode_recursive(chain_step)?;
    }
    Ok(())
}

pub(crate) fn normalize_config(mut config: OrchestratorConfig) -> OrchestratorConfig {
    for workflow in config.workflows.values_mut() {
        normalize_workflow_config(workflow);
    }

    // Ensure builtin CRD definitions are registered
    ensure_builtin_crds(&mut config);

    // Always rebuild the resource store from the (now-normalized) legacy fields.
    // Legacy fields are the source of truth during normalization; the store is
    // a derived index that the CRD pipeline can query.
    //
    // Preserve resource metadata (labels, annotations) from the old store
    // before wiping — sync_legacy_to_store only knows about spec data from
    // legacy fields, not the metadata stored in the CRD resource store.
    let old_store = std::mem::take(&mut config.resource_store);
    crate::crd::writeback::sync_legacy_to_store(&mut config);
    crate::crd::writeback::restore_metadata_from_old_store(&mut config, &old_store);

    config
}

/// Ensure all 9 builtin CRD definitions exist in the config.
fn ensure_builtin_crds(config: &mut OrchestratorConfig) {
    for crd in crate::crd::builtin_defs::builtin_crd_definitions() {
        config
            .custom_resource_definitions
            .entry(crd.kind.clone())
            .or_insert(crd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CaptureSource, ExecutionMode, LoopMode, OrchestratorConfig};
    use crate::config_load::tests::{
        make_builtin_step, make_command_step, make_step, make_workflow,
    };
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn normalize_workflow_sets_builtin_for_self_test() {
        let mut workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "self_test".to_string(),
                description: None,
                builtin: None,
                required_capability: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig {
                    enabled: false,
                    ..crate::config::WorkflowLoopGuardConfig::default()
                },
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };

        normalize_workflow_config(&mut workflow);

        let self_test_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "self_test")
            .expect("self_test step should exist");
        assert_eq!(
            self_test_step.builtin.as_deref(),
            Some("self_test"),
            "builtin should be set to 'self_test' for SelfTest step type"
        );
        assert_eq!(
            self_test_step.behavior.execution,
            ExecutionMode::Builtin {
                name: "self_test".to_string()
            }
        );
    }

    #[test]
    fn normalize_workflow_sets_builtin_execution_for_loop_guard() {
        let mut workflow = make_workflow(vec![make_step("loop_guard", true)]);

        normalize_workflow_config(&mut workflow);

        let guard_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "loop_guard")
            .expect("loop_guard step should exist");
        assert_eq!(guard_step.builtin.as_deref(), Some("loop_guard"));
        assert!(guard_step.is_guard);
        assert_eq!(
            guard_step.behavior.execution,
            ExecutionMode::Builtin {
                name: "loop_guard".to_string()
            }
        );
    }

    #[test]
    fn normalize_workflow_sets_agent_execution_for_plan() {
        let mut workflow = make_workflow(vec![make_step("plan", true)]);

        normalize_workflow_config(&mut workflow);

        let plan_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert_eq!(plan_step.required_capability.as_deref(), Some("plan"));
        assert_eq!(plan_step.behavior.execution, ExecutionMode::Agent);
    }

    #[test]
    fn normalize_workflow_preserves_multiple_self_test_steps() {
        let mut workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "self_test_fail".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                WorkflowStepConfig {
                    id: "self_test_recover".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: crate::config::WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: crate::config::WorkflowLoopGuardConfig {
                    enabled: false,
                    ..crate::config::WorkflowLoopGuardConfig::default()
                },
            },
            finalize: crate::config::WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
        };

        normalize_workflow_config(&mut workflow);

        let self_test_ids: Vec<&str> = workflow
            .steps
            .iter()
            .filter(|s| s.builtin.as_deref() == Some("self_test"))
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(self_test_ids, vec!["self_test_fail", "self_test_recover"]);
    }

    #[test]
    fn normalize_empty_steps_generates_defaults() {
        let mut workflow = make_workflow(vec![]);
        normalize_workflow_config(&mut workflow);
        assert!(
            !workflow.steps.is_empty(),
            "empty steps should generate default steps"
        );
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_step() {
        let mut workflow = make_workflow(vec![make_step("qa", true)]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        assert_eq!(qa_step.required_capability.as_deref(), Some("qa"));
    }

    #[test]
    fn normalize_sets_required_capability_for_fix_step() {
        let mut workflow = make_workflow(vec![make_step("fix", true)]);
        normalize_workflow_config(&mut workflow);
        let fix_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "fix")
            .expect("fix step should exist");
        assert_eq!(fix_step.required_capability.as_deref(), Some("fix"));
    }

    #[test]
    fn normalize_sets_required_capability_for_plan_step() {
        let mut workflow = make_workflow(vec![make_step("plan", true)]);
        normalize_workflow_config(&mut workflow);
        let plan_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert_eq!(plan_step.required_capability.as_deref(), Some("plan"));
    }

    #[test]
    fn normalize_sets_required_capability_for_implement_step() {
        let mut workflow = make_workflow(vec![make_step("implement", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "implement")
            .expect("implement step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("implement"));
    }

    #[test]
    fn normalize_sets_required_capability_for_review_step() {
        let mut workflow = make_workflow(vec![make_step("review", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "review")
            .expect("review step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("review"));
    }

    #[test]
    fn normalize_sets_required_capability_for_build_step() {
        let mut workflow = make_workflow(vec![make_step("build", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "build")
            .expect("build step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("build"));
    }

    #[test]
    fn normalize_sets_required_capability_for_test_step() {
        let mut workflow = make_workflow(vec![make_step("test", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "test")
            .expect("test step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("test"));
    }

    #[test]
    fn normalize_sets_required_capability_for_lint_step() {
        let mut workflow = make_workflow(vec![make_step("lint", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "lint")
            .expect("lint step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("lint"));
    }

    #[test]
    fn normalize_sets_required_capability_for_gitops_step() {
        let mut workflow = make_workflow(vec![make_step("git_ops", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "git_ops")
            .expect("git_ops step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("git_ops"));
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_doc_gen_step() {
        let mut workflow = make_workflow(vec![make_step("qa_doc_gen", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa_doc_gen")
            .expect("qa_doc_gen step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("qa_doc_gen"));
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_testing_step() {
        let mut workflow = make_workflow(vec![make_step("qa_testing", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa_testing")
            .expect("qa_testing step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("qa_testing"));
    }

    #[test]
    fn normalize_sets_required_capability_for_ticket_fix_step() {
        let mut workflow = make_workflow(vec![make_step("ticket_fix", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "ticket_fix")
            .expect("ticket_fix step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("ticket_fix"));
    }

    #[test]
    fn normalize_sets_required_capability_for_doc_governance_step() {
        let mut workflow = make_workflow(vec![make_step("doc_governance", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "doc_governance")
            .expect("doc_governance step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("doc_governance"));
    }

    #[test]
    fn normalize_sets_required_capability_for_align_tests_step() {
        let mut workflow = make_workflow(vec![make_step("align_tests", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "align_tests")
            .expect("align_tests step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("align_tests"));
    }

    #[test]
    fn normalize_sets_required_capability_for_retest_step() {
        let mut workflow = make_workflow(vec![make_step("retest", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "retest")
            .expect("retest step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("retest"));
    }

    #[test]
    fn normalize_sets_required_capability_for_smoke_chain_step() {
        let mut workflow = make_workflow(vec![make_step("smoke_chain", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "smoke_chain")
            .expect("smoke_chain step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("smoke_chain"));
    }

    #[test]
    fn normalize_sets_loop_guard_builtin_and_is_guard() {
        let mut workflow = make_workflow(vec![make_step("loop_guard", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "loop_guard")
            .expect("loop_guard step should exist");
        assert_eq!(step.builtin.as_deref(), Some("loop_guard"));
        assert!(step.is_guard, "LoopGuard step should have is_guard=true");
    }

    #[test]
    fn normalize_sets_default_behavior_for_qa_step() {
        let mut workflow = make_workflow(vec![make_step("qa", true)]);
        normalize_workflow_config(&mut workflow);
        let qa = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        assert!(
            qa.behavior.collect_artifacts,
            "qa step should have collect_artifacts=true"
        );
        assert!(
            qa.behavior
                .captures
                .iter()
                .any(|c| c.var == "qa_failed" && c.source == CaptureSource::FailedFlag),
            "qa step should capture qa_failed from FailedFlag"
        );
    }

    #[test]
    fn normalize_sets_default_behavior_for_fix_step() {
        let mut workflow = make_workflow(vec![make_step("fix", true)]);
        normalize_workflow_config(&mut workflow);
        let fix = workflow
            .steps
            .iter()
            .find(|s| s.id == "fix")
            .expect("fix step should exist");
        assert!(
            fix.behavior
                .captures
                .iter()
                .any(|c| c.var == "fix_success" && c.source == CaptureSource::SuccessFlag),
            "fix step should capture fix_success from SuccessFlag"
        );
    }

    #[test]
    fn normalize_sets_default_behavior_for_retest_step() {
        let mut workflow = make_workflow(vec![make_step("retest", true)]);
        normalize_workflow_config(&mut workflow);
        let retest = workflow
            .steps
            .iter()
            .find(|s| s.id == "retest")
            .expect("retest step should exist");
        assert!(
            retest.behavior.collect_artifacts,
            "retest step should have collect_artifacts=true"
        );
        assert!(
            retest
                .behavior
                .captures
                .iter()
                .any(|c| c.var == "retest_success" && c.source == CaptureSource::SuccessFlag),
            "retest step should capture retest_success from SuccessFlag"
        );
    }

    #[test]
    fn normalize_does_not_duplicate_existing_captures() {
        let mut step = make_step("qa", true);
        step.behavior.captures.push(CaptureDecl {
            var: "qa_failed".to_string(),
            source: CaptureSource::FailedFlag,
        });
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        let qa_failed_count = qa
            .behavior
            .captures
            .iter()
            .filter(|c| c.var == "qa_failed")
            .count();
        assert_eq!(
            qa_failed_count, 1,
            "should not duplicate existing qa_failed capture"
        );
    }

    #[test]
    fn normalize_skips_capability_if_builtin_already_set() {
        let mut step = make_step("qa", true);
        step.builtin = Some("custom_builtin".to_string());
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        // builtin was set, so required_capability should NOT be overridden
        assert_eq!(qa_step.builtin.as_deref(), Some("custom_builtin"));
        assert!(qa_step.required_capability.is_none());
    }

    #[test]
    fn normalize_skips_capability_if_command_already_set() {
        let mut step = make_step("qa", true);
        step.command = Some("echo test".to_string());
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "qa")
            .expect("qa step should exist");
        assert!(qa_step.required_capability.is_none());
    }

    #[test]
    fn normalize_adds_missing_standard_steps_as_disabled() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        normalize_workflow_config(&mut workflow);
        // Should add init_once, plan, qa, ticket_scan, fix, retest as disabled
        let init_step = workflow.steps.iter().find(|s| s.id == "init_once");
        assert!(init_step.is_some(), "should add init_once step");
        assert!(
            !init_step.expect("init_once step should exist").enabled,
            "added init_once should be disabled"
        );

        let plan_step = workflow.steps.iter().find(|s| s.id == "plan");
        assert!(plan_step.is_some(), "should add plan step");
        assert!(
            !plan_step.expect("plan step should exist").enabled,
            "added plan should be disabled"
        );
    }

    #[test]
    fn normalize_does_not_duplicate_existing_step_types() {
        let mut workflow = make_workflow(vec![make_step("plan", true)]);
        normalize_workflow_config(&mut workflow);
        let plan_count = workflow.steps.iter().filter(|s| s.id == "plan").count();
        assert_eq!(
            plan_count, 1,
            "should not duplicate already-present plan step"
        );
    }

    #[test]
    fn normalize_clears_qa_fix_retest_legacy_fields() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.qa = Some("qa_template".to_string());
        workflow.fix = Some("fix_template".to_string());
        workflow.retest = Some("retest_template".to_string());
        normalize_workflow_config(&mut workflow);
        assert!(workflow.qa.is_none(), "qa should be cleared");
        assert!(workflow.fix.is_none(), "fix should be cleared");
        assert!(workflow.retest.is_none(), "retest should be cleared");
    }

    #[test]
    fn normalize_sets_default_finalize_rules_when_empty() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        assert!(workflow.finalize.rules.is_empty());
        normalize_workflow_config(&mut workflow);
        assert!(
            !workflow.finalize.rules.is_empty(),
            "should set default finalize rules when empty"
        );
    }

    #[test]
    fn normalize_clears_guard_agent_template() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.guard.agent_template = Some("old_template".to_string());
        normalize_workflow_config(&mut workflow);
        assert!(
            workflow.loop_policy.guard.agent_template.is_none(),
            "agent_template should be cleared"
        );
    }

    #[test]
    fn normalize_preserves_step_id() {
        let step = make_step("plan", true);
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let plan_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert_eq!(plan_step.id, "plan", "step id should be preserved");
    }

    #[test]
    fn normalize_sets_required_capability_from_id() {
        let step = make_step("plan", true);
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let step = workflow
            .steps
            .iter()
            .find(|s| s.id == "plan")
            .expect("plan step should exist");
        assert_eq!(step.required_capability.as_deref(), Some("plan"));
    }

    #[test]
    fn normalize_enables_ticket_scan_when_fix_only() {
        let mut workflow = make_workflow(vec![make_command_step("fix", "echo fix")]);
        normalize_workflow_config(&mut workflow);
        let scan = workflow.steps.iter().find(|s| s.id == "ticket_scan");
        assert!(scan.is_some(), "ticket_scan should exist");
        assert!(
            scan.expect("ticket_scan should exist").enabled,
            "ticket_scan should be enabled when fix is enabled but qa is not"
        );
    }

    #[test]
    fn normalize_does_not_enable_ticket_scan_when_qa_also_enabled() {
        let mut workflow = make_workflow(vec![
            make_command_step("qa", "echo qa"),
            make_command_step("fix", "echo fix"),
        ]);
        normalize_workflow_config(&mut workflow);
        let scan = workflow.steps.iter().find(|s| s.id == "ticket_scan");
        // ticket_scan should still exist (as disabled placeholder) since it wasn't in steps
        if let Some(s) = scan {
            assert!(
                !s.enabled,
                "ticket_scan should NOT be auto-enabled when qa is also enabled"
            );
        }
    }

    #[test]
    fn normalize_config_normalizes_all_workflows() {
        let mut workflows = HashMap::new();
        workflows.insert("wf1".to_string(), make_workflow(vec![]));
        workflows.insert("wf2".to_string(), make_workflow(vec![]));
        let config = OrchestratorConfig {
            workflows,
            ..OrchestratorConfig::default()
        };
        let normalized = normalize_config(config);
        for wf in normalized.workflows.values() {
            assert!(!wf.steps.is_empty(), "all workflows should be normalized");
        }
    }

    #[test]
    fn normalize_preserves_required_capability_on_custom_step_ids() {
        let steps = vec![WorkflowStepConfig {
            id: "run_qa".to_string(),
            description: None,
            required_capability: Some("qa".to_string()),
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
            max_parallel: None,
            timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
        }];
        let mut wf = make_workflow(steps);
        normalize_workflow_config(&mut wf);

        let run_qa = wf
            .steps
            .iter()
            .find(|s| s.id == "run_qa")
            .expect("run_qa should exist");
        assert_eq!(
            run_qa.required_capability,
            Some("qa".to_string()),
            "required_capability must survive normalization"
        );

        let json = serde_json::to_string_pretty(run_qa).expect("serialize run_qa");
        assert!(
            json.contains("required_capability"),
            "required_capability must appear in JSON: {}",
            json
        );
    }

    // ── CRD pipeline tests in normalize_config ──────────────────────

    #[test]
    fn normalize_config_populates_builtin_crds() {
        let config = OrchestratorConfig {
            workflows: HashMap::new(),
            ..OrchestratorConfig::default()
        };
        let normalized = normalize_config(config);
        let crds = &normalized.custom_resource_definitions;
        for kind in &[
            "Agent",
            "Workflow",
            "Workspace",
            "Project",
            "Defaults",
            "RuntimePolicy",
            "StepTemplate",
            "EnvStore",
            "SecretStore",
        ] {
            assert!(crds.contains_key(*kind), "missing builtin CRD: {}", kind);
            assert!(crds[*kind].builtin, "{} should be marked builtin", kind);
        }
    }

    #[test]
    fn normalize_config_does_not_overwrite_user_crds() {
        let mut config = OrchestratorConfig::default();
        // Insert a user-defined CRD with a non-builtin kind
        let user_crd = crate::crd::types::CustomResourceDefinition {
            kind: "MyCrd".to_string(),
            plural: "mycrds".to_string(),
            short_names: vec![],
            group: "test.dev".to_string(),
            scope: crate::crd::scope::CrdScope::Cluster,
            builtin: false,
            versions: vec![],
            hooks: Default::default(),
        };
        config
            .custom_resource_definitions
            .insert("MyCrd".to_string(), user_crd);

        let normalized = normalize_config(config);
        assert!(normalized.custom_resource_definitions.contains_key("MyCrd"));
        assert!(!normalized.custom_resource_definitions["MyCrd"].builtin);
    }

    #[test]
    fn normalize_config_rebuilds_resource_store_from_legacy() {
        let mut config = OrchestratorConfig::default();
        config.agents.insert(
            "norm-ag".to_string(),
            crate::config::AgentConfig {
                command: "echo test".to_string(),
                ..Default::default()
            },
        );
        let normalized = normalize_config(config);

        assert!(
            normalized.resource_store.get("Agent", "norm-ag").is_some(),
            "store should be populated from legacy agents"
        );
        assert!(
            normalized
                .resource_store
                .get("Defaults", "defaults")
                .is_some(),
            "singletons should also be in the store"
        );
    }

    #[test]
    fn normalize_config_clears_stale_store() {
        let mut config = OrchestratorConfig::default();
        // Manually put a stale entry in the store
        let stale_cr = crate::crd::types::CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "stale-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"command": "echo stale"}),
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        config.resource_store.put(stale_cr);
        assert!(config.resource_store.get("Agent", "stale-agent").is_some());

        // Legacy does NOT have "stale-agent"
        let normalized = normalize_config(config);
        assert!(
            normalized
                .resource_store
                .get("Agent", "stale-agent")
                .is_none(),
            "stale store entries should be cleared during normalize"
        );
    }

    #[test]
    fn normalize_config_idempotent_double_call() {
        let mut config = OrchestratorConfig::default();
        config.agents.insert(
            "idem-ag".to_string(),
            crate::config::AgentConfig {
                command: "echo test".to_string(),
                ..Default::default()
            },
        );
        let first = normalize_config(config);
        let second = normalize_config(first);

        assert!(second.agents.contains_key("idem-ag"));
        assert!(second.resource_store.get("Agent", "idem-ag").is_some());
        assert_eq!(second.custom_resource_definitions.len(), 11);
    }
}
