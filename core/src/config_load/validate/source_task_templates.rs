use crate::config::{OrchestratorConfig, SourceTaskTemplateConfig};
use anyhow::{Result, bail};

/// Validates every source task template and its project-local references.
pub fn validate_source_task_templates(config: &OrchestratorConfig) -> Result<()> {
    for project_id in config.projects.keys() {
        validate_source_task_templates_for_project(config, project_id)?;
    }
    Ok(())
}

/// Validates source task templates belonging to one project.
pub fn validate_source_task_templates_for_project(
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<()> {
    let Some(project) = config.projects.get(project_id) else {
        bail!("project '{}' not found", project_id);
    };
    for (name, template) in &project.source_task_templates {
        validate_one(project_id, name, template, project)?;
    }
    Ok(())
}

fn validate_one(
    project_id: &str,
    name: &str,
    template: &SourceTaskTemplateConfig,
    project: &crate::config::ProjectConfig,
) -> Result<()> {
    crate::source_task_template::validate_template_config(template).map_err(|error| {
        anyhow::anyhow!(
            "SourceTaskTemplate '{}/{}' is invalid: {}",
            project_id,
            name,
            error
        )
    })?;
    if !project.workflows.contains_key(&template.action.workflow) {
        bail!(
            "SourceTaskTemplate '{}/{}' references workflow '{}' which does not exist in project '{}'",
            project_id,
            name,
            template.action.workflow,
            project_id
        );
    }
    if !project.workspaces.contains_key(&template.action.workspace) {
        bail!(
            "SourceTaskTemplate '{}/{}' references workspace '{}' which does not exist in project '{}'",
            project_id,
            name,
            template.action.workspace,
            project_id
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DEFAULT_PROJECT_ID, SourceTaskTemplateActionConfig, SourceTaskTemplateSkillConfig,
    };
    use crate::test_utils::TestState;
    use std::collections::BTreeMap;

    fn template() -> SourceTaskTemplateConfig {
        SourceTaskTemplateConfig {
            skill: SourceTaskTemplateSkillConfig {
                name: "docs".to_string(),
                invocation: "$docs".to_string(),
                args: vec![],
            },
            action: SourceTaskTemplateActionConfig {
                workflow: "basic".to_string(),
                workspace: "default".to_string(),
                start: true,
                initial_vars: BTreeMap::new(),
            },
            goal_template: "{source_message_url}".to_string(),
            allowed_variables: vec!["source_message_url".to_string()],
        }
    }

    #[test]
    fn validates_same_project_references() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = crate::config_load::read_active_config(&state)
            .expect("active config")
            .config
            .clone();
        config
            .projects
            .get_mut(DEFAULT_PROJECT_ID)
            .expect("default project")
            .source_task_templates
            .insert("docs".to_string(), template());
        validate_source_task_templates(&config).expect("valid references");
    }

    #[test]
    fn rejects_missing_workflow_and_workspace() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = crate::config_load::read_active_config(&state)
            .expect("active config")
            .config
            .clone();
        let project = config
            .projects
            .get_mut(DEFAULT_PROJECT_ID)
            .expect("default project");
        let mut value = template();
        value.action.workflow = "missing".to_string();
        project
            .source_task_templates
            .insert("docs".to_string(), value);
        let error = validate_source_task_templates(&config).expect_err("missing workflow");
        assert!(error.to_string().contains("references workflow 'missing'"));
    }
}
