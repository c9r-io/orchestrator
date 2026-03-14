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
}

impl OrchestratorConfigExt for OrchestratorConfig {
    fn runtime_policy(&self) -> RuntimePolicyProjection {
        self.resource_store
            .project_singleton::<RuntimePolicyProjection>()
            .unwrap_or_default()
    }
}
