use super::steps::{apply_default_step_behavior, normalize_step_execution_mode_recursive};
use crate::config::{
    StepBehavior, WorkflowConfig, WorkflowStepConfig, default_builtin_for_step_id,
    default_required_capability_for_step_id,
};
use std::collections::HashSet;

/// Normalizes one workflow config in place to align legacy and implicit defaults.
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
                execution_profile: None,
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
                stagger_delay_ms: None,
                timeout_secs: None,
                stall_timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
                step_vars: None,
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
