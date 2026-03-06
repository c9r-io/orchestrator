# WP01: Persistent Store — Cross-Task Memory via CRD

## Problem

Pipeline variables (`pipeline_vars`) are task-scoped. When a task completes, all accumulated context — plan output, QA results, performance metrics, lessons learned — is lost. The next task starts from scratch with only a static `goal`.

## Goal

Provide a **Persistent Store** that workflow steps can read from and write to declaratively. The store persists across tasks, enabling long-term memory and cross-task data flow.

## Architecture: Two-Layer CRD Design

Following the Kubernetes StorageClass / PersistentVolume pattern:

```
┌─────────────────────────────────────────────────────────┐
│  WorkflowStore CRD (Definition Layer)                    │
│  ● Stored in config blob (low-frequency, declarative)    │
│  ● Defines: backend, schema, retention, hooks            │
│  ● Applied via: orchestrator apply -f store.yaml         │
│  ● Analogous to: K8s StorageClass                        │
├─────────────────────────────────────────────────────────┤
│  Store Entries (Data Layer)                               │
│  ● Stored in dedicated DB table (high-frequency, runtime)│
│  ● Validated against WorkflowStore schema on write       │
│  ● Read/written by workflow steps during execution       │
│  ● Analogous to: K8s PersistentVolume                    │
└─────────────────────────────────────────────────────────┘
```

Why two layers:
- **WorkflowStore** in config blob: schema validation, lifecycle hooks, CLI discoverability — same infrastructure as all other CRDs
- **Entries** in dedicated table: steps write entries every segment; serializing the entire config blob per write is too expensive

## 1. WorkflowStore CRD (10th Builtin Kind)

### Builtin Definition

Added to `builtin_crd_definitions()` alongside the existing 9:

```rust
fn workflow_store_crd() -> CustomResourceDefinition {
    CustomResourceDefinition {
        kind: "WorkflowStore".to_string(),
        plural: "workflowstores".to_string(),
        short_names: vec!["wfs".to_string(), "store".to_string()],
        group: BUILTIN_GROUP.to_string(),
        versions: vec![builtin_version(serde_json::json!({
            "type": "object",
            "required": ["backend"],
            "properties": {
                "backend": {
                    "type": "string",
                    "enum": ["local", "file"]
                },
                "base_path": { "type": "string" },
                "schema": { "type": "object" },
                "retention": { "type": "object" }
            }
        }))],
        hooks: CrdHooks::default(),
        scope: CrdScope::Namespaced,  // project-scoped
        builtin: true,
    }
}
```

### YAML Manifest

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: metrics
  project: my-project           # project-scoped
spec:
  backend: local                 # "local" (SQLite) or "file" (filesystem)

  # file backend only: directory for entries (relative to workspace root)
  # base_path: data/stores/metrics

  # Optional: JSON Schema for entry values (validated on write)
  schema:
    type: object
    properties:
      test_count: { type: integer, minimum: 0 }
      compile_time_ms: { type: integer }
      timestamp: { type: string }

  # Optional: retention policy
  retention:
    max_entries: 200             # auto-prune oldest entries beyond this count
    ttl_days: 90                 # auto-prune entries older than N days (0 = forever)
```

### WorkflowStoreConfig (Rust Type)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStoreConfig {
    pub backend: StoreBackend,
    #[serde(default)]
    pub base_path: Option<String>,
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    #[serde(default)]
    pub retention: StoreRetention,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StoreBackend {
    #[default]
    Local,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreRetention {
    #[serde(default)]
    pub max_entries: Option<usize>,
    #[serde(default)]
    pub ttl_days: Option<u32>,
}
```

Implements `CrdProjectable` for round-trip projection between CRD spec and typed config.

## 2. Store Entry Data Layer

### Backend Trait

```rust
#[async_trait]
pub trait StoreBackendOps: Send + Sync {
    async fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<Option<StoreEntry>>;
    async fn put(&self, store_name: &str, project_id: &str, key: &str, value: &str, task_id: &str) -> Result<()>;
    async fn delete(&self, store_name: &str, project_id: &str, key: &str) -> Result<bool>;
    async fn list(&self, store_name: &str, project_id: &str, limit: usize, offset: usize) -> Result<Vec<StoreEntry>>;
    async fn prune(&self, store_name: &str, project_id: &str, max_entries: Option<usize>, ttl_days: Option<u32>) -> Result<u64>;
}

pub struct StoreEntry {
    pub store_name: String,
    pub key: String,
    pub value_json: String,
    pub task_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

### Backend: `local` (SQLite)

DB table (Migration 7):

```sql
CREATE TABLE IF NOT EXISTS workflow_store_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL DEFAULT '',
    store_name TEXT NOT NULL,
    key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    task_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, store_name, key)
);

CREATE INDEX IF NOT EXISTS idx_wf_store_ns_key
    ON workflow_store_entries(project_id, store_name, key);
CREATE INDEX IF NOT EXISTS idx_wf_store_ns_updated
    ON workflow_store_entries(project_id, store_name, updated_at DESC);
```

Implementation: standard `AsyncDatabase` reader/writer pattern, same as `DbWriteCoordinator`.

### Backend: `file` (Filesystem)

Directory structure:

```
{workspace_root}/{base_path}/{store_name}/
├── {key1}.json
├── {key2}.json
└── _meta.json          # entry metadata (task_id, timestamps)
```

Each entry is a standalone JSON file. This backend is useful for:
- Large values (plan_output, diffs) that are already being spilled to files
- Human-readable / git-trackable store data
- Sharing with external tools that read the filesystem

Implementation: `tokio::fs` async read/write. `_meta.json` tracks timestamps and task_id for each key.

### Store Manager (Router)

```rust
pub struct StoreManager {
    local_backend: LocalStoreBackend,
    stores: HashMap<String, WorkflowStoreConfig>,  // populated from CRD config
}

impl StoreManager {
    /// Resolve the backend for a given store name, then delegate.
    pub async fn get(&self, store_name: &str, project_id: &str, key: &str) -> Result<Option<StoreEntry>> {
        let config = self.resolve_store(store_name)?;
        match config.backend {
            StoreBackend::Local => self.local_backend.get(store_name, project_id, key).await,
            StoreBackend::File => {
                let base = config.base_path.as_deref().unwrap_or("data/stores");
                FileStoreBackend::new(base).get(store_name, project_id, key).await
            }
        }
    }

    /// Write with optional schema validation and retention pruning.
    pub async fn put(&self, store_name: &str, project_id: &str, key: &str, value: &str, task_id: &str) -> Result<()> {
        let config = self.resolve_store(store_name)?;

        // Validate against schema if defined
        if let Some(schema) = &config.schema {
            validate_store_value(value, schema)?;
        }

        // Write
        let backend = self.backend_for(&config);
        backend.put(store_name, project_id, key, value, task_id).await?;

        // Prune if retention policy defined
        if config.retention.max_entries.is_some() || config.retention.ttl_days.is_some() {
            backend.prune(store_name, project_id, config.retention.max_entries, config.retention.ttl_days).await?;
        }

        Ok(())
    }
}
```

## 3. Workflow Step Integration

### store_inputs — Read Before Step Execution

```yaml
- id: plan_with_history
  type: plan
  store_inputs:
    - store: metrics                  # WorkflowStore name
      key: latest_benchmark
      into_var: prev_metrics          # injected as pipeline var
      default: "{}"                   # fallback if key doesn't exist
    - store: journal
      key: recent_improvements
      into_var: history
      default: "[]"
```

Engine resolves `store_inputs` before step execution:
1. Look up `WorkflowStore` CRD by name (with project scope)
2. Call `store_manager.get(store_name, project_id, key)`
3. Inject result into `pipeline_vars` as `into_var`
4. Use `default` if key not found or store doesn't exist

### store_outputs — Write After Step Completion (in captures)

```yaml
- id: benchmark
  type: test
  command: "cargo bench --message-format json 2>&1"
  captures:
    - regex: '"test_count":\s*(\d+)'
      var: test_count
  store_outputs:
    - store: metrics
      key: "benchmark_{{task_id}}"     # templated with pipeline vars
      value_from: test_count            # pipeline var name
    - store: metrics
      key: latest_benchmark
      value: '{"test_count": {{test_count}}}'  # inline template
```

Engine processes `store_outputs` after captures are collected:
1. Resolve templates in `key` and `value`/`value_from`
2. Call `store_manager.put(store_name, project_id, key, value, task_id)`
3. Schema validation + retention pruning happen inside `put()`

### CEL Integration (Prehook)

```yaml
prehook:
  engine: cel
  when: 'int(store("metrics", "latest_test_count")) >= 1334'
  reason: "Only proceed when test count meets baseline"
```

Add `store(store_name, key)` function to CEL context:
- Synchronous read from local backend (CEL evaluation is sync)
- Returns string value or empty string if not found
- For `file` backend: sync `std::fs::read_to_string`

## 4. CLI Support

```bash
# Store definitions (CRD layer)
./orchestrator get workflowstores                        # list all stores
./orchestrator get workflowstore metrics                 # show store definition
./orchestrator apply -f store-definition.yaml            # create/update store
./orchestrator delete workflowstore metrics              # delete store definition

# Store entries (data layer)
./orchestrator store get metrics latest_benchmark        # read entry
./orchestrator store list metrics                        # list entries in store
./orchestrator store list metrics --limit 10             # list with limit
./orchestrator store put metrics my_key '{"value": 42}'  # write entry
./orchestrator store delete metrics old_key              # delete entry
./orchestrator store prune metrics                       # manually trigger retention
```

`get workflowstores` uses standard CRD CLI path. `store get/put/list/delete` are new subcommands that operate on the data layer.

## 5. Auto-Provisioned Default Stores

To lower friction, the engine auto-provisions commonly needed stores when a workflow uses `store_inputs`/`store_outputs` referencing a store that doesn't exist:

```rust
// If store "journal" is referenced but no WorkflowStore CRD exists for it,
// auto-create with defaults:
WorkflowStoreConfig {
    backend: StoreBackend::Local,
    base_path: None,
    schema: None,
    retention: StoreRetention {
        max_entries: Some(500),
        ttl_days: None,
    },
}
```

This means simple workflows can use stores without explicit `WorkflowStore` manifests. Advanced users declare explicit manifests for custom backends, schemas, and retention.

## Files to Change

### New Files

| File | Purpose |
|------|---------|
| `core/src/store/mod.rs` | StoreManager, StoreBackendOps trait, StoreEntry |
| `core/src/store/local.rs` | LocalStoreBackend (SQLite) |
| `core/src/store/file.rs` | FileStoreBackend (filesystem) |
| `core/src/store/validate.rs` | Schema validation for entry values |
| `core/src/config/workflow_store.rs` | WorkflowStoreConfig, StoreBackend, StoreRetention |
| `core/src/cli/store_cmd.rs` | CLI `store get/put/list/delete/prune` |
| `fixtures/manifests/bundles/store-test.yaml` | Test fixture manifests |

### Modified Files

| File | Change |
|------|--------|
| `core/src/migration.rs` | Migration 7: `workflow_store_entries` table |
| `core/src/crd/builtin_defs.rs` | Add `workflow_store_crd()`, update count 9→10 |
| `core/src/crd/projection.rs` | Implement `CrdProjectable` for `WorkflowStoreConfig` |
| `core/src/crd/writeback.rs` | Add WorkflowStore to `project_all_builtins` |
| `core/src/config/mod.rs` | Add `workflow_stores: HashMap<String, WorkflowStoreConfig>` |
| `core/src/config/step.rs` | Parse `store_inputs`, `store_outputs` on steps |
| `core/src/state.rs` | Add `store_manager: StoreManager` to InnerState |
| `core/src/scheduler/item_executor/dispatch.rs` | Resolve `store_inputs` before step, process `store_outputs` after captures |
| `core/src/prehook.rs` | Add `store()` CEL function |
| `core/src/cli_handler/mod.rs` | Register `store` subcommand |

## Execution Plan (Implementation Order)

### Phase 1: Foundation
1. `WorkflowStoreConfig` type + `CrdProjectable` impl
2. `workflow_store_crd()` builtin definition (10th kind)
3. Migration 7: `workflow_store_entries` table
4. `StoreBackendOps` trait + `LocalStoreBackend` (SQLite CRUD)
5. `StoreManager` with local backend routing
6. Wire `StoreManager` into `InnerState`

### Phase 2: File Backend
7. `FileStoreBackend` implementation
8. Backend routing in `StoreManager` based on `WorkflowStoreConfig.backend`

### Phase 3: Workflow Integration
9. Parse `store_inputs` / `store_outputs` in step config
10. Resolve `store_inputs` in step executor (before step runs)
11. Process `store_outputs` in step executor (after captures)
12. Auto-provisioning for undeclared stores

### Phase 4: CEL + CLI
13. `store()` CEL function in prehook evaluator
14. CLI `store get/put/list/delete/prune` subcommands
15. Schema validation on write

## Verification

```bash
# Unit tests
cd core && cargo test --lib -- store::tests
cargo test --lib -- migration::tests
cargo test --lib -- crd::builtin_defs::tests   # now expects 10

# Integration: CRD lifecycle
./orchestrator apply -f fixtures/manifests/bundles/store-test.yaml
./orchestrator get workflowstores
# Should list: metrics, journal

# Integration: CLI data operations
./orchestrator store put metrics bench_001 '{"test_count": 1334}'
./orchestrator store get metrics bench_001
# Should return: {"test_count": 1334}
./orchestrator store list metrics
# Should show 1 entry

# Integration: workflow round-trip
# Task A writes to store, Task B reads from store
TASK_A=$(./orchestrator task create --workflow store_writer --goal "write metrics")
./orchestrator task start $TASK_A
./orchestrator store get metrics "result_${TASK_A}"
# Should contain step output

TASK_B=$(./orchestrator task create --workflow store_reader --goal "read metrics")
./orchestrator task start $TASK_B
# Verify Task B's plan step received Task A's data via store_inputs

# Integration: file backend
./orchestrator apply -f fixtures/manifests/bundles/store-file-test.yaml
./orchestrator store put file_store key1 '{"data": "hello"}'
cat workspace/default/data/stores/file_store/key1.json
# Should contain: {"data": "hello"}

# Integration: retention pruning
for i in $(seq 1 15); do
  ./orchestrator store put metrics "entry_$i" "{\"n\": $i}"
done
./orchestrator store list metrics --limit 100
# If max_entries=10, should show only 10 entries (oldest pruned)

# Full test suite
cargo test --lib
```
