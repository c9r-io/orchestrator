use super::{CheckResult, KNOWN_SYSTEM_VARS};
use crate::anomaly::Severity;
use crate::config::{
    is_known_builtin_step_name, resolve_step_semantic_kind, ExecutionMode, StepSemanticKind,
    WorkflowStepConfig,
};
use crate::scheduler::trace::find_template_vars;
use std::collections::HashSet;

pub(super) fn check_builtin_names(
    workflows: &std::collections::HashMap<String, crate::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        check_steps_builtin(&wf.steps, wf_id, out);
    }
}

fn check_steps_builtin(steps: &[WorkflowStepConfig], wf_id: &str, out: &mut Vec<CheckResult>) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if step.builtin.is_some() && step.required_capability.is_some() {
            out.push(CheckResult::simple(
                "step_semantic_conflict",
                Severity::Error,
                false,
                format!(
                    "workflow \"{wf_id}\" step \"{}\": cannot define both builtin and required_capability",
                    step.id
                ),
                None,
            ));
        }
        if let Some(ref builtin) = step.builtin {
            let known = is_known_builtin_step_name(builtin);
            out.push(CheckResult::simple(
                "builtin_unknown",
                Severity::Error,
                known,
                if known {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": builtin \"{builtin}\" is known",
                        step.id
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": builtin \"{builtin}\" is not a known builtin",
                        step.id
                    )
                },
                Some(
                    "known builtins: [\"init_once\", \"loop_guard\", \"ticket_scan\", \"self_test\"]"
                        .to_string(),
                ),
            ));
        }
        match resolve_step_semantic_kind(step) {
            Ok(semantic) => {
                let matches_execution = match semantic {
                    StepSemanticKind::Builtin { ref name } => {
                        step.behavior.execution == ExecutionMode::Builtin { name: name.clone() }
                    }
                    StepSemanticKind::Agent { .. } => {
                        step.behavior.execution == ExecutionMode::Agent
                    }
                    StepSemanticKind::Command => {
                        step.behavior.execution
                            == ExecutionMode::Builtin {
                                name: step.id.clone(),
                            }
                    }
                    StepSemanticKind::Chain => step.behavior.execution == ExecutionMode::Chain,
                };
                out.push(CheckResult::simple(
                    "execution_mode_mismatch",
                    Severity::Error,
                    matches_execution,
                    if matches_execution {
                        format!(
                            "workflow \"{wf_id}\" step \"{}\": execution mode matches semantic meaning",
                            step.id
                        )
                    } else {
                        format!(
                            "workflow \"{wf_id}\" step \"{}\": execution mode does not match builtin/capability semantics",
                            step.id
                        )
                    },
                    None,
                ));
            }
            Err(err) => out.push(CheckResult::simple(
                "step_semantic_invalid",
                Severity::Error,
                false,
                format!("workflow \"{wf_id}\" step \"{}\": {err}", step.id),
                None,
            )),
        }
        if !step.chain_steps.is_empty() {
            check_steps_builtin(&step.chain_steps, wf_id, out);
        }
    }
}

pub(super) fn check_pipe_to_refs(
    workflows: &std::collections::HashMap<String, crate::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let step_ids: HashSet<&str> = collect_step_ids(&wf.steps);
        check_steps_pipe_to(&wf.steps, wf_id, &step_ids, out);
    }
}

fn collect_step_ids(steps: &[WorkflowStepConfig]) -> HashSet<&str> {
    let mut ids = HashSet::new();
    for step in steps {
        ids.insert(step.id.as_str());
        for child in &step.chain_steps {
            ids.insert(child.id.as_str());
        }
    }
    ids
}

fn check_steps_pipe_to(
    steps: &[WorkflowStepConfig],
    wf_id: &str,
    step_ids: &HashSet<&str>,
    out: &mut Vec<CheckResult>,
) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if let Some(ref target) = step.pipe_to {
            let known = step_ids.contains(target.as_str());
            out.push(CheckResult::simple(
                "pipe_to_unknown",
                Severity::Error,
                known,
                if known {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": pipe_to \"{target}\" exists",
                        step.id
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": pipe_to \"{target}\" is not a step in this workflow",
                        step.id
                    )
                },
                None,
            ));
        }
        if !step.chain_steps.is_empty() {
            check_steps_pipe_to(&step.chain_steps, wf_id, step_ids, out);
        }
    }
}

pub(super) fn check_template_vars(
    step_templates: &std::collections::HashMap<String, crate::config::StepTemplateConfig>,
    workflows: &std::collections::HashMap<String, crate::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let sys_vars: HashSet<&str> = KNOWN_SYSTEM_VARS.iter().copied().collect();

    // Collect pipeline-derived vars per workflow
    for (wf_id, wf) in workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let mut pipeline_vars: HashSet<String> = HashSet::new();
        for step in &wf.steps {
            pipeline_vars.insert(format!("{}_output", step.id));
            pipeline_vars.insert(format!("{}_output_path", step.id));
            for child in &step.chain_steps {
                pipeline_vars.insert(format!("{}_output", child.id));
                pipeline_vars.insert(format!("{}_output_path", child.id));
            }
        }

        // Check each step template for unknown vars
        for (tmpl_name, tmpl_config) in step_templates {
            for var_with_braces in find_template_vars(&tmpl_config.prompt) {
                // strip braces: {foo} -> foo
                let var = &var_with_braces[1..var_with_braces.len() - 1];
                if sys_vars.contains(var) {
                    continue;
                }
                if pipeline_vars.contains(var) {
                    continue;
                }
                out.push(CheckResult::simple(
                    "template_unknown_var",
                    Severity::Warning,
                    false,
                    format!(
                        "step_template \"{tmpl_name}\": prompt references {var_with_braces} \
                         — not a known system variable (may come from pipeline)"
                    ),
                    None,
                ));
            }
        }
    }
}

pub(super) fn check_empty_workflows(
    workflows: &std::collections::HashMap<String, crate::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (wf_id, wf) in workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        let enabled_count = wf.steps.iter().filter(|s| s.enabled).count();
        let is_empty = enabled_count == 0;
        out.push(CheckResult::simple(
            "empty_workflow",
            Severity::Warning,
            !is_empty,
            if is_empty {
                format!("workflow \"{wf_id}\": has 0 enabled steps")
            } else {
                format!("workflow \"{wf_id}\": has {enabled_count} enabled steps")
            },
            None,
        ));
    }
}
