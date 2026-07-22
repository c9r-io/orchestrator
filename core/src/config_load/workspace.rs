use crate::config::{OrchestratorConfig, ResolvedProject, ResolvedWorkspace, WorkspaceKind};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{
    ensure_within_root, validate_execution_profiles_for_project,
    validate_workflow_config_for_project,
};

/// Resolves a workspace-relative path and ensures it stays inside the workspace root.
pub fn resolve_workspace_path(
    workspace_root: &Path,
    rel_path: &str,
    field: &str,
) -> Result<PathBuf> {
    crate::qa_utils::validate_workspace_rel_path(rel_path, field)?;
    let joined = workspace_root.join(rel_path);
    if joined.exists() {
        ensure_within_root(workspace_root, &joined, field)?;
    } else if let Some(parent) = joined.parent() {
        if parent.exists() {
            ensure_within_root(workspace_root, parent, field)?;
        }
    }
    Ok(joined)
}

/// Resolves and validates workspaces for the default project.
pub fn resolve_and_validate_workspaces(
    data_dir: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedWorkspace>> {
    resolve_and_validate_workspaces_for_project(data_dir, config, crate::config::DEFAULT_PROJECT_ID)
}

/// Validate and resolve workspaces, agents, and workflows for a specific
/// project. Returns the resolved workspace map for that project.
pub fn resolve_and_validate_workspaces_for_project(
    data_dir: &Path,
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<HashMap<String, ResolvedWorkspace>> {
    let mut resolved = HashMap::new();
    let file_sharing = orchestrator_config::file_sharing::load_file_sharing_policy(data_dir)?;
    // Validate global Skill directories even before a task references them so
    // a malformed daemon ceiling can never degrade into runtime best effort.
    let _ = file_sharing.resolved_global_skills()?;
    let project = config
        .projects
        .get(project_id)
        .ok_or_else(|| anyhow::anyhow!("project '{}' does not exist", project_id))?;
    for (id, entry) in &project.workspaces {
        if id.trim().is_empty() {
            anyhow::bail!(
                "[INVALID_WORKSPACE] workspace id cannot be empty\n  category: validation\n  suggested_fix: provide a non-empty workspace name"
            );
        }
        match entry.kind {
            WorkspaceKind::CodeRepo => {
                if entry.root_path.trim().is_empty() {
                    anyhow::bail!(
                        "[CODE_REPO_WORK_DIR_REQUIRED] workspace '{}' requires work_dir",
                        id
                    );
                }
                if entry.qa_targets.is_empty() {
                    anyhow::bail!(
                        "[INVALID_WORKSPACE] workspace '{}' qa_targets cannot be empty\n  category: validation\n  suggested_fix: add at least one qa_targets path (e.g. docs/qa)",
                        id
                    );
                }
                if entry.ticket_dir.trim().is_empty() {
                    anyhow::bail!(
                        "[CODE_REPO_TICKET_DIR_REQUIRED] workspace '{}' requires ticket_dir",
                        id
                    );
                }
            }
            WorkspaceKind::Task => {
                if entry.self_referential {
                    anyhow::bail!(
                        "[TASK_WORKSPACE_SELF_REFERENTIAL_FORBIDDEN] workspace '{}' cannot be self_referential",
                        id
                    );
                }
                if !entry.qa_targets.is_empty() || !entry.ticket_dir.is_empty() {
                    anyhow::bail!(
                        "[TASK_WORKSPACE_QA_FIELDS_FORBIDDEN] workspace '{}' cannot define qa_targets or ticket_dir",
                        id
                    );
                }
            }
        }

        let root_path = if entry.root_path.trim().is_empty() {
            PathBuf::new()
        } else {
            let requested = data_dir.join(&entry.root_path);
            let root = requested.canonicalize().with_context(|| {
                format!("workspace '{}' work_dir not found: {}", id, entry.root_path)
            })?;
            if entry.kind == WorkspaceKind::Task {
                file_sharing.ensure_shareable(&root, &format!("workspace '{}'.work_dir", id))?;
            }
            root
        };

        for (idx, target) in entry.qa_targets.iter().enumerate() {
            let field = format!("workspace '{}' qa_targets[{}]", id, idx);
            let resolved_target = resolve_workspace_path(&root_path, target, &field)?;
            if resolved_target.exists() && !resolved_target.is_dir() {
                anyhow::bail!(
                    "{} must be a directory: {}",
                    field,
                    resolved_target.display()
                );
            }
        }
        if entry.kind == WorkspaceKind::CodeRepo {
            let ticket_field = format!("workspace '{}' ticket_dir", id);
            let resolved_ticket =
                resolve_workspace_path(&root_path, &entry.ticket_dir, &ticket_field)?;
            if resolved_ticket.exists() && !resolved_ticket.is_dir() {
                anyhow::bail!(
                    "{} must be a directory: {}",
                    ticket_field,
                    resolved_ticket.display()
                );
            }
        }

        let artifacts_dir = match (&entry.artifacts_dir, root_path.as_os_str().is_empty()) {
            (Some(rel), false) => root_path.join(rel),
            (None, false) => root_path.join(".orchestrator/artifacts"),
            (_, true) => data_dir.join("task-artifacts").join(id),
        };
        if !artifacts_dir.exists() {
            std::fs::create_dir_all(&artifacts_dir).with_context(|| {
                format!(
                    "workspace '{}' failed to create artifacts_dir: {}",
                    id,
                    artifacts_dir.display()
                )
            })?;
        }

        resolved.insert(
            id.clone(),
            ResolvedWorkspace {
                kind: entry.kind,
                root_path,
                qa_targets: entry.qa_targets.clone(),
                ticket_dir: entry.ticket_dir.clone(),
                artifacts_dir,
            },
        );
    }

    for (workflow_id, workflow) in &project.workflows {
        validate_workflow_config_for_project(config, workflow, workflow_id, Some(project_id))?;
        validate_execution_profiles_for_project(config, workflow, workflow_id, project_id)?;
    }

    Ok(resolved)
}

/// Resolves lightweight project snapshots without canonicalizing workspace paths.
pub fn resolve_and_validate_projects(
    data_dir: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedProject>> {
    let mut resolved = HashMap::new();
    for (project_id, project_config) in &config.projects {
        let mut workspaces = HashMap::new();
        for (workspace_id, workspace_config) in &project_config.workspaces {
            let root_path = if workspace_config.kind == WorkspaceKind::Task
                && workspace_config.root_path.trim().is_empty()
            {
                PathBuf::new()
            } else {
                data_dir.join(&workspace_config.root_path)
            };
            workspaces.insert(
                workspace_id.clone(),
                ResolvedWorkspace {
                    kind: workspace_config.kind,
                    root_path: root_path.clone(),
                    qa_targets: workspace_config.qa_targets.clone(),
                    ticket_dir: workspace_config.ticket_dir.clone(),
                    artifacts_dir: match (
                        &workspace_config.artifacts_dir,
                        root_path.as_os_str().is_empty(),
                    ) {
                        (_, true) => data_dir.join("task-artifacts").join(workspace_id),
                        (Some(rel), false) => root_path.join(rel),
                        (None, false) => root_path.join(".orchestrator/artifacts"),
                    },
                },
            );
        }
        resolved.insert(
            project_id.clone(),
            ResolvedProject {
                workspaces,
                agents: project_config.agents.clone(),
                workflows: project_config.workflows.clone(),
                step_templates: project_config.step_templates.clone(),
                env_stores: project_config.env_stores.clone(),
                secret_stores: project_config.secret_stores.clone(),
                execution_profiles: project_config.execution_profiles.clone(),
            },
        );
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OrchestratorConfig;
    use crate::config_load::tests::{make_builtin_step, make_workflow};
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn resolve_workspace_path_joins_rel_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "subdir/file.md", "test_field");
        assert!(result.is_ok());
        let path = result.expect("relative path should resolve");
        assert!(path.starts_with(&root));
        assert!(path.ends_with("subdir/file.md"));
    }

    #[test]
    fn resolve_workspace_path_rejects_absolute_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "/etc/passwd", "test_field");
        assert!(result.is_err(), "should reject absolute path");
    }

    #[test]
    fn resolve_workspace_path_rejects_empty_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "", "test_field");
        assert!(result.is_err(), "should reject empty path");
    }

    #[test]
    fn resolve_workspace_path_rejects_whitespace_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "   ", "test_field");
        assert!(result.is_err(), "should reject whitespace-only path");
    }

    #[test]
    fn resolve_workspace_path_validates_existing_path_within_root() {
        let root = std::env::temp_dir();
        let sub = root.join(format!("test-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&sub).expect("create nested workspace dir");
        let rel = sub
            .file_name()
            .and_then(|name| name.to_str())
            .expect("subdir should have valid UTF-8 file name");
        let result = resolve_workspace_path(&root, rel, "test_field");
        assert!(
            result.is_ok(),
            "existing subdir within root should pass: {:?}",
            result.err()
        );
        std::fs::remove_dir_all(&sub).ok();
    }

    #[test]
    fn resolve_and_validate_rejects_empty_workspaces() {
        let config = OrchestratorConfig::default();
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("project 'default' does not exist")
        );
    }

    #[test]
    fn resolve_and_validate_rejects_empty_agents() {
        use crate::config::{ProjectConfig, WorkspaceConfig};
        let config = OrchestratorConfig {
            projects: [(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: None,
                    workspaces: [(
                        "ws1".to_string(),
                        WorkspaceConfig {
                            kind: Default::default(),
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    agents: HashMap::new(),
                    workflows: HashMap::new(),
                    step_templates: HashMap::new(),
                    source_task_templates: HashMap::new(),
                    source_task_bindings: HashMap::new(),
                    env_stores: HashMap::new(),
                    secret_stores: HashMap::new(),
                    execution_profiles: HashMap::new(),
                    triggers: HashMap::new(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(
            result.is_ok(),
            "empty agent set is allowed for project-scoped config"
        );
    }

    #[test]
    fn resolve_and_validate_rejects_empty_workflows() {
        use crate::config::{AgentConfig, ProjectConfig, WorkspaceConfig};
        let config = OrchestratorConfig {
            projects: [(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: None,
                    workspaces: [(
                        "ws1".to_string(),
                        WorkspaceConfig {
                            kind: Default::default(),
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: HashMap::new(),
                    step_templates: HashMap::new(),
                    source_task_templates: HashMap::new(),
                    source_task_bindings: HashMap::new(),
                    env_stores: HashMap::new(),
                    secret_stores: HashMap::new(),
                    execution_profiles: HashMap::new(),
                    triggers: HashMap::new(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(
            result.is_ok(),
            "empty workflow set is allowed for project-scoped config"
        );
    }

    #[test]
    fn resolve_and_validate_rejects_empty_workspace_id() {
        use crate::config::{AgentConfig, ProjectConfig, WorkspaceConfig};
        let config = OrchestratorConfig {
            projects: [(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: None,
                    workspaces: [(
                        "".to_string(),
                        WorkspaceConfig {
                            kind: Default::default(),
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: [(
                        "wf1".to_string(),
                        make_workflow(vec![make_builtin_step("self_test", "self_test", true)]),
                    )]
                    .into(),
                    step_templates: Default::default(),
                    source_task_templates: Default::default(),
                    source_task_bindings: Default::default(),
                    env_stores: Default::default(),
                    secret_stores: Default::default(),
                    execution_profiles: Default::default(),
                    triggers: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("INVALID_WORKSPACE")
        );
    }

    #[test]
    fn resolve_and_validate_rejects_empty_qa_targets() {
        use crate::config::{AgentConfig, ProjectConfig, WorkspaceConfig};
        let config = OrchestratorConfig {
            projects: [(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: None,
                    workspaces: [(
                        "ws1".to_string(),
                        WorkspaceConfig {
                            kind: Default::default(),
                            root_path: "/tmp".to_string(),
                            qa_targets: vec![],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: [(
                        "wf1".to_string(),
                        make_workflow(vec![make_builtin_step("self_test", "self_test", true)]),
                    )]
                    .into(),
                    step_templates: Default::default(),
                    source_task_templates: Default::default(),
                    source_task_bindings: Default::default(),
                    env_stores: Default::default(),
                    secret_stores: Default::default(),
                    execution_profiles: Default::default(),
                    triggers: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("qa_targets cannot be empty")
        );
    }

    #[test]
    fn resolve_and_validate_rejects_missing_default_project_workflow() {
        use crate::config::{AgentConfig, ProjectConfig, WorkspaceConfig};
        let ws_root = std::env::temp_dir().join(format!("test-ws-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ws_root).expect("create workspace root");
        let qa_dir = ws_root.join("docs");
        std::fs::create_dir_all(&qa_dir).expect("create qa dir");
        let ticket_dir = ws_root.join("tickets");
        std::fs::create_dir_all(&ticket_dir).expect("create ticket dir");

        let config = OrchestratorConfig {
            projects: [(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: None,
                    workspaces: [(
                        "ws1".to_string(),
                        WorkspaceConfig {
                            kind: Default::default(),
                            root_path: ws_root.to_string_lossy().to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: Default::default(),
                    step_templates: Default::default(),
                    source_task_templates: Default::default(),
                    source_task_bindings: Default::default(),
                    env_stores: Default::default(),
                    secret_stores: Default::default(),
                    execution_profiles: Default::default(),
                    triggers: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/"), &config);
        assert!(
            result.is_ok(),
            "project-scoped workspace validation no longer requires workflows"
        );
        std::fs::remove_dir_all(&ws_root).ok();
    }

    #[test]
    fn resolve_and_validate_projects_empty_config() {
        let config = OrchestratorConfig::default();
        let result = resolve_and_validate_projects(Path::new("/tmp"), &config);
        assert!(result.is_ok());
        assert!(
            result
                .expect("empty project config should validate")
                .is_empty()
        );
    }

    #[test]
    fn task_workspace_without_work_dir_stays_unmaterialized_in_snapshot() {
        use crate::config::{ProjectConfig, WorkspaceConfig, WorkspaceKind};
        let data_dir = tempfile::tempdir().expect("data dir");
        let config = OrchestratorConfig {
            projects: [(
                "project".to_string(),
                ProjectConfig {
                    workspaces: [(
                        "task-workspace".to_string(),
                        WorkspaceConfig {
                            kind: WorkspaceKind::Task,
                            root_path: String::new(),
                            qa_targets: Vec::new(),
                            ticket_dir: String::new(),
                            self_referential: false,
                            health_policy: Default::default(),
                            artifacts_dir: None,
                        },
                    )]
                    .into(),
                    ..ProjectConfig::default()
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let projects = resolve_and_validate_projects(data_dir.path(), &config).expect("snapshot");
        let workspace = &projects["project"].workspaces["task-workspace"];
        assert!(workspace.root_path.as_os_str().is_empty());
        assert_eq!(
            workspace.artifacts_dir,
            data_dir.path().join("task-artifacts/task-workspace")
        );
    }

    #[test]
    fn resolve_and_validate_projects_resolves_workspaces() {
        use crate::config::{ProjectConfig, WorkspaceConfig};
        let mut projects = HashMap::new();
        let mut ws = HashMap::new();
        ws.insert(
            "proj-ws".to_string(),
            WorkspaceConfig {
                kind: Default::default(),
                root_path: "/app/some/absolute/path".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
                health_policy: Default::default(),
                artifacts_dir: None,
            },
        );
        projects.insert(
            "proj1".to_string(),
            ProjectConfig {
                description: None,
                workspaces: ws,
                agents: HashMap::new(),
                workflows: HashMap::new(),
                step_templates: HashMap::new(),
                source_task_templates: HashMap::new(),
                source_task_bindings: HashMap::new(),
                env_stores: HashMap::new(),
                secret_stores: HashMap::new(),
                execution_profiles: HashMap::new(),
                triggers: HashMap::new(),
            },
        );
        let config = OrchestratorConfig {
            projects,
            ..OrchestratorConfig::default()
        };
        let result =
            resolve_and_validate_projects(Path::new("/app"), &config).expect("resolve projects");
        assert!(result.contains_key("proj1"));
        let proj = &result["proj1"];
        assert!(proj.workspaces.contains_key("proj-ws"));
        assert!(proj.workspaces["proj-ws"].root_path.starts_with("/app"));
    }
}
