# Orchestrator - Persistent Store (WP01)

**Module**: orchestrator
**Scope**: WorkflowStore and StoreBackendProvider CRDs, store CLI, local/file/command backends
**Scenarios**: 5
**Priority**: High

---

## Background

WP01 adds a **Persistent Store** for cross-task workflow memory. The architecture follows a three-layer pattern (analogous to K8s StorageClass):

- **StoreBackendProvider** CRD (11th builtin kind): defines HOW a backend works — built-in providers (`local`, `file`) or user-defined shell commands.
- **WorkflowStore** CRD (10th builtin kind): defines WHAT store to use — references a provider, optional schema validation, and retention policy.
- **Store entries**: actual data, managed by the provider backend (SQLite table `workflow_store_entries` for the `local` provider).

CLI surface: `orchestrator store get|put|delete|list|prune`.

---

## Database Schema Reference

```sql
-- Migration 7: workflow_store_entries
CREATE TABLE workflow_store_entries (
    store_name TEXT NOT NULL,
    project_id TEXT NOT NULL DEFAULT '',
    key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    task_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (store_name, project_id, key)
);
```

---

## Scenario 1: Store Put / Get / Delete via CLI (Local Backend)

### Preconditions
- Orchestrator binary is built and accessible via `orchestrator`
- Database initialized (`orchestrator init`)

### Goal
Verify basic CRUD operations against the default `local` (SQLite) backend without declaring a WorkflowStore CRD (auto-provisioning with defaults).

### Steps
1. Put a JSON value:
   ```bash
   orchestrator store put metrics bench_001 '{"test_count": 1334, "pass_rate": 0.98}'
   ```
2. Get the value back:
   ```bash
   orchestrator store get metrics bench_001
   ```
3. Put a second entry:
   ```bash
   orchestrator store put metrics bench_002 '{"test_count": 1400, "pass_rate": 0.99}'
   ```
4. List all entries in the store:
   ```bash
   orchestrator store list metrics
   ```
5. Delete the first entry:
   ```bash
   orchestrator store delete metrics bench_001
   ```
6. Verify deletion:
   ```bash
   orchestrator store get metrics bench_001
   ```

### Expected
- Step 1: Output `stored key 'bench_001' in 'metrics'`
- Step 2: Output is pretty-printed JSON: `{"test_count": 1334, "pass_rate": 0.98}`
- Step 4: Table output listing both `bench_001` and `bench_002` with their values and timestamps
- Step 5: Output `deleted key 'bench_001' from 'metrics'`
- Step 6: Output `key 'bench_001' not found in store 'metrics'`, exit code 1

### Expected Data State
```sql
-- After step 3 (two entries):
SELECT store_name, key, value_json FROM workflow_store_entries
WHERE store_name = 'metrics';
-- Returns: metrics|bench_001|{"test_count":1334,"pass_rate":0.98}
--          metrics|bench_002|{"test_count":1400,"pass_rate":0.99}

-- After step 5 (one entry):
SELECT COUNT(*) FROM workflow_store_entries WHERE store_name = 'metrics';
-- Returns: 1
```

---

## Scenario 2: WorkflowStore CRD Apply with Schema Validation

### Preconditions
- Orchestrator binary is built and accessible
- Database initialized

### Goal
Verify that a WorkflowStore CRD can be applied, and that its schema is enforced on write operations.

### Steps
1. Create a WorkflowStore manifest `test-store.yaml`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: WorkflowStore
   metadata:
     name: validated-metrics
   spec:
     provider: local
     schema:
       type: object
       properties:
         test_count:
           type: integer
           minimum: 0
         pass_rate:
           type: number
     retention:
       max_entries: 100
       ttl_days: 30
   ```
2. Apply the manifest:
   ```bash
   orchestrator apply -f test-store.yaml
   ```
3. Verify the CRD appears:
   ```bash
   orchestrator get workflowstores
   ```
4. Put a valid value:
   ```bash
   orchestrator store put validated-metrics run_001 '{"test_count": 50, "pass_rate": 0.95}'
   ```
5. Put an invalid value (negative test_count violates `minimum: 0`):
   ```bash
   orchestrator store put validated-metrics run_002 '{"test_count": -1, "pass_rate": 0.5}'
   ```
6. Put an invalid type (string instead of object):
   ```bash
   orchestrator store put validated-metrics run_003 '"not an object"'
   ```

### Expected
- Step 2: Output shows `Created` for `validated-metrics`
- Step 3: Table includes `validated-metrics` with provider `local`
- Step 4: Succeeds with `stored key 'run_001' in 'validated-metrics'`
- Step 5: Fails with error containing `below minimum`
- Step 6: Fails with error containing `expected type 'object'`

---

## Scenario 3: StoreBackendProvider CRD and Command Adapter

### Preconditions
- Orchestrator binary is built and accessible
- Database initialized

### Goal
Verify that a user-defined StoreBackendProvider CRD can be applied and that the command adapter dispatches CRUD operations to shell commands.

### Steps
1. Create a provider + store manifest `test-command-provider.yaml`:
   ```yaml
   apiVersion: orchestrator.dev/v2
   kind: StoreBackendProvider
   metadata:
     name: mock-file
   spec:
     commands:
       get: "cat /tmp/wfs-test/${STORE_NAME}-${KEY}.json 2>/dev/null || true"
       put: "mkdir -p /tmp/wfs-test && echo \"$VALUE\" > /tmp/wfs-test/${STORE_NAME}-${KEY}.json"
       delete: "rm -f /tmp/wfs-test/${STORE_NAME}-${KEY}.json"
       list: "echo '[]'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: WorkflowStore
   metadata:
     name: mock-store
   spec:
     provider: mock-file
   ```
2. Apply the manifest:
   ```bash
   orchestrator apply -f test-command-provider.yaml
   ```
3. Verify both CRDs appear:
   ```bash
   orchestrator get storebackendproviders
   orchestrator get workflowstores
   ```
4. Put a value via command adapter:
   ```bash
   orchestrator store put mock-store key1 '{"data": "hello"}'
   ```
5. Get the value back:
   ```bash
   orchestrator store get mock-store key1
   ```
6. Delete the value:
   ```bash
   orchestrator store delete mock-store key1
   ```
7. Verify deletion:
   ```bash
   orchestrator store get mock-store key1
   ```

### Expected
- Step 2: Output shows `Created` for both `mock-file` (StoreBackendProvider) and `mock-store` (WorkflowStore)
- Step 3: `storebackendproviders` lists `mock-file`; `workflowstores` lists `mock-store` with provider `mock-file`
- Step 4: Succeeds — file `/tmp/wfs-test/mock-store-key1.json` is created
- Step 5: Returns `{"data": "hello"}`
- Step 7: Returns empty (key not found), exit code 1

---

## Scenario 4: Store List with Output Formats and Upsert Behavior

### Preconditions
- Scenario 1 completed (or at least database initialized with `local` backend working)

### Goal
Verify list command output formats (table, JSON, YAML), upsert semantics on duplicate key, and project-scoped isolation.

### Steps
1. Put entries in two different projects:
   ```bash
   orchestrator store put shared k1 '{"v": 1}' --project proj-a
   orchestrator store put shared k2 '{"v": 2}' --project proj-a
   orchestrator store put shared k1 '{"v": 99}' --project proj-b
   ```
2. List entries for `proj-a`:
   ```bash
   orchestrator store list shared --project proj-a
   ```
3. List entries for `proj-b` in JSON format:
   ```bash
   orchestrator store list shared --project proj-b -o json
   ```
4. Upsert: overwrite `k1` in `proj-a`:
   ```bash
   orchestrator store put shared k1 '{"v": 100}' --project proj-a
   ```
5. Verify upsert:
   ```bash
   orchestrator store get shared k1 --project proj-a
   ```
6. Verify `proj-b` is unaffected:
   ```bash
   orchestrator store get shared k1 --project proj-b
   ```

### Expected
- Step 2: Table shows 2 entries (`k1`, `k2`) for `proj-a`
- Step 3: JSON array with 1 entry for `proj-b` (`k1` with value `{"v": 99}`)
- Step 5: Returns `{"v": 100}` (overwritten value)
- Step 6: Returns `{"v": 99}` (unaffected by proj-a upsert)

### Expected Data State
```sql
-- After all steps, three rows total:
SELECT store_name, project_id, key, value_json FROM workflow_store_entries
WHERE store_name = 'shared' ORDER BY project_id, key;
-- proj-a | k1 | {"v":100}
-- proj-a | k2 | {"v":2}
-- proj-b | k1 | {"v":99}
```

---

## Scenario 5: Builtin CRD Count and Describe Validation

### Preconditions
- Orchestrator binary is built and accessible
- Database initialized

### Goal
Verify that the two new builtin CRDs (WorkflowStore, StoreBackendProvider) are registered alongside the original 9, bringing the total to 11. Verify describe output for both new kinds.

### Steps
1. Verify builtin CRD count in unit tests (11 definitions):
   ```bash
   cd core && cargo test --lib -- crd::builtin_defs::tests::returns_eleven_definitions
   ```
2. Verify short names work for list queries:
   ```bash
   orchestrator get wfs
   orchestrator get sbp
   ```
3. Verify the `store` CLI help surface:
   ```bash
   orchestrator store --help
   ```
4. Verify both new kinds can be applied and listed:
   ```bash
   orchestrator get workflowstores
   orchestrator get storebackendproviders
   ```

### Expected
- Step 1: Unit test passes — 11 builtin CRD definitions
- Step 2: Short names `wfs` and `sbp` are accepted as aliases for their respective resource types
- Step 3: Help shows `get`, `put`, `delete`, `list`, `prune` subcommands
- Step 4: Both resource types list applied instances correctly

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Store Put / Get / Delete via CLI | ✅ | 2026-03-07 | claude | |
| 2 | WorkflowStore CRD with Schema Validation | PASS | 2026-03-15 | claude | Fixed: `get workflowstores` now routes through CRD fallback (was blocked by `!crd.builtin` guard) |
| 3 | StoreBackendProvider CRD and Command Adapter | PASS | 2026-03-15 | claude | Fixed: `get storebackendproviders` now routes through CRD fallback |
| 4 | Store List / Output Formats / Upsert / Project Isolation | ✅ | 2026-03-07 | claude | |
| 5 | Builtin CRD Count and Describe Validation | PASS | 2026-03-15 | claude | 11 builtin CRDs verified; short names (wfs/sbp) fixed via CRD fallback guard |
