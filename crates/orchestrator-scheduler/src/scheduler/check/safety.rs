use super::CheckResult;
use agent_orchestrator::anomaly::Severity;
use agent_orchestrator::self_referential_policy::evaluate_self_referential_policy;

pub(super) fn check_self_referential_policy(
    workspaces: &std::collections::HashMap<String, agent_orchestrator::config::WorkspaceConfig>,
    workflows: &std::collections::HashMap<String, agent_orchestrator::config::WorkflowConfig>,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    for (workspace_id, workspace) in workspaces {
        if !workspace.self_referential {
            continue;
        }
        for (workflow_id, workflow) in workflows {
            if let Some(filter) = workflow_filter {
                if workflow_id != filter {
                    continue;
                }
            }
            match evaluate_self_referential_policy(workflow, workflow_id, workspace_id, true) {
                Ok(report) => out.extend(report.diagnostics.into_iter().map(CheckResult::from)),
                Err(err) => out.push(CheckResult::simple(
                    "self_ref.policy_evaluation_failed",
                    Severity::Error,
                    false,
                    format!(
                        "workflow \"{workflow_id}\" workspace \"{workspace_id}\": failed to evaluate self-referential policy: {err}"
                    ),
                    None,
                )),
            }
        }
    }
}
