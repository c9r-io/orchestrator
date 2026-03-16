//! Extension trait adding CRD-projected accessors to `OrchestratorConfig`.
//!
//! These methods depend on `CrdProjectable` which stays in core because its
//! implementations reference resource converters.

use crate::config::OrchestratorConfig;
use crate::crd::projection::RuntimePolicyProjection;
use crate::crd::store::ResourceStoreExt;

/// Extension methods for `OrchestratorConfig` that require CRD projection.
pub trait OrchestratorConfigExt {
    /// Returns the `RuntimePolicyProjection` from the resource store, or defaults.
    fn runtime_policy(&self) -> RuntimePolicyProjection;

    /// Returns the `RuntimePolicyProjection` scoped to a specific project.
    ///
    /// Falls back to the `_system` project, then to defaults. This prevents
    /// RuntimePolicy resources from other projects from contaminating the
    /// runner configuration.
    fn runtime_policy_for_project(&self, project: &str) -> RuntimePolicyProjection;
}

impl OrchestratorConfigExt for OrchestratorConfig {
    fn runtime_policy(&self) -> RuntimePolicyProjection {
        self.resource_store
            .project_singleton::<RuntimePolicyProjection>()
            .unwrap_or_default()
    }

    fn runtime_policy_for_project(&self, project: &str) -> RuntimePolicyProjection {
        // Try project-specific RuntimePolicy first
        if let Some(rp) = self
            .resource_store
            .project_singleton_for_project::<RuntimePolicyProjection>(project)
        {
            return rp;
        }
        // Fall back to _system project
        if let Some(rp) = self
            .resource_store
            .project_singleton_for_project::<RuntimePolicyProjection>(
                orchestrator_config::resource_store::SYSTEM_PROJECT,
            )
        {
            return rp;
        }
        // Final fallback: defaults
        RuntimePolicyProjection::default()
    }
}
