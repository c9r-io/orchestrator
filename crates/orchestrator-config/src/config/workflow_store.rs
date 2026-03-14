use serde::{Deserialize, Serialize};

/// Configuration for a workflow store instance.
///
/// A WorkflowStore defines a named data store that workflows can use for
/// cross-task persistent memory. It references a StoreBackendProvider by name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStoreConfig {
    /// Provider name (references a StoreBackendProvider). Default: "local".
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Base path for the file provider. Only relevant when provider is "file".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,

    /// Optional JSON Schema validated on write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_json::Value>,

    /// Retention policy for store entries.
    #[serde(default)]
    pub retention: StoreRetention,
}

/// Retention policy for workflow store entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreRetention {
    /// Maximum number of entries to keep.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u64>,

    /// Time-to-live in days. Entries older than this are pruned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_days: Option<u64>,
}

fn default_provider() -> String {
    "local".to_string()
}

impl Default for WorkflowStoreConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            base_path: None,
            schema: None,
            retention: StoreRetention::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_local() {
        let config = WorkflowStoreConfig::default();
        assert_eq!(config.provider, "local");
        assert!(config.base_path.is_none());
        assert!(config.schema.is_none());
        assert!(config.retention.max_entries.is_none());
        assert!(config.retention.ttl_days.is_none());
    }

    #[test]
    fn serde_round_trip() {
        let config = WorkflowStoreConfig {
            provider: "redis".to_string(),
            base_path: None,
            schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "test_count": { "type": "integer", "minimum": 0 }
                }
            })),
            retention: StoreRetention {
                max_entries: Some(200),
                ttl_days: Some(90),
            },
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: WorkflowStoreConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.provider, "redis");
        assert!(back.schema.is_some());
        assert_eq!(back.retention.max_entries, Some(200));
        assert_eq!(back.retention.ttl_days, Some(90));
    }

    #[test]
    fn serde_with_file_provider() {
        let config = WorkflowStoreConfig {
            provider: "file".to_string(),
            base_path: Some("data/stores/metrics".to_string()),
            schema: None,
            retention: StoreRetention::default(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let back: WorkflowStoreConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.provider, "file");
        assert_eq!(back.base_path.as_deref(), Some("data/stores/metrics"));
    }
}
