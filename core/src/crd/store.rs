use crate::crd::projection::CrdProjectable;
use crate::crd::types::CustomResource;
use crate::resource::ApplyResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Project namespace for singleton/cluster-scoped resources (RuntimePolicy, Project, CRDs).
pub const SYSTEM_PROJECT: &str = "_system";

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
    fn storage_key(kind: &str, metadata: &crate::cli_types::ResourceMetadata) -> String {
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
    pub fn get_namespaced(&self, kind: &str, project: &str, name: &str) -> Option<&CustomResource> {
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
    pub fn put(&mut self, cr: CustomResource) -> ApplyResult {
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
    pub fn remove_by_kind_name_any_project(
        &mut self,
        kind: &str,
        name: &str,
    ) -> Option<CustomResource> {
        let suffix = format!("/{}", name);
        let prefix = format!("{}/", kind);
        let key = self
            .resources
            .keys()
            .find(|k| k.starts_with(&prefix) && k.ends_with(&suffix) && k.matches('/').count() == 2)
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

    /// Project all CRs of a given kind into a typed HashMap.
    pub fn project_map<T: CrdProjectable>(&self) -> HashMap<String, T> {
        let kind = T::crd_kind();
        let mut result = HashMap::new();
        for cr in self.list_by_kind(kind) {
            if let Ok(typed) = T::from_cr_spec(&cr.spec) {
                result.insert(cr.metadata.name.clone(), typed);
            }
        }
        result
    }

    /// Project a singleton CR of a given kind.
    pub fn project_singleton<T: CrdProjectable>(&self) -> Option<T> {
        let kind = T::crd_kind();
        let items = self.list_by_kind(kind);
        items
            .into_iter()
            .next()
            .and_then(|cr| T::from_cr_spec(&cr.spec).ok())
    }

    /// Access the underlying resource map (for iteration/serialization).
    pub fn resources(&self) -> &HashMap<String, CustomResource> {
        &self.resources
    }

    /// Mutable access to the underlying resource map.
    pub fn resources_mut(&mut self) -> &mut HashMap<String, CustomResource> {
        &mut self.resources
    }

    /// Re-key any legacy 2-segment global keys (`kind/name`) to 3-segment
    /// format (`kind/_system/name`). Called during config normalization for
    /// backward compatibility with stores serialized before the project-scoped
    /// key migration.
    pub fn rekey_legacy_global_resources(&mut self) {
        let legacy_keys: Vec<(String, CustomResource)> = self
            .resources
            .iter()
            .filter(|(k, _)| k.matches('/').count() == 1)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (old_key, mut cr) in legacy_keys {
            self.resources.remove(&old_key);
            if cr.metadata.project.is_none() {
                cr.metadata.project = Some(SYSTEM_PROJECT.to_string());
            }
            let new_key = Self::storage_key(&cr.kind, &cr.metadata);
            self.resources.insert(new_key, cr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::config::{AgentConfig, StepTemplateConfig};
    use crate::crd::projection::CrdProjectable;

    fn make_cr(kind: &str, name: &str, spec: serde_json::Value) -> CustomResource {
        CustomResource {
            kind: kind.to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: name.to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec,
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn put_and_get() {
        let mut store = ResourceStore::default();
        let cr = make_cr("Foo", "bar", serde_json::json!({"x": 1}));
        assert_eq!(store.put(cr.clone()), ApplyResult::Created);
        assert!(store.get("Foo", "bar").is_some());
        assert!(store.get("Foo", "missing").is_none());
    }

    #[test]
    fn put_unchanged() {
        let mut store = ResourceStore::default();
        let cr = make_cr("Foo", "bar", serde_json::json!({"x": 1}));
        store.put(cr.clone());
        assert_eq!(store.put(cr), ApplyResult::Unchanged);
    }

    #[test]
    fn put_configured() {
        let mut store = ResourceStore::default();
        let cr1 = make_cr("Foo", "bar", serde_json::json!({"x": 1}));
        store.put(cr1);
        let cr2 = make_cr("Foo", "bar", serde_json::json!({"x": 2}));
        assert_eq!(store.put(cr2), ApplyResult::Configured);
    }

    #[test]
    fn remove_existing() {
        let mut store = ResourceStore::default();
        let cr = make_cr("Foo", "bar", serde_json::json!({}));
        store.put(cr);
        assert!(store.remove("Foo", "bar").is_some());
        assert!(store.get("Foo", "bar").is_none());
    }

    #[test]
    fn remove_missing() {
        let mut store = ResourceStore::default();
        assert!(store.remove("Foo", "bar").is_none());
    }

    #[test]
    fn list_by_kind() {
        let mut store = ResourceStore::default();
        store.put(make_cr("Foo", "a", serde_json::json!({})));
        store.put(make_cr("Foo", "b", serde_json::json!({})));
        store.put(make_cr("Bar", "c", serde_json::json!({})));
        assert_eq!(store.list_by_kind("Foo").len(), 2);
        assert_eq!(store.list_by_kind("Bar").len(), 1);
        assert_eq!(store.list_by_kind("Baz").len(), 0);
    }

    #[test]
    fn generation_increments() {
        let mut store = ResourceStore::default();
        assert_eq!(store.generation(), 0);
        store.put(make_cr("Foo", "a", serde_json::json!({})));
        assert_eq!(store.generation(), 1);
        store.put(make_cr("Foo", "b", serde_json::json!({})));
        assert_eq!(store.generation(), 2);
        store.remove("Foo", "a");
        assert_eq!(store.generation(), 3);
    }

    #[test]
    fn is_empty_and_len() {
        let mut store = ResourceStore::default();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        store.put(make_cr("Foo", "a", serde_json::json!({})));
        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn project_map_for_agents() {
        let mut store = ResourceStore::default();
        let agent = AgentConfig {
            command: "echo {prompt}".to_string(),
            capabilities: vec!["plan".to_string()],
            ..Default::default()
        };
        let spec_val = agent.to_cr_spec();
        store.put(make_cr("Agent", "test-agent", spec_val));

        let map: HashMap<String, AgentConfig> = store.project_map();
        assert_eq!(map.len(), 1);
        let loaded = map.get("test-agent").expect("should exist");
        assert_eq!(loaded.command, "echo {prompt}");
    }

    #[test]
    fn project_singleton() {
        let mut store = ResourceStore::default();
        let tmpl = StepTemplateConfig {
            prompt: "do qa".to_string(),
            description: None,
        };
        let spec_val = tmpl.to_cr_spec();
        store.put(make_cr("StepTemplate", "qa", spec_val));

        let loaded: Option<StepTemplateConfig> = store.project_singleton();
        let loaded = loaded.expect("should project singleton");
        assert_eq!(loaded.prompt, "do qa");
    }

    // ── Edge case tests ─────────────────────────────────────────────────────

    #[test]
    fn cross_kind_key_isolation() {
        // Same name under different kinds must not collide.
        let mut store = ResourceStore::default();
        store.put(make_cr("Agent", "alpha", serde_json::json!({"a": 1})));
        store.put(make_cr("Workflow", "alpha", serde_json::json!({"w": 2})));
        assert_eq!(store.len(), 2);
        assert_eq!(store.get("Agent", "alpha").unwrap().spec["a"], 1);
        assert_eq!(store.get("Workflow", "alpha").unwrap().spec["w"], 2);
        // Removing one kind doesn't affect the other.
        store.remove("Agent", "alpha");
        assert!(store.get("Agent", "alpha").is_none());
        assert!(store.get("Workflow", "alpha").is_some());
    }

    #[test]
    fn list_by_kind_does_not_match_prefix_substring() {
        // "Foo" list should not include "FooBar" entries.
        let mut store = ResourceStore::default();
        store.put(make_cr("Foo", "x", serde_json::json!({})));
        store.put(make_cr("FooBar", "y", serde_json::json!({})));
        assert_eq!(store.list_by_kind("Foo").len(), 1);
        assert_eq!(store.list_by_kind("FooBar").len(), 1);
    }

    #[test]
    fn generation_does_not_increment_on_failed_remove() {
        let mut store = ResourceStore::default();
        store.put(make_cr("X", "a", serde_json::json!({})));
        let gen_before = store.generation();
        store.remove("X", "nonexistent");
        assert_eq!(store.generation(), gen_before);
    }

    #[test]
    fn generation_increments_on_unchanged_put() {
        // Even an unchanged put increments generation (it's a write attempt).
        let mut store = ResourceStore::default();
        let cr = make_cr("X", "a", serde_json::json!({}));
        store.put(cr.clone());
        let gen_after_create = store.generation();
        store.put(cr);
        assert_eq!(store.generation(), gen_after_create + 1);
    }

    #[test]
    fn get_namespaced_uses_three_segment_key() {
        let mut store = ResourceStore::default();
        // Manually insert a namespaced key.
        let cr = CustomResource {
            kind: "Agent".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: "my-agent".to_string(),
                project: Some("proj1".to_string()),
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({}),
            generation: 1,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        // Use the three-segment key directly.
        store
            .resources
            .insert("Agent/proj1/my-agent".to_string(), cr);
        assert!(store.get_namespaced("Agent", "proj1", "my-agent").is_some());
        assert!(store.get_namespaced("Agent", "proj2", "my-agent").is_none());
        // Regular get won't find it (different key format).
        assert!(store.get("Agent", "my-agent").is_none());
    }

    #[test]
    fn project_singleton_runtime_policy() {
        use crate::config::{ResumeConfig, RunnerConfig};
        use crate::crd::projection::RuntimePolicyProjection;

        let mut store = ResourceStore::default();
        let rp = RuntimePolicyProjection {
            runner: RunnerConfig::default(),
            resume: ResumeConfig { auto: true },
            observability: crate::config::ObservabilityConfig::default(),
        };
        store.put(make_cr("RuntimePolicy", "default", rp.to_cr_spec()));
        let projected: Option<RuntimePolicyProjection> = store.project_singleton();
        let p = projected.expect("should project RuntimePolicy singleton");
        assert!(p.resume.auto);
        assert_eq!(p.runner.shell, "/bin/bash");
    }

    #[test]
    fn project_map_skips_corrupted_specs() {
        // A CR with an unparseable spec should be silently skipped, not panic.
        let mut store = ResourceStore::default();
        // Valid agent
        let good = AgentConfig {
            command: "echo ok".to_string(),
            ..Default::default()
        };
        store.put(make_cr("Agent", "good", good.to_cr_spec()));
        // Corrupted — missing required `command` field
        store.put(make_cr(
            "Agent",
            "bad",
            serde_json::json!({"not_command": 42}),
        ));
        let map: HashMap<String, AgentConfig> = store.project_map();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("good"));
        assert!(!map.contains_key("bad"));
    }

    #[test]
    fn project_singleton_returns_none_for_empty_store() {
        let store = ResourceStore::default();
        let runtime: Option<crate::crd::projection::RuntimePolicyProjection> =
            store.project_singleton();
        assert!(runtime.is_none());
    }

    #[test]
    fn put_detects_metadata_change_as_configured() {
        let mut store = ResourceStore::default();
        let cr1 = make_cr("Agent", "a", serde_json::json!({"command": "echo x"}));
        store.put(cr1);
        // Same spec, different metadata (add label).
        let mut cr2 = make_cr("Agent", "a", serde_json::json!({"command": "echo x"}));
        cr2.metadata.labels = Some([("env".to_string(), "prod".to_string())].into());
        assert_eq!(store.put(cr2), ApplyResult::Configured);
    }
}
