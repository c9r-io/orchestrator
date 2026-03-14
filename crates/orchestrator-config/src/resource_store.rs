//! Unified resource store and apply-result types.

use crate::cli_types::ResourceMetadata;
use crate::config::DEFAULT_PROJECT_ID;
use crate::crd_types::CustomResource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of applying a manifest resource into an `OrchestratorConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResult {
    /// Resource did not exist and was created.
    Created,
    /// Resource existed and its stored representation changed.
    Configured,
    /// Resource already matched the requested representation.
    Unchanged,
}

/// Project namespace for singleton/cluster-scoped resources (RuntimePolicy, Project, CRDs).
pub const SYSTEM_PROJECT: &str = "_system";

/// Returns true for resource kinds that must belong to a project (not `_system`).
pub fn is_project_scoped(kind: &str) -> bool {
    matches!(
        kind,
        "Agent"
            | "Workflow"
            | "Workspace"
            | "StepTemplate"
            | "ExecutionProfile"
            | "EnvStore"
            | "SecretStore"
    )
}

/// Unified resource store — single source of truth for all resource instances.
///
/// All resources use 3-segment keys: `kind/project/name`.
/// Singleton/cluster-scoped resources use `_system` as their project namespace.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceStore {
    #[serde(default)]
    resources: HashMap<String, CustomResource>,
    #[serde(skip)]
    generation: u64,
}

impl ResourceStore {
    fn storage_key(kind: &str, metadata: &ResourceMetadata) -> String {
        let project = metadata
            .project
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or(SYSTEM_PROJECT);
        format!("{}/{}/{}", kind, project, metadata.name)
    }

    /// Get a resource by kind and name (delegates to `_system` project).
    pub fn get(&self, kind: &str, name: &str) -> Option<&CustomResource> {
        self.get_namespaced(kind, SYSTEM_PROJECT, name)
    }

    /// Get a mutable reference to a resource by its storage key.
    pub fn get_mut_by_key(&mut self, key: &str) -> Option<&mut CustomResource> {
        self.resources.get_mut(key)
    }

    /// Get a namespaced resource by kind, project, and name.
    pub fn get_namespaced(
        &self,
        kind: &str,
        project: &str,
        name: &str,
    ) -> Option<&CustomResource> {
        let key = format!("{}/{}/{}", kind, project, name);
        self.resources.get(&key)
    }

    /// List all resources of a given kind.
    pub fn list_by_kind(&self, kind: &str) -> Vec<&CustomResource> {
        let prefix = format!("{}/", kind);
        self.resources
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| v)
            .collect()
    }

    /// Insert or update a resource. Returns the apply result.
    /// For project-scoped kinds with no project, auto-assigns DEFAULT_PROJECT_ID.
    pub fn put(&mut self, mut cr: CustomResource) -> ApplyResult {
        // Auto-assign DEFAULT_PROJECT_ID for project-scoped kinds with no/empty project
        if is_project_scoped(&cr.kind)
            && cr
                .metadata
                .project
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .is_none()
        {
            cr.metadata.project = Some(DEFAULT_PROJECT_ID.to_string());
        }
        let key = Self::storage_key(&cr.kind, &cr.metadata);
        self.generation += 1;

        match self.resources.get(&key) {
            None => {
                self.resources.insert(key, cr);
                ApplyResult::Created
            }
            Some(existing) => {
                if existing.spec == cr.spec
                    && existing.api_version == cr.api_version
                    && existing.metadata == cr.metadata
                {
                    ApplyResult::Unchanged
                } else {
                    self.resources.insert(key, cr);
                    ApplyResult::Configured
                }
            }
        }
    }

    /// Remove a resource by kind and name (delegates to `_system` project).
    pub fn remove(&mut self, kind: &str, name: &str) -> Option<CustomResource> {
        self.remove_namespaced(kind, SYSTEM_PROJECT, name)
    }

    /// Remove a resource by kind and name from any project namespace.
    /// Scans all entries of the form `kind/*/name`.
    pub fn remove_first_by_kind_name(&mut self, kind: &str, name: &str) -> Option<CustomResource> {
        let suffix = format!("/{}", name);
        let prefix = format!("{}/", kind);
        let key = self
            .resources
            .keys()
            .find(|k| {
                k.starts_with(&prefix) && k.ends_with(&suffix) && k.matches('/').count() == 2
            })
            .cloned();
        if let Some(key) = key {
            let removed = self.resources.remove(&key);
            if removed.is_some() {
                self.generation += 1;
            }
            return removed;
        }
        None
    }

    /// Removes one project-scoped resource by kind, project, and name.
    pub fn remove_namespaced(
        &mut self,
        kind: &str,
        project: &str,
        name: &str,
    ) -> Option<CustomResource> {
        let key = format!("{}/{}/{}", kind, project, name);
        let removed = self.resources.remove(&key);
        if removed.is_some() {
            self.generation += 1;
        }
        removed
    }

    /// Current generation counter (incremented on each mutation).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether the store has no resources.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Number of resources in the store.
    pub fn len(&self) -> usize {
        self.resources.len()
    }

    /// Access the underlying resource map (for iteration/serialization).
    pub fn resources(&self) -> &HashMap<String, CustomResource> {
        &self.resources
    }

    /// Mutable access to the underlying resource map.
    pub fn resources_mut(&mut self) -> &mut HashMap<String, CustomResource> {
        &mut self.resources
    }
}
