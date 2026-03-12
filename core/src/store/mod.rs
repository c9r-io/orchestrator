//! Persistent Store — cross-task workflow memory with pluggable backends.
//!
//! Three-layer architecture (analogous to K8s StorageClass pattern):
//! - `StoreBackendProvider` CRD: defines HOW a backend works
//! - `WorkflowStore` CRD: defines WHAT store to use
//! - Store entries: actual data managed by the provider

mod command;
mod file;
mod local;
mod validate;

pub use command::CommandAdapter;
pub use file::FileStoreBackend;
pub use local::LocalStoreBackend;
pub use validate::validate_schema;

use crate::async_database::AsyncDatabase;
use crate::config::{StoreBackendProviderConfig, WorkflowStoreConfig};
use crate::crd::projection::CrdProjectable;
use crate::crd::types::CustomResource;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Operations that can be performed on a store.
#[derive(Debug, Clone)]
pub enum StoreOp {
    /// Load a single value by key.
    Get {
        /// Logical workflow-store name.
        store_name: String,
        /// Project scope used to namespace entries.
        project_id: String,
        /// Entry key to load.
        key: String,
    },
    /// Upsert a single value by key.
    Put {
        /// Logical workflow-store name.
        store_name: String,
        /// Project scope used to namespace entries.
        project_id: String,
        /// Entry key to write.
        key: String,
        /// JSON payload to persist.
        value: String,
        /// Task responsible for the write.
        task_id: String,
    },
    /// Remove a single value by key.
    Delete {
        /// Logical workflow-store name.
        store_name: String,
        /// Project scope used to namespace entries.
        project_id: String,
        /// Entry key to delete.
        key: String,
    },
    /// List entries in a store.
    List {
        /// Logical workflow-store name.
        store_name: String,
        /// Project scope used to namespace entries.
        project_id: String,
        /// Maximum number of rows to return.
        limit: u64,
        /// Zero-based row offset.
        offset: u64,
    },
    /// Apply retention pruning to a store.
    Prune {
        /// Logical workflow-store name.
        store_name: String,
        /// Project scope used to namespace entries.
        project_id: String,
        /// Optional maximum number of retained entries.
        max_entries: Option<u64>,
        /// Optional retention window in days.
        ttl_days: Option<u64>,
    },
}

/// Result of a store operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StoreOpResult {
    /// Value retrieved (None if key not found).
    Value(Option<serde_json::Value>),
    /// List of entries.
    Entries(Vec<StoreEntry>),
    /// Operation succeeded with no return value.
    Ok,
}

/// A single entry in a workflow store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreEntry {
    /// Entry key.
    pub key: String,
    /// JSON payload stored under `key`.
    pub value: serde_json::Value,
    /// RFC 3339 timestamp of the latest update.
    pub updated_at: String,
}

/// Manages store operations, dispatching to the appropriate backend.
pub struct StoreManager {
    local_backend: LocalStoreBackend,
    file_backend: FileStoreBackend,
    command_adapter: CommandAdapter,
}

impl StoreManager {
    /// Creates a store manager with built-in local and file backends.
    pub fn new(async_db: Arc<AsyncDatabase>, app_root: std::path::PathBuf) -> Self {
        Self {
            local_backend: LocalStoreBackend::new(async_db.clone()),
            file_backend: FileStoreBackend::new(app_root),
            command_adapter: CommandAdapter,
        }
    }

    /// Execute a store operation, dispatching to the correct backend.
    ///
    /// `custom_resources` is the CRD instance map from `OrchestratorConfig.custom_resources`.
    pub async fn execute(
        &self,
        custom_resources: &HashMap<String, CustomResource>,
        op: StoreOp,
    ) -> Result<StoreOpResult> {
        let store_name = match &op {
            StoreOp::Get { store_name, .. }
            | StoreOp::Put { store_name, .. }
            | StoreOp::Delete { store_name, .. }
            | StoreOp::List { store_name, .. }
            | StoreOp::Prune { store_name, .. } => store_name.clone(),
        };

        // Resolve WorkflowStore config (auto-provision with defaults if not declared)
        let store_config = self.resolve_store_config(custom_resources, &store_name);

        // Validate schema on put
        if let StoreOp::Put { ref value, .. } = op {
            if let Some(ref schema) = store_config.schema {
                let parsed: serde_json::Value = serde_json::from_str(value)
                    .map_err(|e| anyhow!("invalid JSON value for store put: {}", e))?;
                validate_schema(&parsed, schema)?;
            }
        }

        let provider_name = &store_config.provider;
        self.dispatch(custom_resources, provider_name, op).await
    }

    fn resolve_store_config(
        &self,
        custom_resources: &HashMap<String, CustomResource>,
        store_name: &str,
    ) -> WorkflowStoreConfig {
        let key = format!("WorkflowStore/{}", store_name);
        custom_resources
            .get(&key)
            .and_then(|cr| WorkflowStoreConfig::from_cr_spec(&cr.spec).ok())
            .unwrap_or_default()
    }

    async fn dispatch(
        &self,
        custom_resources: &HashMap<String, CustomResource>,
        provider_name: &str,
        op: StoreOp,
    ) -> Result<StoreOpResult> {
        let provider = self.resolve_provider(custom_resources, provider_name)?;

        if provider.builtin {
            match provider_name {
                "local" => self.local_backend.execute(op).await,
                "file" => self.file_backend.execute(op).await,
                _ => Err(anyhow!("unknown builtin provider: {}", provider_name)),
            }
        } else {
            let commands = provider
                .commands
                .as_ref()
                .ok_or_else(|| anyhow!("provider '{}' has no commands defined", provider_name))?;
            self.command_adapter.execute(commands, op).await
        }
    }

    fn resolve_provider(
        &self,
        custom_resources: &HashMap<String, CustomResource>,
        provider_name: &str,
    ) -> Result<StoreBackendProviderConfig> {
        // Built-in providers don't need a CRD instance
        match provider_name {
            "local" | "file" => {
                return Ok(StoreBackendProviderConfig {
                    builtin: true,
                    commands: None,
                });
            }
            _ => {}
        }

        // Look up user-defined provider from custom_resources
        let key = format!("StoreBackendProvider/{}", provider_name);
        custom_resources
            .get(&key)
            .and_then(|cr| StoreBackendProviderConfig::from_cr_spec(&cr.spec).ok())
            .ok_or_else(|| anyhow!("store backend provider '{}' not found", provider_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_op_debug() {
        let op = StoreOp::Get {
            store_name: "metrics".to_string(),
            project_id: "proj1".to_string(),
            key: "k1".to_string(),
        };
        let debug = format!("{:?}", op);
        assert!(debug.contains("Get"));
        assert!(debug.contains("metrics"));
    }

    #[test]
    fn store_entry_serde_round_trip() {
        let entry = StoreEntry {
            key: "bench_001".to_string(),
            value: serde_json::json!({"test_count": 42}),
            updated_at: "2026-03-07T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: StoreEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.key, "bench_001");
    }

    #[test]
    fn store_op_all_variants_debug() {
        let variants: Vec<StoreOp> = vec![
            StoreOp::Get {
                store_name: "s".into(),
                project_id: "p".into(),
                key: "k".into(),
            },
            StoreOp::Put {
                store_name: "s".into(),
                project_id: "p".into(),
                key: "k".into(),
                value: "v".into(),
                task_id: "t".into(),
            },
            StoreOp::Delete {
                store_name: "s".into(),
                project_id: "p".into(),
                key: "k".into(),
            },
            StoreOp::List {
                store_name: "s".into(),
                project_id: "p".into(),
                limit: 10,
                offset: 0,
            },
            StoreOp::Prune {
                store_name: "s".into(),
                project_id: "p".into(),
                max_entries: Some(100),
                ttl_days: Some(30),
            },
        ];
        for op in &variants {
            let debug = format!("{:?}", op);
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn store_op_result_serde_round_trip_value() {
        let result = StoreOpResult::Value(Some(serde_json::json!("hello")));
        let json = serde_json::to_string(&result).expect("serialize");
        let back: StoreOpResult = serde_json::from_str(&json).expect("deserialize");
        match back {
            StoreOpResult::Value(Some(v)) => assert_eq!(v, serde_json::json!("hello")),
            _ => panic!("expected Value(Some)"),
        }
    }

    #[test]
    fn store_op_result_serde_round_trip_none() {
        let result = StoreOpResult::Value(None);
        let json = serde_json::to_string(&result).expect("serialize");
        let back: StoreOpResult = serde_json::from_str(&json).expect("deserialize");
        match back {
            StoreOpResult::Value(None) => {}
            _ => panic!("expected Value(None)"),
        }
    }

    #[test]
    fn store_op_result_serde_round_trip_entries() {
        let result = StoreOpResult::Entries(vec![StoreEntry {
            key: "k1".to_string(),
            value: serde_json::json!(42),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }]);
        let json = serde_json::to_string(&result).expect("serialize");
        let back: StoreOpResult = serde_json::from_str(&json).expect("deserialize");
        match back {
            StoreOpResult::Entries(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].key, "k1");
            }
            _ => panic!("expected Entries"),
        }
    }

    #[test]
    fn store_op_result_serde_round_trip_ok() {
        let result = StoreOpResult::Ok;
        let json = serde_json::to_string(&result).expect("serialize");
        let back: StoreOpResult = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, StoreOpResult::Ok));
    }

    // ── resolve_store_config tests ──

    use crate::test_utils::TestState;

    fn make_store_manager() -> StoreManager {
        let mut fixture = TestState::new();
        let state = fixture.build();
        StoreManager::new(
            state.async_database.clone(),
            std::path::PathBuf::from("/tmp"),
        )
    }

    #[test]
    fn resolve_store_config_returns_default_when_not_found() {
        let mgr = make_store_manager();
        let cr = HashMap::new();
        let config = mgr.resolve_store_config(&cr, "nonexistent");
        assert_eq!(config.provider, "local");
    }

    // ── resolve_provider tests ──

    #[test]
    fn resolve_provider_builtin_local() {
        let mgr = make_store_manager();
        let cr = HashMap::new();
        let provider = mgr.resolve_provider(&cr, "local").unwrap();
        assert!(provider.builtin);
        assert!(provider.commands.is_none());
    }

    #[test]
    fn resolve_provider_builtin_file() {
        let mgr = make_store_manager();
        let cr = HashMap::new();
        let provider = mgr.resolve_provider(&cr, "file").unwrap();
        assert!(provider.builtin);
    }

    #[test]
    fn resolve_provider_unknown_custom_not_found() {
        let mgr = make_store_manager();
        let cr = HashMap::new();
        let result = mgr.resolve_provider(&cr, "my_custom_provider");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
