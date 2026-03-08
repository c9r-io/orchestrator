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
) -> ResourceMetadata {
    match config.resource_store.get(kind, name) {
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

/// Apply a builtin resource to the unified ResourceStore, then write back
/// the single affected entry to the legacy config field.
pub(crate) fn apply_to_store(
    config: &mut OrchestratorConfig,
    kind: &str,
    name: &str,
    metadata: &ResourceMetadata,
    spec: serde_json::Value,
) -> ApplyResult {
    use crate::crd::types::CustomResource;

    let now = chrono::Utc::now().to_rfc3339();

    // If the store doesn't have this entry yet but the legacy field does,
    // seed the store from the legacy field so that put() can correctly
    // detect Unchanged vs Configured (instead of always returning Created).
    if config.resource_store.get(kind, name).is_none() {
        crate::crd::writeback::seed_store_from_legacy(config, kind, name, &now);
    }

    // Preserve generation and created_at from existing CR if updating
    let (generation, created_at) = match config.resource_store.get(kind, name) {
        Some(existing) => (existing.generation + 1, existing.created_at.clone()),
        None => (1, now.clone()),
    };

    let cr = CustomResource {
        kind: kind.to_string(),
        api_version: "orchestrator.dev/v2".to_string(),
        metadata: metadata.clone(),
        spec,
        generation,
        created_at,
        updated_at: now,
    };
    let result = config.resource_store.put(cr);
    // Targeted writeback: only update the specific entry, not the whole map
    crate::crd::writeback::write_back_single(config, kind, name);
    result
}

/// Delete a builtin resource from the unified ResourceStore, then remove
/// the single affected entry from the legacy config field.
pub(crate) fn delete_from_store(config: &mut OrchestratorConfig, kind: &str, name: &str) -> bool {
    // If the store doesn't have this entry yet but the legacy field does,
    // seed it first so that remove() returns Some and we actually delete it.
    if config.resource_store.get(kind, name).is_none() {
        let now = chrono::Utc::now().to_rfc3339();
        crate::crd::writeback::seed_store_from_legacy(config, kind, name, &now);
    }

    let removed = config.resource_store.remove(kind, name).is_some();
    if removed {
        crate::crd::writeback::remove_from_legacy(config, kind, name);
    }
    removed
}
