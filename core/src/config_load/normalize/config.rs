use super::workflow::normalize_workflow_config;
use crate::config::OrchestratorConfig;

pub(crate) fn normalize_config(mut config: OrchestratorConfig) -> OrchestratorConfig {
    // Ensure the built-in "default" project always exists (like k8s default
    // namespace). It starts empty — users can populate it via
    // `orchestrator apply --project default`.
    config
        .projects
        .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
        .or_insert_with(|| crate::config::ProjectConfig {
            description: Some("Built-in default project".to_string()),
            workspaces: Default::default(),
            agents: Default::default(),
            workflows: Default::default(),
            step_templates: Default::default(),
            env_stores: Default::default(),
            execution_profiles: Default::default(),
        });

    for project in config.projects.values_mut() {
        for workflow in project.workflows.values_mut() {
            normalize_workflow_config(workflow);
        }
    }

    ensure_builtin_crds(&mut config);

    // Always rebuild the resource store from the normalized project-scoped
    // config snapshot. The store is a derived index that the CRD pipeline can query.
    //
    // Preserve RuntimePolicy and other cluster-scoped CRs across the store rebuild,
    // since they are not derived from the config.projects snapshot.
    // Also preserve resource metadata (labels, annotations) from the old store.
    let old_store = std::mem::take(&mut config.resource_store);
    crate::crd::writeback::sync_config_snapshot_to_store(&mut config);

    if let Some(rp_cr) = old_store
        .list_by_kind("RuntimePolicy")
        .into_iter()
        .next()
        .cloned()
    {
        config.resource_store.put(rp_cr);
    } else {
        let rp = crate::crd::projection::RuntimePolicyProjection::default();
        let now = chrono::Utc::now().to_rfc3339();
        let cr = crate::crd::types::CustomResource {
            kind: "RuntimePolicy".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "runtime".to_string(),
                project: Some(crate::crd::store::SYSTEM_PROJECT.to_string()),
                labels: None,
                annotations: None,
            },
            spec: crate::crd::projection::CrdProjectable::to_cr_spec(&rp),
            generation: 1,
            created_at: now.clone(),
            updated_at: now,
        };
        config.resource_store.put(cr);
    }

    crate::crd::writeback::restore_metadata_from_previous_store(&mut config, &old_store);

    config
}

/// Ensure all builtin CRD definitions exist in the config.
fn ensure_builtin_crds(config: &mut OrchestratorConfig) {
    for crd in crate::crd::builtin_defs::builtin_crd_definitions() {
        config
            .custom_resource_definitions
            .entry(crd.kind.clone())
            .or_insert(crd);
    }
}
