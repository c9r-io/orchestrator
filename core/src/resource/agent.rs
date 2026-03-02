use crate::cli_types::{
    AgentMetadataSpec, AgentSelectionSpec, AgentSpec, AgentTemplatesSpec, OrchestratorResource,
    ResourceKind, ResourceSpec,
};
use crate::config::{AgentConfig, AgentMetadata, AgentSelectionConfig, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct AgentResource {
    pub metadata: ResourceMetadata,
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
        let templates = &self.spec.templates;
        let all_templates: Vec<Option<&str>> = vec![
            templates.init_once.as_deref(),
            templates.qa.as_deref(),
            templates.plan.as_deref(),
            templates.fix.as_deref(),
            templates.retest.as_deref(),
            templates.loop_guard.as_deref(),
            templates.ticket_scan.as_deref(),
            templates.build.as_deref(),
            templates.test.as_deref(),
            templates.lint.as_deref(),
            templates.implement.as_deref(),
            templates.review.as_deref(),
            templates.git_ops.as_deref(),
        ];
        let has_named_template = all_templates.iter().any(|t| t.is_some());
        let has_extra_template = !templates.extra.is_empty();
        if !has_named_template && !has_extra_template {
            return Err(anyhow!(
                "agent.spec.templates must define at least one template"
            ));
        }
        for value in &all_templates {
            if matches!(value, Some(raw) if raw.trim().is_empty()) {
                return Err(anyhow!(
                    "agent.spec.templates entries cannot be empty strings"
                ));
            }
        }
        for (name, value) in &templates.extra {
            if value.trim().is_empty() {
                return Err(anyhow!(
                    "agent.spec.templates.{} cannot be an empty string",
                    name
                ));
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> ApplyResult {
        let incoming = agent_spec_to_config(&self.spec);
        let result = super::apply_to_map(&mut config.agents, self.name(), incoming);
        config.resource_meta.agents.insert(
            self.name().to_string(),
            crate::config::ResourceStoredMetadata {
                labels: self.metadata.labels.clone(),
                annotations: self.metadata.annotations.clone(),
            },
        );
        result
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Agent,
            &self.metadata,
            ResourceSpec::Agent(Box::new(self.spec.clone())),
        )
    }

    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        config.agents.get(name).map(|agent| Self {
            metadata: match config.resource_meta.agents.get(name) {
                Some(stored) => super::metadata_from_parts(
                    name,
                    None,
                    stored.labels.clone(),
                    stored.annotations.clone(),
                ),
                None => super::metadata_with_name(name),
            },
            spec: agent_config_to_spec(agent),
        })
    }

    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        let removed = config.agents.remove(name).is_some();
        if removed {
            config.resource_meta.agents.remove(name);
        }
        removed
    }
}

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

pub(super) fn agent_spec_to_config(spec: &AgentSpec) -> AgentConfig {
    let named_templates: Vec<(&str, &Option<String>)> = vec![
        ("init_once", &spec.templates.init_once),
        ("plan", &spec.templates.plan),
        ("qa", &spec.templates.qa),
        ("ticket_scan", &spec.templates.ticket_scan),
        ("fix", &spec.templates.fix),
        ("retest", &spec.templates.retest),
        ("loop_guard", &spec.templates.loop_guard),
        ("build", &spec.templates.build),
        ("test", &spec.templates.test),
        ("lint", &spec.templates.lint),
        ("implement", &spec.templates.implement),
        ("review", &spec.templates.review),
        ("git_ops", &spec.templates.git_ops),
    ];

    let template_capabilities: Vec<String> = named_templates
        .iter()
        .filter_map(|(name, opt)| opt.as_ref().map(|_| name.to_string()))
        .collect();

    let mut capabilities = spec.capabilities.clone().unwrap_or_default();
    for cap in template_capabilities {
        if !capabilities.contains(&cap) {
            capabilities.push(cap);
        }
    }

    let mut templates = std::collections::HashMap::new();
    for (name, opt) in &named_templates {
        if let Some(t) = opt {
            templates.insert(name.to_string(), t.clone());
        }
    }
    // Include extra/custom templates (qa_doc_gen, qa_testing, ticket_fix, etc.)
    for (name, t) in &spec.templates.extra {
        if !templates.contains_key(name) {
            templates.insert(name.clone(), t.clone());
            if !capabilities.contains(name) {
                capabilities.push(name.clone());
            }
        }
    }

    AgentConfig {
        metadata: AgentMetadata {
            name: String::new(),
            description: spec.metadata.as_ref().and_then(|m| m.description.clone()),
            version: None,
            cost: spec.metadata.as_ref().and_then(|m| m.cost),
        },
        capabilities,
        templates,
        selection: spec
            .selection
            .as_ref()
            .map(|selection| AgentSelectionConfig {
                strategy: selection.strategy,
                weights: selection.weights.clone(),
            })
            .unwrap_or_default(),
    }
}

pub(super) fn agent_config_to_spec(config: &AgentConfig) -> AgentSpec {
    AgentSpec {
        templates: AgentTemplatesSpec {
            init_once: config.templates.get("init_once").cloned(),
            plan: config.templates.get("plan").cloned(),
            qa: config.templates.get("qa").cloned(),
            ticket_scan: config.templates.get("ticket_scan").cloned(),
            fix: config.templates.get("fix").cloned(),
            retest: config.templates.get("retest").cloned(),
            loop_guard: config.templates.get("loop_guard").cloned(),
            build: config.templates.get("build").cloned(),
            test: config.templates.get("test").cloned(),
            lint: config.templates.get("lint").cloned(),
            implement: config.templates.get("implement").cloned(),
            review: config.templates.get("review").cloned(),
            git_ops: config.templates.get("git_ops").cloned(),
            extra: {
                let named_keys: std::collections::HashSet<&str> = [
                    "init_once",
                    "plan",
                    "qa",
                    "ticket_scan",
                    "fix",
                    "retest",
                    "loop_guard",
                    "build",
                    "test",
                    "lint",
                    "implement",
                    "review",
                    "git_ops",
                ]
                .into_iter()
                .collect();
                config
                    .templates
                    .iter()
                    .filter(|(k, _)| !named_keys.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            },
        },
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ResourceMetadata, ResourceSpec};
    use crate::config_load::read_active_config;
    use crate::resource::{dispatch_resource, API_VERSION};
    use crate::test_utils::TestState;

    use super::super::test_fixtures::{agent_manifest, make_config};

    #[test]
    fn agent_resource_apply() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let mut config = {
            let active = read_active_config(&state).expect("state should be readable");
            active.config.clone()
        };

        let resource = dispatch_resource(agent_manifest("agent-roundtrip", "cargo test"))
            .expect("agent dispatch should succeed");
        assert_eq!(resource.apply(&mut config), ApplyResult::Created);

        let loaded = AgentResource::get_from(&config, "agent-roundtrip")
            .expect("agent should be present in config");
        assert_eq!(loaded.spec.templates.qa.as_deref(), Some("cargo test"));
        assert_eq!(loaded.kind(), ResourceKind::Agent);
    }

    #[test]
    fn agent_validate_rejects_empty_string_template() {
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-empty-tmpl"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: Some("  ".to_string()), // empty string
                    qa: Some("valid".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let err = agent.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("cannot be empty strings"));
    }

    #[test]
    fn agent_validate_rejects_empty_extra_template() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("custom".to_string(), "  ".to_string());
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-empty-extra"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some("valid".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra,
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let err = agent.validate().expect_err("operation should fail");
        assert!(err.to_string().contains("cannot be an empty string"));
    }

    #[test]
    fn agent_validate_accepts_extra_only_templates() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("qa_doc_gen".to_string(), "do qa doc gen".to_string());
        let agent = AgentResource {
            metadata: super::super::metadata_with_name("ag-extra-only"),
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: None,
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra,
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        assert!(agent.validate().is_ok());
    }

    #[test]
    fn agent_validation_rejects_empty_templates() {
        let agent = AgentResource {
            metadata: ResourceMetadata {
                name: "test-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: None,
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let result = agent.validate();
        assert!(result.is_err());
        assert!(result
            .expect_err("operation should fail")
            .to_string()
            .contains("at least one template"));
    }

    #[test]
    fn agent_get_from_without_stored_metadata() {
        let mut config = make_config();
        config.agents.insert(
            "bare-ag".to_string(),
            AgentConfig {
                metadata: AgentMetadata::default(),
                capabilities: vec!["qa".to_string()],
                templates: [("qa".to_string(), "run qa".to_string())].into(),
                selection: AgentSelectionConfig::default(),
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
        let ag =
            dispatch_resource(agent_manifest("meta-ag", "cmd")).expect("dispatch agent resource");
        ag.apply(&mut config);
        assert!(config.resource_meta.agents.contains_key("meta-ag"));

        AgentResource::delete_from(&mut config, "meta-ag");
        assert!(!config.resource_meta.agents.contains_key("meta-ag"));
    }

    #[test]
    fn agent_to_yaml_includes_templates() {
        let agent = AgentResource {
            metadata: ResourceMetadata {
                name: "full-agent".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: AgentSpec {
                templates: AgentTemplatesSpec {
                    init_once: Some("init".to_string()),
                    plan: None,
                    qa: Some("test".to_string()),
                    fix: Some("fix".to_string()),
                    retest: Some("retest".to_string()),
                    loop_guard: Some("guard".to_string()),
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            },
        };
        let yaml = agent.to_yaml().expect("should serialize");
        assert!(yaml.contains("full-agent"));
        assert!(yaml.contains("init"));
        assert!(yaml.contains("test"));
        assert!(yaml.contains("fix"));
        assert!(yaml.contains("retest"));
        assert!(yaml.contains("guard"));
    }

    // ── agent_spec_to_config / agent_config_to_spec roundtrip ───────

    #[test]
    fn agent_spec_config_roundtrip_with_extra_templates() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("qa_doc_gen".to_string(), "gen docs".to_string());
        extra.insert("qa_testing".to_string(), "run qa testing".to_string());

        let spec = AgentSpec {
            templates: AgentTemplatesSpec {
                init_once: None,
                plan: Some("plan template".to_string()),
                qa: Some("qa template".to_string()),
                fix: None,
                retest: None,
                loop_guard: None,
                ticket_scan: None,
                build: None,
                test: None,
                lint: None,
                implement: Some("implement template".to_string()),
                review: None,
                git_ops: None,
                extra,
            },
            capabilities: Some(vec!["plan".to_string(), "custom_cap".to_string()]),
            metadata: Some(AgentMetadataSpec {
                cost: Some(2),
                description: Some("A test agent".to_string()),
            }),
            selection: Some(AgentSelectionSpec {
                strategy: Default::default(),
                weights: None,
            }),
        };

        let config = agent_spec_to_config(&spec);
        // Check extra templates are in config
        assert!(config.templates.contains_key("qa_doc_gen"));
        assert!(config.templates.contains_key("qa_testing"));
        assert!(config.templates.contains_key("plan"));
        assert!(config.templates.contains_key("implement"));
        // Check capabilities include both explicit and template-derived
        assert!(config.capabilities.contains(&"plan".to_string()));
        assert!(config.capabilities.contains(&"custom_cap".to_string()));
        assert!(config.capabilities.contains(&"qa".to_string()));
        assert!(config.capabilities.contains(&"qa_doc_gen".to_string()));

        // Roundtrip back to spec
        let roundtripped = agent_config_to_spec(&config);
        assert_eq!(
            roundtripped.templates.plan,
            Some("plan template".to_string())
        );
        assert_eq!(roundtripped.templates.qa, Some("qa template".to_string()));
        assert_eq!(
            roundtripped.templates.implement,
            Some("implement template".to_string())
        );
        assert!(roundtripped.templates.extra.contains_key("qa_doc_gen"));
        assert!(roundtripped.templates.extra.contains_key("qa_testing"));
        assert!(roundtripped.capabilities.is_some());
        // Metadata (cost, description) is now preserved through the roundtrip.
        let rt_meta = roundtripped.metadata.expect("metadata should be preserved");
        assert_eq!(rt_meta.cost, Some(2));
        assert_eq!(rt_meta.description, Some("A test agent".to_string()));
    }

    #[test]
    fn agent_config_to_spec_empty_capabilities_becomes_none() {
        let config = AgentConfig {
            metadata: AgentMetadata::default(),
            capabilities: vec![],
            templates: std::collections::HashMap::new(),
            selection: AgentSelectionConfig::default(),
        };
        let spec = agent_config_to_spec(&config);
        assert!(spec.capabilities.is_none());
    }

    #[test]
    fn agent_config_to_spec_no_metadata_becomes_none() {
        let config = AgentConfig {
            metadata: AgentMetadata {
                name: String::new(),
                description: None,
                version: None,
                cost: None,
            },
            capabilities: vec![],
            templates: std::collections::HashMap::new(),
            selection: AgentSelectionConfig::default(),
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
                templates: AgentTemplatesSpec {
                    init_once: None,
                    plan: None,
                    qa: Some("run".to_string()),
                    fix: None,
                    retest: None,
                    loop_guard: None,
                    ticket_scan: None,
                    build: None,
                    test: None,
                    lint: None,
                    implement: None,
                    review: None,
                    git_ops: None,
                    extra: std::collections::HashMap::new(),
                },
                capabilities: None,
                metadata: None,
                selection: None,
            })),
        };
        let rr = dispatch_resource(resource).expect("dispatch agent resource");
        rr.apply(&mut config);

        let stored = config
            .resource_meta
            .agents
            .get("store-meta-ag")
            .expect("stored agent metadata should exist");
        assert_eq!(
            stored
                .labels
                .as_ref()
                .expect("labels should exist")
                .get("tier")
                .expect("tier label should exist"),
            "primary"
        );
    }
}
