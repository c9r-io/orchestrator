//! Configuration structures for the orchestrator.

mod agent;
mod defaults;
mod execution;
mod pipeline;
mod prehook;
mod runner;
mod safety;
mod step;
mod workflow;

pub use agent::*;
pub use defaults::*;
pub use execution::*;
pub use pipeline::*;
pub use prehook::*;
pub use runner::*;
pub use safety::*;
pub use step::*;
pub use workflow::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main orchestrator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub runner: RunnerConfig,
    pub resume: ResumeConfig,
    pub defaults: ConfigDefaults,
    #[serde(default)]
    pub projects: HashMap<String, ProjectConfig>,
    #[serde(default)]
    pub workspaces: HashMap<String, WorkspaceConfig>,
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    #[serde(default)]
    pub workflows: HashMap<String, WorkflowConfig>,
    #[serde(default)]
    pub resource_meta: ResourceMetadataStore,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            runner: RunnerConfig::default(),
            resume: ResumeConfig { auto: false },
            defaults: ConfigDefaults {
                project: String::new(),
                workspace: String::new(),
                workflow: String::new(),
            },
            projects: HashMap::new(),
            workspaces: HashMap::new(),
            agents: HashMap::new(),
            workflows: HashMap::new(),
            resource_meta: ResourceMetadataStore::default(),
        }
    }
}

/// Default configuration values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDefaults {
    #[serde(default = "default_project")]
    pub project: String,
    pub workspace: String,
    pub workflow: String,
}

fn default_project() -> String {
    "default".to_string()
}

/// Persisted metadata for declarative resources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceMetadataStore {
    #[serde(default)]
    pub workspaces: HashMap<String, ResourceStoredMetadata>,
    #[serde(default)]
    pub agents: HashMap<String, ResourceStoredMetadata>,
    #[serde(default)]
    pub workflows: HashMap<String, ResourceStoredMetadata>,
}

/// Labels and annotations persisted independently from resource specs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceStoredMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_config_default() {
        let cfg = OrchestratorConfig::default();
        assert!(cfg.projects.is_empty());
        assert!(cfg.workspaces.is_empty());
        assert!(cfg.agents.is_empty());
        assert!(cfg.workflows.is_empty());
        assert!(!cfg.resume.auto);
        assert_eq!(cfg.defaults.project, "");
        assert_eq!(cfg.defaults.workspace, "");
        assert_eq!(cfg.defaults.workflow, "");
    }

    #[test]
    fn test_resource_metadata_store_default() {
        let store = ResourceMetadataStore::default();
        assert!(store.workspaces.is_empty());
        assert!(store.agents.is_empty());
        assert!(store.workflows.is_empty());
    }

    #[test]
    fn test_resource_stored_metadata_default() {
        let meta = ResourceStoredMetadata::default();
        assert!(meta.labels.is_none());
        assert!(meta.annotations.is_none());
    }

    #[test]
    fn test_orchestrator_config_serde_round_trip() {
        let cfg = OrchestratorConfig::default();
        let json = serde_json::to_string(&cfg).expect("config should serialize");
        let cfg2: OrchestratorConfig =
            serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(cfg2.defaults.project, cfg.defaults.project);
        assert!(cfg2.projects.is_empty());
    }

    #[test]
    fn test_default_project() {
        assert_eq!(default_project(), "default");
    }
}
