use super::CheckResult;
use agent_orchestrator::anomaly::Severity;
use agent_orchestrator::runner::{
    ResolvedExecutionProfile, sandbox_backend_label, sandbox_backend_preflight_issues,
};
use std::path::Path;

pub(super) fn check_execution_profile_backend_support(
    workspaces: &std::collections::HashMap<String, agent_orchestrator::config::WorkspaceConfig>,
    workflows: &std::collections::HashMap<String, agent_orchestrator::config::WorkflowConfig>,
    project_id: &str,
    projects: &std::collections::HashMap<String, agent_orchestrator::config::ProjectConfig>,
    data_dir: &Path,
    workflow_filter: Option<&str>,
    out: &mut Vec<CheckResult>,
) {
    let Some(project) = projects.get(project_id) else {
        return;
    };
    let workspace_root = workspaces
        .values()
        .next()
        .map(|ws| data_dir.join(&ws.root_path))
        .unwrap_or_else(|| data_dir.to_path_buf());

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
            let backend = sandbox_backend_label(&resolved);
            for issue in sandbox_backend_preflight_issues(&resolved) {
                // All preflight issues from detect_linux_sandbox_support are hard
                // blockers (root, missing binaries, unsupported fs_mode) — treat
                // them as Error, not Warning, so `orchestrator check` clearly
                // signals that the step will fail at runtime.
                let severity = if resolved.mode
                    == agent_orchestrator::config::ExecutionProfileMode::Sandbox
                {
                    Severity::Error
                } else {
                    Severity::Warning
                };
                out.push(
                    CheckResult::simple(
                        "execution_profile_backend_support",
                        severity,
                        false,
                        format!(
                            "workflow \"{workflow_id}\" step \"{}\" execution profile \"{profile_name}\": {issue}",
                            step.id
                        ),
                        None,
                    )
                    .with_details(
                        format!(
                            "mode={:?}, fs_mode={:?}, network_mode={:?}, sandbox_backend={}",
                            resolved.mode, resolved.fs_mode, resolved.network_mode, backend
                        ),
                        "sandbox backend supports requested execution profile",
                        "step will fail at runtime; the sandbox backend cannot satisfy the profile requirements",
                        "check platform prerequisites or adjust the execution profile to match backend capabilities",
                    ),
                );
            }
        }
    }
}
