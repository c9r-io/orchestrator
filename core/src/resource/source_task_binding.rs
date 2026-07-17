use crate::cli_types::{
    OrchestratorResource, ResourceKind, ResourceSpec, SourceTaskBindingMatchSpec,
    SourceTaskBindingSpec,
};
use crate::config::{OrchestratorConfig, SourceTaskBindingConfig, SourceTaskBindingMatchConfig};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for project-scoped `SourceTaskBinding` resources.
pub struct SourceTaskBindingResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload.
    pub spec: SourceTaskBindingSpec,
}

impl SourceTaskBindingResource {
    fn config(&self) -> SourceTaskBindingConfig {
        SourceTaskBindingConfig {
            trigger_ref: self.spec.trigger_ref.clone(),
            match_rule: SourceTaskBindingMatchConfig {
                event_kind: self.spec.match_rule.event_kind.clone(),
                reaction: self.spec.match_rule.reaction.clone(),
                target_kind: self.spec.match_rule.target_kind.clone(),
                channels: self.spec.match_rule.channels.clone(),
                all_channels: self.spec.match_rule.all_channels,
            },
            template_ref: self.spec.template_ref.clone(),
            allowed_actor_roles: self.spec.allowed_actor_roles.clone(),
            suspend: self.spec.suspend,
        }
    }

    fn spec(config: &SourceTaskBindingConfig) -> SourceTaskBindingSpec {
        SourceTaskBindingSpec {
            trigger_ref: config.trigger_ref.clone(),
            match_rule: SourceTaskBindingMatchSpec {
                event_kind: config.match_rule.event_kind.clone(),
                reaction: config.match_rule.reaction.clone(),
                target_kind: config.match_rule.target_kind.clone(),
                channels: config.match_rule.channels.clone(),
                all_channels: config.match_rule.all_channels,
            },
            template_ref: config.template_ref.clone(),
            allowed_actor_roles: config.allowed_actor_roles.clone(),
            suspend: config.suspend,
        }
    }
}

impl Resource for SourceTaskBindingResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::SourceTaskBinding
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        crate::source_task_binding::validate_binding_config(&self.config())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = self.config();
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.source_task_bindings,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::SourceTaskBinding,
            &self.metadata,
            ResourceSpec::SourceTaskBinding(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .source_task_bindings
            .get(name)
            .map(|binding| Self {
                metadata: super::metadata_with_name(name),
                spec: Self::spec(binding),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| project.source_task_bindings.remove(name).is_some())
            .unwrap_or(false)
    }
}

/// Builds a typed source task binding from a generic manifest.
pub(super) fn build_source_task_binding(
    resource: OrchestratorResource,
) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::SourceTaskBinding {
        return Err(anyhow!("resource kind/spec mismatch for SourceTaskBinding"));
    }
    match spec {
        ResourceSpec::SourceTaskBinding(spec) => Ok(RegisteredResource::SourceTaskBinding(
            SourceTaskBindingResource { metadata, spec },
        )),
        _ => Err(anyhow!("resource kind/spec mismatch for SourceTaskBinding")),
    }
}
