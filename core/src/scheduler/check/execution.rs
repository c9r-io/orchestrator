use super::CheckResult;
use crate::anomaly::Severity;
use crate::runner::{sandbox_backend_preflight_issues, ResolvedExecutionProfile};
use std::path::Path;

pub(super) fn check_execution_profile_backend_support(
    workspaces: &std::collections::HashMap<String, crate::config::WorkspaceConfig>,
    workflows: &std::collections::HashMap<String, crate::config::WorkflowConfig>,
    project_id: &str,
    projects: &std::collections::HashMap<String, crate::config::ProjectConfig>,
    app_root: &Path,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let Some(project) = projects.get(project_id) else {
        return;
    };
    let workspace_root = workspaces
        .values()
        .next()
        .map(|ws| app_root.join(&ws.root_path))
        .unwrap_or_else(|| app_root.to_path_buf());

    for (workflow_id, workflow) in workflows {
        if let Some(filter) = workflow_filter {
            if workflow_id != filter {
                continue;
            }
        }
        for step in &workflow.steps {
            let Some(profile_name) = step.execution_profile.as_deref() else {
                continue;
            };
            let Some(profile) = project.execution_profiles.get(profile_name) else {
                continue;
            };
            let resolved =
                ResolvedExecutionProfile::from_config(profile_name, profile, &workspace_root, &[]);
            for issue in sandbox_backend_preflight_issues(&resolved) {
                let severity =
                    if resolved.network_mode == crate::config::ExecutionNetworkMode::Allowlist {
                        Severity::Error
                    } else {
                        Severity::Warning
                    };
                out.push(CheckResult::simple(
                    "execution_profile_backend_support",
                    severity,
                    false,
                    format!(
                        "workflow \"{workflow_id}\" step \"{}\" execution profile \"{profile_name}\": {issue}",
                        step.id
                    ),
                    None,
                ));
            }
        }
    }
}
