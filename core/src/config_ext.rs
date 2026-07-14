//! Extension trait adding CRD-projected accessors to `OrchestratorConfig`.
//!
//! These methods depend on `CrdProjectable` which stays in core because its
//! implementations reference resource converters.

use crate::config::OrchestratorConfig;
use crate::crd::projection::RuntimePolicyProjection;
use crate::crd::store::ResourceStoreExt;

/// Extension methods for `OrchestratorConfig` that require CRD projection.
pub trait OrchestratorConfigExt {
    /// Returns the global `_system` `RuntimePolicyProjection`, or defaults.
    fn global_runtime_policy(&self) -> RuntimePolicyProjection;

    /// Compatibility alias for [`Self::global_runtime_policy`].
    ///
    /// Callers should prefer the explicitly named global or project-scoped
    /// accessor so policy authority is visible at the call site.
    fn runtime_policy(&self) -> RuntimePolicyProjection;

    /// Returns the `RuntimePolicyProjection` scoped to a specific project.
    ///
    /// Falls back to the `_system` project, then to defaults. This prevents
    /// RuntimePolicy resources from other projects from contaminating the
    /// runner configuration.
    fn runtime_policy_for_project(&self, project: &str) -> RuntimePolicyProjection;
}

impl OrchestratorConfigExt for OrchestratorConfig {
    fn global_runtime_policy(&self) -> RuntimePolicyProjection {
        self.resource_store
            .project_singleton_for_project::<RuntimePolicyProjection>(
                orchestrator_config::resource_store::SYSTEM_PROJECT,
            )
            .unwrap_or_default()
    }

    fn runtime_policy(&self) -> RuntimePolicyProjection {
        self.global_runtime_policy()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::crd::projection::CrdProjectable;
    use crate::crd::types::CustomResource;
    use orchestrator_config::resource_store::SYSTEM_PROJECT;

    fn policy(project: &str, control: bool, read: bool) -> CustomResource {
        let projection = RuntimePolicyProjection {
            session_control_enabled: control,
            session_read_enabled: read,
            ..RuntimePolicyProjection::default()
        };
        CustomResource {
            kind: RuntimePolicyProjection::crd_kind().to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: "default".to_string(),
                project: Some(project.to_string()),
                labels: None,
                annotations: None,
            },
            spec: projection.to_cr_spec(),
            generation: 1,
            created_at: "2026-07-15T00:00:00Z".to_string(),
            updated_at: "2026-07-15T00:00:00Z".to_string(),
        }
    }

    fn config_with_policies(system_first: bool) -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        let system = policy(SYSTEM_PROJECT, false, false);
        let project = policy("alpha", true, true);
        if system_first {
            config.resource_store.put(system);
            config.resource_store.put(project);
        } else {
            config.resource_store.put(project);
            config.resource_store.put(system);
        }
        config
    }

    #[test]
    fn global_runtime_policy_is_system_authoritative_for_any_insert_order() {
        for system_first in [true, false] {
            let config = config_with_policies(system_first);
            let global = config.global_runtime_policy();
            assert!(!global.session_control_enabled);
            assert!(!global.session_read_enabled);
            let compatibility = config.runtime_policy();
            assert_eq!(
                compatibility.session_control_enabled,
                global.session_control_enabled
            );
            assert_eq!(
                compatibility.session_read_enabled,
                global.session_read_enabled
            );
        }
    }

    #[test]
    fn project_runtime_policy_overrides_system_and_falls_back_to_it() {
        let config = config_with_policies(false);
        let project = config.runtime_policy_for_project("alpha");
        assert!(project.session_control_enabled);
        assert!(project.session_read_enabled);

        let fallback = config.runtime_policy_for_project("missing");
        assert!(!fallback.session_control_enabled);
        assert!(!fallback.session_read_enabled);
    }

    #[test]
    fn missing_global_runtime_policy_uses_fail_closed_control_default() {
        let mut config = OrchestratorConfig::default();
        config.resource_store.put(policy("alpha", true, false));

        let global = config.global_runtime_policy();
        assert!(!global.session_control_enabled);
        assert!(global.session_read_enabled);
    }
}
