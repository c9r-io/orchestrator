use super::CheckResult;
use agent_orchestrator::anomaly::Severity;
use agent_orchestrator::config::{
    resolve_step_semantic_kind, PromptDelivery, StepSemanticKind, WorkflowStepConfig,
};
use std::collections::HashSet;

pub(super) fn check_capability_coverage(
    agents: &std::collections::HashMap<String, agent_orchestrator::config::AgentConfig>,
    workflows: &std::collections::HashMap<String, agent_orchestrator::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let all_caps: HashSet<&str> = agents
        .values()
        .flat_map(|a| a.capabilities.iter().map(|s| s.as_str()))
        .collect();

    for (wf_id, wf) in workflows {
        if let Some(filter) = workflow_filter {
            if wf_id != filter {
                continue;
            }
        }
        check_steps_capability(&wf.steps, wf_id, &all_caps, out);
    }
}

fn check_steps_capability(
    steps: &[WorkflowStepConfig],
    wf_id: &str,
    all_caps: &HashSet<&str>,
    out: &mut Vec<CheckResult>,
) {
    for step in steps {
        if !step.enabled {
            continue;
        }
        if let Ok(StepSemanticKind::Agent { capability }) = resolve_step_semantic_kind(step) {
            let cap = capability;
            let covered = all_caps.contains(cap.as_str());
            out.push(CheckResult::simple(
                "capability_no_agent",
                Severity::Error,
                covered,
                if covered {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": capability \"{}\" is provided",
                        step.id, cap
                    )
                } else {
                    format!(
                        "workflow \"{wf_id}\" step \"{}\": requires capability \"{}\" but no agent provides it",
                        step.id, cap
                    )
                },
                None,
            ));
        }
        // Recurse into chain_steps
        if !step.chain_steps.is_empty() {
            check_steps_capability(&step.chain_steps, wf_id, all_caps, out);
        }
    }
}

pub(super) fn check_prompt_delivery(
    agents: &std::collections::HashMap<String, agent_orchestrator::config::AgentConfig>,
    out: &mut Vec<CheckResult>,
) {
    for (agent_id, agent) in agents {
        let delivery = agent.prompt_delivery;
        let cmd = &agent.command;

        match delivery {
            PromptDelivery::Stdin | PromptDelivery::Env if cmd.contains("{prompt}") => {
                out.push(CheckResult::simple(
                    "prompt_delivery_placeholder_ignored",
                    Severity::Warning,
                    false,
                    format!(
                        "agent \"{agent_id}\": command contains {{prompt}} but prompt_delivery={delivery:?}; placeholder will be ignored"
                    ),
                    None,
                ));
            }
            PromptDelivery::File if cmd.contains("{prompt}") => {
                out.push(CheckResult::simple(
                    "prompt_delivery_placeholder_ignored",
                    Severity::Warning,
                    false,
                    format!(
                        "agent \"{agent_id}\": command contains {{prompt}} but prompt_delivery=file; use {{prompt_file}} instead"
                    ),
                    None,
                ));
            }
            PromptDelivery::File if !cmd.contains("{prompt_file}") => {
                out.push(CheckResult::simple(
                    "prompt_delivery_missing_placeholder",
                    Severity::Warning,
                    false,
                    format!(
                        "agent \"{agent_id}\": prompt_delivery=file but command is missing {{prompt_file}} placeholder"
                    ),
                    None,
                ));
            }
            _ => {}
        }
    }
}

pub(super) fn check_capability_templates(
    agents: &std::collections::HashMap<String, agent_orchestrator::config::AgentConfig>,
    out: &mut Vec<CheckResult>,
) {
    for (agent_id, agent) in agents {
        let has_command = !agent.command.is_empty();
        out.push(CheckResult::simple(
            "agent_has_command",
            Severity::Error,
            has_command,
            if has_command {
                format!("agent \"{agent_id}\": has command configured")
            } else {
                format!("agent \"{agent_id}\": has no command configured")
            },
            None,
        ));
    }
}
