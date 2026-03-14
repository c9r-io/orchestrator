use super::CheckResult;
use agent_orchestrator::anomaly::Severity;
use std::path::Path;

pub(super) fn check_workspace_roots(
    workspaces: &std::collections::HashMap<String, agent_orchestrator::config::WorkspaceConfig>,
    app_root: &Path,
    out: &mut Vec<CheckResult>,
) {
    for (ws_id, ws) in workspaces {
        let full = app_root.join(&ws.root_path);
        let exists = full.exists();
        out.push(CheckResult::simple(
            "workspace_root_missing",
            Severity::Error,
            exists,
            if exists {
                format!("workspace \"{ws_id}\": root path exists")
            } else {
                format!(
                    "workspace \"{ws_id}\": root path \"{}\" does not exist",
                    ws.root_path
                )
            },
            Some(full.display().to_string()),
        ));
    }
}

pub(super) fn check_qa_targets(
    workspaces: &std::collections::HashMap<String, agent_orchestrator::config::WorkspaceConfig>,
    app_root: &Path,
    out: &mut Vec<CheckResult>,
) {
    for (ws_id, ws) in workspaces {
        let ws_root = app_root.join(&ws.root_path);
        for target in &ws.qa_targets {
            let full = ws_root.join(target);
            let exists = full.exists();
            out.push(CheckResult::simple(
                "qa_targets_missing",
                Severity::Warning,
                exists,
                if exists {
                    format!("workspace \"{ws_id}\": qa_target \"{target}\" exists")
                } else {
                    format!("workspace \"{ws_id}\": qa_target \"{target}\" does not exist")
                },
                Some(full.display().to_string()),
            ));
        }
    }
}
