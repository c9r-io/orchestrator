pub use orchestrator_config::resource_store::*;

use crate::crd::projection::CrdProjectable;
use std::collections::HashMap;

/// Extension trait adding CRD projection methods to ResourceStore.
/// These methods require the CrdProjectable trait which stays in core
/// because its implementations depend on resource converters.
pub trait ResourceStoreExt {
    /// Project all CRs of a given kind into a typed HashMap.
    fn project_map<T: CrdProjectable>(&self) -> HashMap<String, T>;
    /// Project a singleton CR of a given kind.
    fn project_singleton<T: CrdProjectable>(&self) -> Option<T>;
    /// Project a singleton CR of a given kind within a specific project scope.
    fn project_singleton_for_project<T: CrdProjectable>(&self, project: &str) -> Option<T>;
}

impl ResourceStoreExt for ResourceStore {
    fn project_map<T: CrdProjectable>(&self) -> HashMap<String, T> {
        let kind = T::crd_kind();
        let mut result = HashMap::new();
        for cr in self.list_by_kind(kind) {
            if let Ok(typed) = T::from_cr_spec(&cr.spec) {
                result.insert(cr.metadata.name.clone(), typed);
            }
        }
        result
    }

    fn project_singleton<T: CrdProjectable>(&self) -> Option<T> {
        let kind = T::crd_kind();
        let items = self.list_by_kind(kind);
        items
            .into_iter()
            .next()
            .and_then(|cr| T::from_cr_spec(&cr.spec).ok())
    }

    fn project_singleton_for_project<T: CrdProjectable>(&self, project: &str) -> Option<T> {
        let kind = T::crd_kind();
        let items = self.list_by_kind_for_project(kind, project);
        items
            .into_iter()
            .next()
            .and_then(|cr| T::from_cr_spec(&cr.spec).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::config::{AgentConfig, StepTemplateConfig};
    use crate::crd::projection::CrdProjectable;
    use crate::crd::types::CustomResource;

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
            enabled: true,
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

    #[test]
    fn cross_kind_key_isolation() {
        let mut store = ResourceStore::default();
        // Use Trigger (cluster-scoped) and Project (cluster-scoped) to test
        // cross-kind key isolation without project-scoping complications.
        store.put(make_cr("Trigger", "alpha", serde_json::json!({"a": 1})));
        store.put(make_cr("Project", "alpha", serde_json::json!({"w": 2})));
        assert_eq!(store.len(), 2);
        assert_eq!(store.get("Trigger", "alpha").unwrap().spec["a"], 1);
        assert_eq!(store.get("Project", "alpha").unwrap().spec["w"], 2);
        store.remove("Trigger", "alpha");
        assert!(store.get("Trigger", "alpha").is_none());
        assert!(store.get("Project", "alpha").is_some());
    }

    #[test]
    fn list_by_kind_does_not_match_prefix_substring() {
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
        store
            .resources_mut()
            .insert("Agent/proj1/my-agent".to_string(), cr);
        assert!(store.get_namespaced("Agent", "proj1", "my-agent").is_some());
        assert!(store.get_namespaced("Agent", "proj2", "my-agent").is_none());
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
        let mut store = ResourceStore::default();
        let good = AgentConfig {
            enabled: true,
            command: "echo ok".to_string(),
            ..Default::default()
        };
        store.put(make_cr("Agent", "good", good.to_cr_spec()));
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
    fn put_auto_assigns_default_project_for_project_scoped_kinds() {
        let mut store = ResourceStore::default();
        let cr = make_cr(
            "Agent",
            "my-agent",
            serde_json::json!({"command": "echo test"}),
        );
        assert!(cr.metadata.project.is_none());
        store.put(cr);
        assert!(store
            .get_namespaced("Agent", crate::config::DEFAULT_PROJECT_ID, "my-agent")
            .is_some());
        assert!(store.get("Agent", "my-agent").is_none());
    }

    #[test]
    fn put_keeps_system_project_for_cluster_scoped_kinds() {
        let mut store = ResourceStore::default();
        // Project is cluster-scoped — stays in _system when no project specified.
        let cr = make_cr("Project", "my-project", serde_json::json!({}));
        store.put(cr);
        assert!(store.get("Project", "my-project").is_some());
    }

    #[test]
    fn put_assigns_default_project_for_runtime_policy() {
        let mut store = ResourceStore::default();
        // RuntimePolicy is project-scoped — auto-assigned to DEFAULT_PROJECT_ID.
        let cr = make_cr("RuntimePolicy", "runtime", serde_json::json!({}));
        store.put(cr);
        assert!(store
            .get_namespaced(
                "RuntimePolicy",
                crate::config::DEFAULT_PROJECT_ID,
                "runtime"
            )
            .is_some());
        // Not in _system
        assert!(store.get("RuntimePolicy", "runtime").is_none());
    }

    #[test]
    fn put_detects_metadata_change_as_configured() {
        let mut store = ResourceStore::default();
        let cr1 = make_cr("Agent", "a", serde_json::json!({"command": "echo x"}));
        store.put(cr1);
        let mut cr2 = make_cr("Agent", "a", serde_json::json!({"command": "echo x"}));
        cr2.metadata.labels = Some([("env".to_string(), "prod".to_string())].into());
        assert_eq!(store.put(cr2), ApplyResult::Configured);
    }
}
