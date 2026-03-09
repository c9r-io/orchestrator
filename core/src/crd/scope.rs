use serde::{Deserialize, Serialize};

/// Defines the scope of a CRD — how instances are organized.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrdScope {
    /// Project-scoped (Agent, Workflow, Workspace)
    Namespaced,
    /// Global multi-instance (Project, StepTemplate, EnvStore, SecretStore)
    #[default]
    Cluster,
    /// Singleton resources such as RuntimePolicy.
    Singleton,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_cluster() {
        assert_eq!(CrdScope::default(), CrdScope::Cluster);
    }

    #[test]
    fn serde_round_trip() {
        for scope in [CrdScope::Namespaced, CrdScope::Cluster, CrdScope::Singleton] {
            let json = serde_json::to_string(&scope).expect("serialize");
            let back: CrdScope = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(scope, back);
        }
    }

    #[test]
    fn deserializes_from_snake_case() {
        let s: CrdScope = serde_json::from_str("\"namespaced\"").unwrap();
        assert_eq!(s, CrdScope::Namespaced);
        let s: CrdScope = serde_json::from_str("\"cluster\"").unwrap();
        assert_eq!(s, CrdScope::Cluster);
        let s: CrdScope = serde_json::from_str("\"singleton\"").unwrap();
        assert_eq!(s, CrdScope::Singleton);
    }
}
