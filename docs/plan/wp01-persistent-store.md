# WP01: Persistent Store — Cross-Task Memory for Workflows

## Problem

Pipeline variables (`pipeline_vars`) are task-scoped. When a task completes, all accumulated context — plan output, QA results, performance metrics, lessons learned — is lost. The next task starts from scratch with only a static `goal`.

This means:
- Workflows can't learn from past runs
- No trend analysis (are things getting better or worse?)
- No knowledge accumulation (what approaches worked?)
- Every self-bootstrap cycle rediscovers the same context

## Goal

Add a **Persistent Store** that workflow steps can read from and write to declaratively. The store persists across tasks, enabling long-term memory and cross-task data flow.

## Design

### 1. Store Model

A key-value store with namespaces, scoped to a project or workspace:

```
store://{namespace}/{key}
```

- **Namespace** isolates stores by concern (e.g., `metrics`, `journal`, `baselines`)
- **Key** identifies individual entries
- **Value** is JSON (arbitrary structure)
- **Metadata**: `created_at`, `updated_at`, `task_id` (which task wrote it)

### 2. DB Schema

Leverage the existing `ResourceStore` / CRD infrastructure where possible. If CRDs are too heavy, a lightweight dedicated table:

```sql
CREATE TABLE workflow_store (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL DEFAULT '',
    namespace TEXT NOT NULL,
    key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    task_id TEXT,            -- which task wrote this entry
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, namespace, key)
);

CREATE INDEX idx_workflow_store_ns_key ON workflow_store(project_id, namespace, key);
CREATE INDEX idx_workflow_store_ns_updated ON workflow_store(project_id, namespace, updated_at DESC);
```

### 3. Workflow YAML Syntax

#### Reading from store (step inputs)

```yaml
- id: plan_with_history
  type: plan
  store_inputs:
    - namespace: journal
      key: recent_improvements
      into_var: improvement_history    # injected as pipeline var
      default: "[]"                    # fallback if key doesn't exist
    - namespace: metrics
      query: "ORDER BY updated_at DESC LIMIT 5"
      into_var: recent_metrics
```

`store_inputs` are resolved before the step executes. Values are injected into `pipeline_vars` so the step's command/agent can reference them via `{improvement_history}`.

#### Writing to store (step outputs / captures)

```yaml
- id: record_improvement
  type: plan
  captures:
    - regex: "(?s)(.*)"
      var: plan_output
    - regex: "(?s)(.*)"
      var: plan_output
      store:                           # ← new: persist to store
        namespace: journal
        key: "improvement_{{task_id}}"
        append_to: recent_improvements  # optional: append to a list-type key
        max_entries: 50                 # cap list growth
```

#### Writing metrics (post_action)

```yaml
- id: benchmark
  type: test
  command: "cargo bench --output-format json"
  post_actions:
    - store_put:
        namespace: metrics
        key: "benchmark_{{task_id}}"
        value_from: benchmark_output    # pipeline var name
    - store_put:
        namespace: baselines
        key: test_count
        value: "{{test_count}}"         # overwrite baseline
```

### 4. Engine Support (Rust)

#### New module: `core/src/store.rs`

```rust
pub struct WorkflowStore {
    async_db: Arc<AsyncDatabase>,
}

impl WorkflowStore {
    pub async fn get(&self, project_id: &str, namespace: &str, key: &str) -> Result<Option<String>>;
    pub async fn put(&self, project_id: &str, namespace: &str, key: &str, value: &str, task_id: &str) -> Result<()>;
    pub async fn list(&self, project_id: &str, namespace: &str, limit: usize) -> Result<Vec<StoreEntry>>;
    pub async fn append_to_list(&self, project_id: &str, namespace: &str, key: &str, value: &str, max_entries: usize) -> Result<()>;
    pub async fn delete(&self, project_id: &str, namespace: &str, key: &str) -> Result<()>;
}
```

#### Integration points

1. **Config parsing**: Extend `TaskExecutionStep` to parse `store_inputs` and `store:` in captures
2. **Step executor**: Before step execution, resolve `store_inputs` → inject into pipeline_vars
3. **Capture processing**: After step execution, process `store:` directives in captures
4. **Post-action processing**: Handle `store_put` post_actions
5. **InnerState**: Add `workflow_store: WorkflowStore` field

### 5. CEL Integration

Expose store values in prehook CEL expressions:

```yaml
prehook:
  engine: cel
  when: "int(store('metrics', 'test_count')) >= int(store('baselines', 'min_test_count'))"
```

This requires adding a `store()` function to the CEL evaluation context.

### 6. CLI Support

```bash
# Read
./orchestrator store get metrics benchmark_latest
./orchestrator store list metrics --limit 10

# Write (manual / scripted)
./orchestrator store put baselines min_test_count '{"value": 1334}'

# Delete
./orchestrator store delete metrics old_key

# Inspect all namespaces
./orchestrator store namespaces
```

## Migration

- Migration 7: `m0007_create_workflow_store`
- Idempotent via `CREATE TABLE IF NOT EXISTS`

## Scope Boundary

### In scope
- CRUD for key-value entries with namespace isolation
- Declarative read (store_inputs) and write (captures + post_actions) in YAML
- CEL `store()` function for prehook conditions
- CLI for inspection and manual management
- Project-scoped isolation

### Out of scope (deferred)
- Store TTL / automatic expiration
- Store replication across DB instances
- Complex query language beyond key lookup and list-by-namespace
- Store-triggered workflows (watch/subscribe pattern)

## Files to Change

| File | Change |
|------|--------|
| `core/src/migration.rs` | Migration 7: workflow_store table |
| `core/src/store.rs` (new) | WorkflowStore struct + CRUD methods |
| `core/src/state.rs` | Add `workflow_store` to InnerState |
| `core/src/config/step.rs` | Parse `store_inputs`, `store:` in captures, `store_put` post_action |
| `core/src/scheduler/item_executor/apply.rs` | Process store writes in captures |
| `core/src/scheduler/item_executor/dispatch.rs` | Resolve store_inputs before step execution |
| `core/src/prehook.rs` | Add `store()` CEL function |
| `core/src/cli/store.rs` (new) | CLI subcommands for store CRUD |
| `core/src/db_write.rs` | Store write coordinator methods |

## Verification

```bash
# Unit tests
cargo test --lib -- store::tests
cargo test --lib -- migration::tests

# Integration: workflow with store_inputs + store captures
./orchestrator apply -f fixtures/manifests/bundles/store-test.yaml
./orchestrator task create --workflow store_roundtrip --goal "test store persistence"
./orchestrator task start <id>
./orchestrator store get test_ns roundtrip_key   # should show written value

# Cross-task verification
./orchestrator task create --workflow store_reader --goal "read previous task output"
./orchestrator task start <id2>
# Verify plan step received store data in its pipeline vars
```
