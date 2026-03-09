use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceMetadata, ResourceSpec};
use crate::config::OrchestratorConfig;
use anyhow::{anyhow, Result};
use serde::Serialize;

use super::{ApplyResult, API_VERSION};

pub(crate) fn validate_resource_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("metadata.name cannot be empty"));
    }
    Ok(())
}

pub(crate) fn metadata_with_name(name: &str) -> ResourceMetadata {
    ResourceMetadata {
        name: name.to_string(),
        project: None,
        labels: None,
        annotations: None,
    }
}

#[allow(dead_code)]
pub(crate) fn metadata_from_parts(
    name: &str,
    project: Option<String>,
    labels: Option<std::collections::HashMap<String, String>>,
    annotations: Option<std::collections::HashMap<String, String>>,
) -> ResourceMetadata {
    ResourceMetadata {
        name: name.to_string(),
        project,
        labels,
        annotations,
    }
}

/// Read resource metadata from the ResourceStore, falling back to name-only.
pub fn metadata_from_store(
    config: &OrchestratorConfig,
    kind: &str,
    name: &str,
    project_id: Option<&str>,
) -> ResourceMetadata {
    use crate::crd::store::is_project_scoped;
    let pid = config.effective_project_id(project_id);
    let cr = if is_project_scoped(kind) {
        config.resource_store.get_namespaced(kind, pid, name)
    } else {
        config
            .resource_store
            .get_namespaced(kind, pid, name)
            .or_else(|| config.resource_store.get(kind, name))
    };
    match cr {
        Some(cr) => cr.metadata.clone(),
        None => metadata_with_name(name),
    }
}

pub(crate) fn manifest_yaml(
    kind: ResourceKind,
    metadata: &ResourceMetadata,
    spec: ResourceSpec,
) -> Result<String> {
    let manifest = OrchestratorResource {
        api_version: API_VERSION.to_string(),
        kind,
        metadata: metadata.clone(),
        spec,
    };
    Ok(serde_yml::to_string(&manifest)?)
}

pub(crate) fn apply_to_map<T: Clone + Serialize>(
    map: &mut std::collections::HashMap<String, T>,
    name: &str,
    incoming: T,
) -> ApplyResult {
    match map.get(name) {
        None => {
            map.insert(name.to_string(), incoming);
            ApplyResult::Created
        }
        Some(existing) => {
            if serializes_equal(existing, &incoming) {
                ApplyResult::Unchanged
            } else {
                map.insert(name.to_string(), incoming);
                ApplyResult::Configured
            }
        }
    }
}

pub(crate) fn serializes_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

/// Apply a builtin resource to the unified ResourceStore, then reconcile
/// the single affected entry back into the in-memory config snapshot.
pub(crate) fn apply_to_store(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
    metadata: &ResourceMetadata,
    spec: serde_json::Value,
) -> ApplyResult {
    use crate::crd::types::CustomResource;

    let now = chrono::Utc::now().to_rfc3339();

    // If the store doesn't have this entry yet but the config snapshot does,
    // seed the store from the current snapshot so that put() can correctly
    // detect Unchanged vs Configured (instead of always returning Created).
    let is_project_scoped = matches!(
        kind,
        "Agent" | "Workflow" | "Workspace" | "StepTemplate" | "EnvStore" | "SecretStore"
    );
    let default_project = if is_project_scoped {
        crate::config::DEFAULT_PROJECT_ID
    } else {
        crate::crd::store::SYSTEM_PROJECT
    };
    let project_id = metadata
        .project
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default_project);
    let stored_metadata = if metadata
        .project
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .is_none()
    {
        let mut adjusted = metadata.clone();
        adjusted.project = Some(project_id.to_string());
        adjusted
    } else {
        metadata.clone()
    };
    if config
        .resource_store
        .get_namespaced(kind, project_id, name)
        .is_none()
    {
        crate::crd::writeback::seed_store_from_config_snapshot(config, kind, name, &now);
    }

    // Preserve generation and created_at from existing CR if updating
    let (generation, created_at) = match config
        .resource_store
        .get_namespaced(kind, project_id, name)
    {
        Some(existing) => (existing.generation + 1, existing.created_at.clone()),
        None => (1, now.clone()),
    };

    let cr = CustomResource {
        kind: kind.to_string(),
        api_version: "orchestrator.dev/v2".to_string(),
        metadata: stored_metadata,
        spec,
        generation,
        created_at,
        updated_at: now,
    };
    let result = config.resource_store.put(cr);
    // Targeted reconciliation: only update the specific entry, not the whole map
    crate::crd::writeback::reconcile_single_resource(config, kind, Some(project_id), name);
    result
}

/// Delete a builtin resource from the unified ResourceStore, then remove
/// the single affected entry from the in-memory config snapshot.
pub(crate) fn delete_from_store(config: &mut OrchestratorConfig, kind: &str, name: &str) -> bool {
    if !config
        .resource_store
        .list_by_kind(kind)
        .into_iter()
        .any(|cr| cr.metadata.name == name)
    {
        crate::crd::writeback::seed_store_from_config_snapshot(
            config,
            kind,
            name,
            &chrono::Utc::now().to_rfc3339(),
        );
    }
    let removed = config
        .resource_store
        .remove_first_by_kind_name(kind, name)
        .is_some();
    if removed {
        crate::crd::writeback::remove_from_config_snapshot(config, kind, None, name);
    }
    removed
}

pub(crate) fn delete_from_store_project(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
    project_id: Option<&str>,
) -> bool {
    let project_id = config.effective_project_id(project_id).to_string();
    if config
        .resource_store
        .get_namespaced(kind, &project_id, name)
        .is_none()
    {
        crate::crd::writeback::seed_store_from_config_snapshot(
            config,
            kind,
            name,
            &chrono::Utc::now().to_rfc3339(),
        );
    }
    let removed = config
        .resource_store
        .remove_namespaced(kind, &project_id, name)
        .is_some();
    if removed {
        crate::crd::writeback::remove_from_config_snapshot(config, kind, Some(&project_id), name);
    }
    removed
}
