use crate::cli_types::{
    OrchestratorResource, ResourceKind, ResourceSpec, SourceTaskTemplateActionSpec,
    SourceTaskTemplateSkillSpec, SourceTaskTemplateSpec,
};
use crate::config::{
    OrchestratorConfig, SourceTaskTemplateActionConfig, SourceTaskTemplateConfig,
    SourceTaskTemplateSkillConfig,
};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for project-scoped `SourceTaskTemplate` resources.
pub struct SourceTaskTemplateResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload.
    pub spec: SourceTaskTemplateSpec,
}

impl SourceTaskTemplateResource {
    fn config(&self) -> SourceTaskTemplateConfig {
        SourceTaskTemplateConfig {
            skill: SourceTaskTemplateSkillConfig {
                name: self.spec.skill.name.clone(),
                invocation: self.spec.skill.invocation.clone(),
                args: self.spec.skill.args.clone(),
            },
            action: SourceTaskTemplateActionConfig {
                workflow: self.spec.action.workflow.clone(),
                workspace: self.spec.action.workspace.clone(),
                start: self.spec.action.start,
                initial_vars: self.spec.action.initial_vars.clone(),
            },
            goal_template: self.spec.goal_template.clone(),
            allowed_variables: self.spec.allowed_variables.clone(),
        }
    }

    fn spec(config: &SourceTaskTemplateConfig) -> SourceTaskTemplateSpec {
        SourceTaskTemplateSpec {
            skill: SourceTaskTemplateSkillSpec {
                name: config.skill.name.clone(),
                invocation: config.skill.invocation.clone(),
                args: config.skill.args.clone(),
            },
            action: SourceTaskTemplateActionSpec {
                workflow: config.action.workflow.clone(),
                workspace: config.action.workspace.clone(),
                start: config.action.start,
                initial_vars: config.action.initial_vars.clone(),
            },
            goal_template: config.goal_template.clone(),
            allowed_variables: config.allowed_variables.clone(),
        }
    }
}

impl Resource for SourceTaskTemplateResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::SourceTaskTemplate
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        crate::source_task_template::validate_template_config(&self.config())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = self.config();
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.source_task_templates,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::SourceTaskTemplate,
            &self.metadata,
            ResourceSpec::SourceTaskTemplate(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .source_task_templates
            .get(name)
            .map(|template| Self {
                metadata: super::metadata_with_name(name),
                spec: Self::spec(template),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| project.source_task_templates.remove(name).is_some())
            .unwrap_or(false)
    }
}

/// Builds a typed source task template from a generic manifest.
pub(super) fn build_source_task_template(
    resource: OrchestratorResource,
) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::SourceTaskTemplate {
        return Err(anyhow!(
            "resource kind/spec mismatch for SourceTaskTemplate"
        ));
    }
    match spec {
        ResourceSpec::SourceTaskTemplate(spec) => Ok(RegisteredResource::SourceTaskTemplate(
            SourceTaskTemplateResource { metadata, spec },
        )),
        _ => Err(anyhow!(
            "resource kind/spec mismatch for SourceTaskTemplate"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::dispatch_resource;
    use crate::resource::test_fixtures::{make_config, source_task_template_manifest};

    #[test]
    fn dispatch_apply_get_delete_and_yaml_round_trip() {
        let mut config = make_config();
        let resource = dispatch_resource(source_task_template_manifest("slack-docs"))
            .expect("dispatch template");
        assert_eq!(resource.kind(), ResourceKind::SourceTaskTemplate);
        resource.validate().expect("valid template");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );

        let loaded = SourceTaskTemplateResource::get_from(&config, "slack-docs")
            .expect("template should exist");
        assert_eq!(loaded.spec.skill.invocation, "$docs");
        let yaml = loaded.to_yaml().expect("serialize template");
        let parsed: OrchestratorResource = serde_yaml::from_str(&yaml).expect("parse template");
        assert_eq!(parsed.kind, ResourceKind::SourceTaskTemplate);

        assert!(SourceTaskTemplateResource::delete_from(
            &mut config,
            "slack-docs"
        ));
        assert!(SourceTaskTemplateResource::get_from(&config, "slack-docs").is_none());
    }

    #[test]
    fn validation_rejects_unknown_variable_and_excessive_args() {
        let mut manifest = source_task_template_manifest("bad");
        let ResourceSpec::SourceTaskTemplate(spec) = &mut manifest.spec else {
            panic!("source task template fixture");
        };
        spec.allowed_variables.push("arbitrary".to_string());
        let resource = dispatch_resource(manifest).expect("dispatch template");
        assert!(resource.validate().is_err());
    }
}
