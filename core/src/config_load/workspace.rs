use crate::config::{OrchestratorConfig, ResolvedProject, ResolvedWorkspace};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{ensure_within_root, validate_workflow_config_with_agents};

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

pub fn resolve_and_validate_workspaces(
    app_root: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedWorkspace>> {
    resolve_and_validate_workspaces_for_project(app_root, config, crate::config::DEFAULT_PROJECT_ID)
}

/// Validate and resolve workspaces, agents, and workflows for a specific
/// project. Returns the resolved workspace map for that project.
pub fn resolve_and_validate_workspaces_for_project(
    app_root: &Path,
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<HashMap<String, ResolvedWorkspace>> {
    let mut resolved = HashMap::new();
    let project = config
        .projects
        .get(project_id)
        .ok_or_else(|| anyhow::anyhow!("project '{}' does not exist", project_id))?;
    for (id, entry) in &project.workspaces {
        if id.trim().is_empty() {
            anyhow::bail!("[INVALID_WORKSPACE] workspace id cannot be empty\n  category: validation\n  suggested_fix: provide a non-empty workspace name");
        }
        if entry.qa_targets.is_empty() {
            anyhow::bail!("[INVALID_WORKSPACE] workspace '{}' qa_targets cannot be empty\n  category: validation\n  suggested_fix: add at least one qa_targets path (e.g. docs/qa)", id);
        }

        let root_path = app_root
            .join(&entry.root_path)
            .canonicalize()
            .with_context(|| {
                format!(
                    "workspace '{}' root_path not found: {}",
                    id, entry.root_path
                )
            })?;

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
        let ticket_field = format!("workspace '{}' ticket_dir", id);
        let resolved_ticket = resolve_workspace_path(&root_path, &entry.ticket_dir, &ticket_field)?;
        if resolved_ticket.exists() && !resolved_ticket.is_dir() {
            anyhow::bail!(
                "{} must be a directory: {}",
                ticket_field,
                resolved_ticket.display()
            );
        }

        resolved.insert(
            id.clone(),
            ResolvedWorkspace {
                root_path,
                qa_targets: entry.qa_targets.clone(),
                ticket_dir: entry.ticket_dir.clone(),
            },
        );
    }

    let project_agents: HashMap<String, &crate::config::AgentConfig> = project
        .agents
        .iter()
        .map(|(k, v)| (k.clone(), v))
        .collect();
    for (workflow_id, workflow) in &project.workflows {
        validate_workflow_config_with_agents(&project_agents, workflow, workflow_id)?;
    }

    Ok(resolved)
}

pub fn resolve_and_validate_projects(
    app_root: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedProject>> {
    let mut resolved = HashMap::new();
    for (project_id, project_config) in &config.projects {
        let mut workspaces = HashMap::new();
        for (workspace_id, workspace_config) in &project_config.workspaces {
            let root_path = app_root.join(&workspace_config.root_path);
            workspaces.insert(
                workspace_id.clone(),
                ResolvedWorkspace {
                    root_path,
                    qa_targets: workspace_config.qa_targets.clone(),
                    ticket_dir: workspace_config.ticket_dir.clone(),
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
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("EMPTY_WORKSPACES"));
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
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                        },
                    )]
                    .into(),
                    agents: HashMap::new(),
                    workflows: HashMap::new(),
                    step_templates: HashMap::new(),
                    env_stores: HashMap::new(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("EMPTY_AGENTS"));
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
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: HashMap::new(),
                    step_templates: HashMap::new(),
                    env_stores: HashMap::new(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("EMPTY_WORKFLOWS"));
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
                            root_path: "/tmp".to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
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
                    env_stores: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("INVALID_WORKSPACE"));
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
                            root_path: "/tmp".to_string(),
                            qa_targets: vec![],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
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
                    env_stores: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("qa_targets cannot be empty"));
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
                            root_path: ws_root.to_string_lossy().to_string(),
                            qa_targets: vec!["docs".to_string()],
                            ticket_dir: "tickets".to_string(),
                            self_referential: false,
                        },
                    )]
                    .into(),
                    agents: [("agent1".to_string(), AgentConfig::default())].into(),
                    workflows: Default::default(),
                    step_templates: Default::default(),
                    env_stores: Default::default(),
                },
            )]
            .into(),
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/"), &config);
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("at least one workflow"));
        std::fs::remove_dir_all(&ws_root).ok();
    }

    #[test]
    fn resolve_and_validate_projects_empty_config() {
        let config = OrchestratorConfig::default();
        let result = resolve_and_validate_projects(Path::new("/tmp"), &config);
        assert!(result.is_ok());
        assert!(result
            .expect("empty project config should validate")
            .is_empty());
    }

    #[test]
    fn resolve_and_validate_projects_resolves_workspaces() {
        use crate::config::{ProjectConfig, WorkspaceConfig};
        let mut projects = HashMap::new();
        let mut ws = HashMap::new();
        ws.insert(
            "proj-ws".to_string(),
            WorkspaceConfig {
                root_path: "some/relative/path".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
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
                env_stores: HashMap::new(),
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
