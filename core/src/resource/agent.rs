use crate::cli_types::{
    AgentMetadataSpec, AgentSelectionSpec, AgentSpec, HealthPolicySpec, OrchestratorResource,
    ResourceKind, ResourceSpec,
};
use crate::config::{
    AgentConfig, AgentMetadata, AgentSelectionConfig, HealthPolicyConfig, OrchestratorConfig,
    PromptDelivery,
};
use anyhow::{Result, anyhow};

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
        if self.spec.driver.is_none() {
            // FR-173: command-only Agents were accepted and promoted to
            // shell/cli at persist time. That promotion is gone, so the manifest
            // has to say which driver it means.
            return Err(anyhow!(
                "agent.spec.driver is required; a command-only Agent is no longer promoted to shell/cli — declare `driver: {{provider: shell, transport: cli}}` explicitly"
            ));
        }
        if let Some(driver) = self.spec.driver.as_ref() {
            crate::driver::validate_driver_config(driver, &self.spec.command)?;
            crate::driver::validate_driver_command_rules(driver, &self.spec.command_rules)?;
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
        driver: spec.driver.clone(),
        command_rules: spec.command_rules.clone(),
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
        health_policy: spec
            .health_policy
            .as_ref()
            .map(|hp| HealthPolicyConfig {
                disease_duration_hours: hp
                    .disease_duration_hours
                    .unwrap_or_else(|| HealthPolicyConfig::default().disease_duration_hours),
                disease_threshold: hp
                    .disease_threshold
                    .unwrap_or_else(|| HealthPolicyConfig::default().disease_threshold),
                capability_success_threshold: hp
                    .capability_success_threshold
                    .unwrap_or_else(|| HealthPolicyConfig::default().capability_success_threshold),
            })
            .unwrap_or_default(),
    }
}

/// Converts runtime agent config into its manifest spec representation.
pub(crate) fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        command: config.command.clone(),
        driver: config.driver.clone(),
        command_rules: config.command_rules.clone(),
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
        health_policy: if config.health_policy.is_default() {
            None
        } else {
            Some(HealthPolicySpec {
                disease_duration_hours: Some(config.health_policy.disease_duration_hours),
                disease_threshold: Some(config.health_policy.disease_threshold),
                capability_success_threshold: Some(
                    config.health_policy.capability_success_threshold,
                ),
            })
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::resource::{API_VERSION, dispatch_resource};

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

    /// The empty command is refused by the shell driver's own check now, not by
    /// the "command or driver" alternative FR-173 removed. The driver has to be
    /// declared for the assertion to reach that check at all — without one,
    /// validate stops at `driver is required` and this test would be measuring
    /// the wrong refusal.
    #[test]
    fn agent_validate_rejects_empty_command_under_a_shell_driver() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-empty-cmd"),
            spec: AgentSpec {
                enabled: None,
                command: "  ".to_string(),
                driver: Some(crate::config::AgentDriverConfig {
                    provider: crate::config::DriverProvider::Shell,
                    transport: crate::config::DriverTransport::Cli,
                    binary: None,
                    options: Default::default(),
                    claude: None,
                    codex: None,
                    shell: None,
                    raw_args: vec![],
                    unsafe_raw_args: false,
                }),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![],
            },
        };
        let err = agent.validate().expect_err("operation should fail");
        let message = err.to_string();
        assert!(
            message.contains("driver shell/cli requires agent.spec.command"),
            "{message}"
        );
    }

    /// The other half: no driver at all is its own refusal, and it must not be
    /// reported as a missing command.
    #[test]
    fn agent_validate_rejects_a_driverless_agent_by_name() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-driverless"),
            spec: AgentSpec {
                enabled: None,
                command: "echo {prompt}".to_string(),
                driver: None,
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![],
            },
        };
        let err = agent.validate().expect_err("a driverless Agent is refused");
        let message = err.to_string();
        assert!(
            message.contains("agent.spec.driver is required"),
            "{message}"
        );
        assert!(message.contains("provider: shell"), "{message}");
    }

    #[test]
    fn agent_validate_accepts_valid_command() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-valid"),
            spec: AgentSpec {
                enabled: None,
                command: "glmcode -p \"{prompt}\"".to_string(),
                driver: None,
                capabilities: Some(vec!["plan".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![],
            },
        };
        // FR-173: this used to apply with a `[legacy_agent_command_deprecated]`
        // warning and be promoted to shell/cli at persist time. Both are gone,
        // so the manifest has to say which driver it means — and the diagnostic
        // has to say so too, since an author who wrote `command:` and nothing
        // else needs the next step, not just a refusal.
        let error = agent
            .validate()
            .expect_err("a command-only Agent is no longer promoted")
            .to_string();
        assert!(error.contains("agent.spec.driver is required"), "{error}");
        assert!(error.contains("provider: shell"), "{error}");
    }

    #[test]
    fn agent_validate_rejects_command_rules_for_vendor_driver() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-vendor-rules"),
            spec: AgentSpec {
                enabled: None,
                command: String::new(),
                driver: Some(crate::config::AgentDriverConfig {
                    provider: crate::config::DriverProvider::Claude,
                    transport: crate::config::DriverTransport::Cli,
                    binary: None,
                    options: Default::default(),
                    claude: None,
                    codex: None,
                    shell: None,
                    raw_args: vec![],
                    unsafe_raw_args: false,
                }),
                capabilities: None,
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![crate::config::AgentCommandRule {
                    when: "true".to_string(),
                    command: "echo selected".to_string(),
                }],
            },
        };

        let error = agent
            .validate()
            .expect_err("vendor driver command rules should fail closed");
        assert!(error.to_string().contains("require driver shell/cli"));
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
                driver: None,
                selection: AgentSelectionConfig::default(),
                env: None,
                prompt_delivery: PromptDelivery::default(),
                health_policy: Default::default(),
                command_rules: Vec::new(),
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
        assert!(
            config
                .resource_store
                .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "meta-ag")
                .is_some()
        );

        AgentResource::delete_from(&mut config, "meta-ag");
        assert!(
            config
                .resource_store
                .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "meta-ag")
                .is_none()
        );
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
                driver: None,
                capabilities: Some(vec!["plan".to_string(), "implement".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![],
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
            driver: None,
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
            health_policy: None,
            command_rules: vec![],
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
            driver: None,
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
            health_policy: Default::default(),
            command_rules: Vec::new(),
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
            driver: None,
            selection: AgentSelectionConfig::default(),
            env: None,
            prompt_delivery: PromptDelivery::default(),
            health_policy: Default::default(),
            command_rules: Vec::new(),
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
                driver: None,
                capabilities: Some(vec!["qa".to_string()]),
                metadata: None,
                selection: None,
                env: None,
                prompt_delivery: None,
                health_policy: None,
                command_rules: vec![],
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
