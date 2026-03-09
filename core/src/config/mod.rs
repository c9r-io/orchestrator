//! Configuration structures for the orchestrator.

mod agent;
mod defaults;
mod dynamic_items;
mod env_store;
mod execution;
mod invariant;
mod item_select;
mod observability;
mod pipeline;
mod prehook;
mod runner;
mod safety;
mod spawn;
mod step;
mod step_template;
mod store_backend_provider;
mod store_io;
mod workflow;
mod workflow_store;

pub use agent::*;
pub use defaults::*;
pub use dynamic_items::*;
pub use env_store::*;
pub use execution::*;
pub use invariant::*;
pub use item_select::*;
pub use observability::*;
pub use pipeline::*;
pub use prehook::*;
pub use runner::*;
pub use safety::*;
pub use spawn::*;
pub use step::*;
pub use step_template::*;
pub use store_backend_provider::*;
pub use store_io::*;
pub use workflow::*;
pub use workflow_store::*;

use crate::crd::store::ResourceStore;
use crate::crd::types::{CustomResource, CustomResourceDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_PROJECT_ID: &str = "default";

/// Main orchestrator configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    #[serde(default)]
    pub projects: HashMap<String, ProjectConfig>,
    #[serde(default)]
    pub custom_resource_definitions: HashMap<String, CustomResourceDefinition>,
    /// Custom resource instances (CRD-defined resources).
    #[serde(default)]
    pub custom_resources: HashMap<String, CustomResource>,
    /// Unified resource store — stores all resources (builtin + custom CRD instances).
    #[serde(default)]
    pub resource_store: ResourceStore,
}

impl OrchestratorConfig {
    /// Convenience accessor: runner config from RuntimePolicy.
    pub fn runner(&self) -> RunnerConfig {
        self.runtime_policy().runner
    }

    /// Convenience accessor: resume config from RuntimePolicy.
    pub fn resume(&self) -> ResumeConfig {
        self.runtime_policy().resume
    }

    /// Access the RuntimePolicy projection from the resource store.
    /// Returns defaults if the store has no RuntimePolicy CR (cold start).
    pub fn runtime_policy(&self) -> crate::crd::projection::RuntimePolicyProjection {
        self.resource_store
            .project_singleton::<crate::crd::projection::RuntimePolicyProjection>()
            .unwrap_or_default()
    }

    pub fn effective_project_id<'a>(&'a self, project_id: Option<&'a str>) -> &'a str {
        project_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_PROJECT_ID)
    }

    pub fn project(&self, project_id: Option<&str>) -> Option<&ProjectConfig> {
        self.projects.get(self.effective_project_id(project_id))
    }

    pub fn project_mut(&mut self, project_id: Option<&str>) -> Option<&mut ProjectConfig> {
        let project_id = self.effective_project_id(project_id).to_string();
        self.projects.get_mut(&project_id)
    }

    pub fn default_project(&self) -> Option<&ProjectConfig> {
        self.project(Some(DEFAULT_PROJECT_ID))
    }

    pub fn ensure_project(&mut self, project_id: Option<&str>) -> &mut ProjectConfig {
        let project_id = self.effective_project_id(project_id).to_string();
        self.projects.entry(project_id).or_default()
    }
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
        // RuntimePolicy from resource store defaults
        let rp = cfg.runtime_policy();
        assert!(!rp.resume.auto);
        assert_eq!(rp.observability, ObservabilityConfig::default());
    }

    #[test]
    fn test_orchestrator_config_serde_round_trip() {
        let cfg = OrchestratorConfig::default();
        let json = serde_json::to_string(&cfg).expect("config should serialize");
        let cfg2: OrchestratorConfig =
            serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(cfg2.projects.len(), cfg.projects.len());
        assert!(cfg2.projects.is_empty());
    }

    #[test]
    fn test_default_project() {
        assert_eq!(DEFAULT_PROJECT_ID, "default");
    }
}
