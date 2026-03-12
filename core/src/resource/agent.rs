use crate::cli_types::{
    AgentMetadataSpec, AgentSelectionSpec, AgentSpec, OrchestratorResource, ResourceKind,
    ResourceSpec,
};
use crate::config::{
    AgentConfig, AgentMetadata, AgentSelectionConfig, OrchestratorConfig, PromptDelivery,
};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for `Agent` resources.
pub struct AgentResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the agent.
    pub spec: AgentSpec,
}

impl Resource for AgentResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Agent
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.command.trim().is_empty() {
            return Err(anyhow!("agent.spec.command cannot be empty"));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let mut metadata = self.metadata.clone();
        metadata.project = Some(
            config
                .effective_project_id(metadata.project.as_deref())
                .to_string(),
        );
        Ok(super::apply_to_store(
            config,
            "Agent",
            self.name(),
            &metadata,
            serde_json::to_value(&self.spec)?,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Agent,
            &self.metadata,
            ResourceSpec::Agent(Box::new(self.spec.clone())),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .agents
            .get(name)
            .map(|agent| Self {
                metadata: super::metadata_from_store(config, "Agent", name, project_id),
                spec: agent_config_to_spec(agent),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        super::helpers::delete_from_store_project(config, "Agent", name, project_id)
    }
}

/// Builds a typed `AgentResource` from a generic manifest wrapper.
pub(super) fn build_agent(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Agent {
        return Err(anyhow!("resource kind/spec mismatch for Agent"));
    }
    match spec {
        ResourceSpec::Agent(spec) => Ok(RegisteredResource::Agent(Box::new(AgentResource {
            metadata,
            spec: *spec,
        }))),
        _ => Err(anyhow!("resource kind/spec mismatch for Agent")),
    }
}

/// Converts an `AgentSpec` manifest payload into runtime config.
pub(crate) fn agent_spec_to_config(spec: &AgentSpec) -> AgentConfig {
    let capabilities = spec.capabilities.clone().unwrap_or_default();

    AgentConfig {
        metadata: AgentMetadata {
            name: String::new(),
            description: spec.metadata.as_ref().and_then(|m| m.description.clone()),
            version: None,
            cost: spec.metadata.as_ref().and_then(|m| m.cost),
        },
        enabled: spec.enabled.unwrap_or(true),
        capabilities,
        command: spec.command.clone(),
        selection: spec
            .selection
            .as_ref()
            .map(|selection| AgentSelectionConfig {
                strategy: selection.strategy,
                weights: selection.weights.clone(),
            })
            .unwrap_or_default(),
        env: spec.env.clone(),
        prompt_delivery: spec.prompt_delivery.unwrap_or_default(),
    }
}

/// Converts runtime agent config into its manifest spec representation.
pub(crate) fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        command: config.command.clone(),
        enabled: if config.enabled { None } else { Some(false) },
        capabilities: if config.capabilities.is_empty() {
            None
        } else {
            Some(config.capabilities.clone())
        },
        metadata: if config.metadata.description.is_none() && config.metadata.cost.is_none() {
            None
        } else {
            Some(AgentMetadataSpec {
                cost: config.metadata.cost,
                description: config.metadata.description.clone(),
            })
        },
        selection: Some(AgentSelectionSpec {
            strategy: config.selection.strategy,
            weights: config.selection.weights.clone(),
        }),
        env: config.env.clone(),
        prompt_delivery: if config.prompt_delivery == PromptDelivery::Arg {
            None
        } else {
            Some(config.prompt_delivery)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::resource::{dispatch_resource, API_VERSION};

    use super::super::test_fixtures::{agent_manifest, make_config};

    #[test]
    fn agent_resource_apply() {
        let mut config = make_config();

        let resource =
            dispatch_resource(agent_manifest("agent-roundtrip", "glmcode -p \"{prompt}\""))
                .expect("agent dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );

        let loaded = AgentResource::get_from(&config, "agent-roundtrip")
            .expect("agent should be present in config");
        assert!(loaded.spec.command.contains("{prompt}"));
        assert_eq!(loaded.kind(), ResourceKind::Agent);
    }

    #[test]
    fn agent_validate_rejects_empty_command() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-empty-cmd"),
            spec: AgentSpec {
                enabled: None,
                command: "  ".to_string(),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            },
        };
        let err = agent.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("command cannot be empty"));
    }

    #[test]
    fn agent_validate_accepts_valid_command() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-valid"),
            spec: AgentSpec {
                enabled: None,
                command: "glmcode -p \"{prompt}\"".to_string(),
                capabilities: Some(vec!["plan".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            },
        };
        assert!(agent.validate().is_ok());
    }

    #[test]
    fn agent_get_from_without_stored_metadata() {
        let mut config = make_config();
        config.ensure_project(None).agents.insert(
            "bare-ag".to_string(),
            AgentConfig {
                enabled: true,
                metadata: AgentMetadata::default(),
                capabilities: vec!["qa".to_string()],
                command: "glmcode -p \"{prompt}\"".to_string(),
                selection: AgentSelectionConfig::default(),
                env: None,
                prompt_delivery: PromptDelivery::default(),
            },
        );
        let loaded =
            AgentResource::get_from(&config, "bare-ag").expect("bare agent should be returned");
        assert_eq!(loaded.metadata.name, "bare-ag");
        assert!(loaded.metadata.labels.is_none());
    }

    #[test]
    fn agent_get_from_returns_none_for_missing() {
        let config = make_config();
        assert!(AgentResource::get_from(&config, "nonexistent-ag").is_none());
    }

    #[test]
    fn agent_delete_cleans_up_metadata() {
        let mut config = make_config();
        let ag = dispatch_resource(agent_manifest("meta-ag", "glmcode -p \"{prompt}\""))
            .expect("dispatch agent resource");
        ag.apply(&mut config).expect("apply");
        assert!(config
            .resource_store
            .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "meta-ag")
            .is_some());

        AgentResource::delete_from(&mut config, "meta-ag");
        assert!(config
            .resource_store
            .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "meta-ag")
            .is_none());
    }

    #[test]
    fn agent_to_yaml_includes_command() {
        let agent = AgentResource {
            metadata: ResourceMetadata {
                name: "full-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: AgentSpec {
                enabled: None,
                command: "glmcode -p \"{prompt}\" --verbose".to_string(),
                capabilities: Some(vec!["plan".to_string(), "implement".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            },
        };
        let yaml = agent.to_yaml().expect("should serialize");
        assert!(yaml.contains("full-agent"));
        assert!(yaml.contains("glmcode"));
        assert!(yaml.contains("{prompt}"));
    }

    #[test]
    fn agent_spec_config_roundtrip() {
        let spec = AgentSpec {
            enabled: None,
            command: "glmcode -p \"{prompt}\" --verbose".to_string(),
            capabilities: Some(vec!["plan".to_string(), "implement".to_string()]),
            metadata: Some(AgentMetadataSpec {
                cost: Some(2),
                description: Some("A test agent".to_string()),
            }),
            selection: Some(AgentSelectionSpec {
                strategy: Default::default(),
                weights: None,
            }),
            env: None,
            prompt_delivery: None,
        };

        let config = agent_spec_to_config(&spec);
        assert_eq!(config.command, "glmcode -p \"{prompt}\" --verbose");
        assert!(config.capabilities.contains(&"plan".to_string()));
        assert!(config.capabilities.contains(&"implement".to_string()));

        let roundtripped = agent_config_to_spec(&config);
        assert_eq!(roundtripped.command, spec.command);
        assert!(roundtripped.capabilities.is_some());
        let rt_meta = roundtripped.metadata.expect("metadata should be preserved");
        assert_eq!(rt_meta.cost, Some(2));
        assert_eq!(rt_meta.description, Some("A test agent".to_string()));
    }

    #[test]
    fn agent_config_to_spec_empty_capabilities_becomes_none() {
        let config = AgentConfig {
            enabled: true,
            metadata: AgentMetadata::default(),
            capabilities: vec![],
            command: "echo".to_string(),
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
        };
        let spec = agent_config_to_spec(&config);
        assert!(spec.capabilities.is_none());
    }

    #[test]
    fn agent_config_to_spec_no_metadata_becomes_none() {
        let config = AgentConfig {
            enabled: true,
            metadata: AgentMetadata {
                name: String::new(),
                description: None,
                version: None,
                cost: None,
            },
            capabilities: vec![],
            command: "echo".to_string(),
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
        };
        let spec = agent_config_to_spec(&config);
        assert!(spec.metadata.is_none());
    }

    #[test]
    fn agent_apply_stores_resource_metadata() {
        let mut config = make_config();
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::Agent,
            metadata: ResourceMetadata {
                name: "store-meta-ag".to_string(),
                project: None,
                labels: Some([("tier".to_string(), "primary".to_string())].into()),
                annotations: None,
            },
            spec: ResourceSpec::Agent(Box::new(AgentSpec {
                enabled: None,
                command: "glmcode -p \"{prompt}\"".to_string(),
                capabilities: Some(vec!["qa".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
            })),
        };
        let rr = dispatch_resource(resource).expect("dispatch agent resource");
        rr.apply(&mut config).expect("apply");

        let cr = config
            .resource_store
            .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "store-meta-ag")
            .expect("stored agent CR should exist");
        assert_eq!(
            cr.metadata
                .labels
                .as_ref()
                .expect("labels should exist")
                .get("tier")
                .expect("tier label should exist"),
            "primary"
        );
    }
}
