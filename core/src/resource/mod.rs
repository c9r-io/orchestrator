use crate::cli_types::ResourceKind;
use crate::config::OrchestratorConfig;
use anyhow::Result;

pub(crate) const API_VERSION: &str = "orchestrator.dev/v2";

// ── Submodules ────────────────────────────────────────────────────────────────

pub(crate) mod agent;
mod env_store;
pub(crate) mod execution_profile;
mod export;
mod parse;
mod project;
pub(crate) mod runtime_policy;
mod secret_store;
mod step_template;
mod trigger;
pub(crate) mod workflow;
pub(crate) mod workspace;

mod apply;
pub(crate) mod helpers;
mod registry;
#[cfg(test)]
pub(crate) mod test_fixtures;
mod tests;

// ── Re-exports (resource types from existing submodules) ──────────────────────

pub use agent::AgentResource;
pub use env_store::EnvStoreResource;
pub use execution_profile::ExecutionProfileResource;
pub use export::{export_crd_documents, export_manifest_documents, export_manifest_resources};
pub use parse::{
    delete_resource_by_kind, kind_as_str, parse_manifests_from_yaml, parse_resources_from_yaml,
};
pub use project::ProjectResource;
pub use runtime_policy::RuntimePolicyResource;
pub use secret_store::SecretStoreResource;
pub use step_template::StepTemplateResource;
pub use trigger::TriggerResource;
pub use workflow::WorkflowResource;
pub use workspace::WorkspaceResource;

// ── Re-exports (from new submodules) ──────────────────────────────────────────

pub use helpers::metadata_from_store;
pub(crate) use helpers::{
    apply_to_store, delete_from_store, manifest_yaml, metadata_with_name, serializes_equal,
    validate_resource_name,
};
pub use registry::*;
// apply_to_map is used by individual resource apply methods via super::
pub use apply::apply_to_project;

// ── Re-exports (cli_types used by submodules via super::) ─────────────────────

pub(crate) use crate::cli_types::ResourceMetadata;

// ── Core types ────────────────────────────────────────────────────────────────

pub use orchestrator_config::resource_store::ApplyResult;

/// Common behavior implemented by builtin manifest resource adapters.
pub trait Resource: Sized {
    /// Returns the builtin resource kind.
    fn kind(&self) -> ResourceKind;
    /// Returns the resource name.
    fn name(&self) -> &str;
    /// Validates the resource contents before apply.
    fn validate(&self) -> Result<()>;
    /// Applies the resource into the provided config.
    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult>;
    /// Serializes the resource back into manifest YAML.
    fn to_yaml(&self) -> Result<String>;

    /// Project-scoped resource lookup. `project_id` of `None` defaults to the
    /// default project (via `OrchestratorConfig::effective_project_id`).
    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self>;

    /// Project-scoped resource deletion. `project_id` of `None` defaults to
    /// the default project.
    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool;

    /// Convenience: lookup in the default project.
    fn get_from(config: &OrchestratorConfig, name: &str) -> Option<Self> {
        Self::get_from_project(config, name, None)
    }

    /// Convenience: delete from the default project.
    fn delete_from(config: &mut OrchestratorConfig, name: &str) -> bool {
        Self::delete_from_project(config, name, None)
    }
}
