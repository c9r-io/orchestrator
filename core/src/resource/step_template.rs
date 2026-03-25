use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, StepTemplateSpec};
use crate::config::{OrchestratorConfig, StepTemplateConfig};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for `StepTemplate` resources.
pub struct StepTemplateResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the step template.
    pub spec: StepTemplateSpec,
}

impl Resource for StepTemplateResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::StepTemplate
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.prompt.trim().is_empty() {
            return Err(anyhow!("step_template.spec.prompt cannot be empty"));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = StepTemplateConfig {
            prompt: self.spec.prompt.clone(),
            description: self.spec.description.clone(),
        };
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.step_templates,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::StepTemplate,
            &self.metadata,
            ResourceSpec::StepTemplate(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .step_templates
            .get(name)
            .map(|tmpl| Self {
                metadata: super::metadata_with_name(name),
                spec: StepTemplateSpec {
                    prompt: tmpl.prompt.clone(),
                    description: tmpl.description.clone(),
                },
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| project.step_templates.remove(name).is_some())
            .unwrap_or(false)
    }
}

/// Builds a typed `StepTemplateResource` from a generic manifest wrapper.
pub(super) fn build_step_template(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::StepTemplate {
        return Err(anyhow!("resource kind/spec mismatch for StepTemplate"));
    }
    match spec {
        ResourceSpec::StepTemplate(spec) => {
            Ok(RegisteredResource::StepTemplate(StepTemplateResource {
                metadata,
                spec,
            }))
        }
        _ => Err(anyhow!("resource kind/spec mismatch for StepTemplate")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::dispatch_resource;

    use super::super::test_fixtures::{make_config, step_template_manifest};

    #[test]
    fn step_template_dispatch_and_kind() {
        let resource = dispatch_resource(step_template_manifest("plan", "You are a planner."))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::StepTemplate);
        assert_eq!(resource.name(), "plan");
    }

    #[test]
    fn step_template_validate_accepts_valid() {
        let resource = dispatch_resource(step_template_manifest("plan", "You are a planner."))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn step_template_validate_rejects_empty_name() {
        let resource = dispatch_resource(step_template_manifest("", "prompt"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_err());
    }

    #[test]
    fn step_template_validate_rejects_empty_prompt() {
        let tmpl = StepTemplateResource {
            metadata: super::super::metadata_with_name("empty-prompt"),
            spec: StepTemplateSpec {
                prompt: "  ".to_string(),
                description: None,
            },
        };
        let err = tmpl.validate().expect_err("should reject empty prompt");
        assert!(err.to_string().contains("prompt cannot be empty"));
    }

    #[test]
    fn step_template_apply_created_then_unchanged() {
        let mut config = make_config();
        let resource = dispatch_resource(step_template_manifest("plan", "You are a planner."))
            .expect("dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn step_template_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(step_template_manifest("plan", "v1"))
            .expect("dispatch should succeed");
        assert_eq!(r1.apply(&mut config).expect("apply"), ApplyResult::Created);
        let r2 = dispatch_resource(step_template_manifest("plan", "v2"))
            .expect("dispatch should succeed");
        assert_eq!(
            r2.apply(&mut config).expect("apply"),
            ApplyResult::Configured
        );
    }

    #[test]
    fn step_template_get_from_and_delete_from() {
        let mut config = make_config();
        let resource = dispatch_resource(step_template_manifest("plan", "prompt text"))
            .expect("dispatch should succeed");
        resource.apply(&mut config).expect("apply");

        let loaded = StepTemplateResource::get_from(&config, "plan");
        let loaded = loaded.expect("should be found after apply");
        assert_eq!(loaded.spec.prompt, "prompt text");

        assert!(StepTemplateResource::delete_from(&mut config, "plan"));
        assert!(StepTemplateResource::get_from(&config, "plan").is_none());
    }

    #[test]
    fn step_template_to_yaml() {
        let resource = dispatch_resource(step_template_manifest("plan", "You are a planner."))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: StepTemplate"));
        assert!(yaml.contains("plan"));
        assert!(yaml.contains("planner"));
    }

    #[test]
    fn step_template_yaml_roundtrip() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  prompt: "You are a planner for {source_tree}."
  description: "Planning template"
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        resource
            .validate_version()
            .expect("version should be valid");
        assert_eq!(resource.kind, ResourceKind::StepTemplate);
        if let ResourceSpec::StepTemplate(ref spec) = resource.spec {
            assert!(spec.prompt.contains("planner"));
            assert_eq!(spec.description.as_deref(), Some("Planning template"));
        } else {
            panic!("expected StepTemplate spec");
        }
    }
}
