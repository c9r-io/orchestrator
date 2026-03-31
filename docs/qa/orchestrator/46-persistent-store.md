---
self_referential_safe: true
---

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

## Scenario 1: Store Put / Get / Delete / List Round-Trip

### Goal
Verify basic CRUD operations for the local (SQLite) backend work correctly through unit tests.

### Steps

1. **Unit test** — verify full CRUD round-trip:
   ```bash
   cargo test -p agent-orchestrator --lib store_put_get_list_delete_round_trip
   ```

2. **Unit test** — verify store put serde config:
   ```bash
   cargo test -p orchestrator-config --lib test_post_action_store_put_serde_round_trip
   ```

### Expected
- `store_put` inserts entry; `store_get` retrieves it; `store_list` shows all entries; `store_delete` removes it
- PostAction store_put config serializes/deserializes correctly

---

## Scenario 2: WorkflowStore CRD Apply with Schema Validation

### Goal
Verify that WorkflowStore CRD configuration with retention policy works correctly.

### Steps

1. **Unit test** — verify retention/pruning behavior:
   ```bash
   cargo test -p agent-orchestrator --lib store_prune_uses_workflow_store_retention
   ```

2. **Unit test** — verify CRD projection round-trip (covers config serde indirectly):
   ```bash
   cargo test -p agent-orchestrator --lib workflow_store_config_round_trip
   ```

### Expected
- Pruning respects `max_entries` and `ttl_days` from WorkflowStore spec
- WorkflowStore config with default/custom retention round-trips correctly
- CRD projection preserves all fields

---

## Scenario 3: StoreBackendProvider CRD and Command Adapter

### Goal
Verify that StoreBackendProvider CRD configuration works correctly.

### Steps

1. **Unit test** — verify CRD projection round-trip (covers config serde indirectly):
   ```bash
   cargo test -p agent-orchestrator --lib store_backend_provider_config_round_trip
   ```

### Expected
- StoreBackendProvider config with default/custom options round-trips correctly
- CRD projection preserves command adapter configuration

---

## Scenario 4: Store List with Output Formats and Project Isolation

### Goal
Verify that store operations support multiple output formats and project-scoped isolation.

### Steps

1. **Code review** — verify project-scoped isolation in store table:
   ```bash
   rg -n "PRIMARY KEY.*store_name.*project_id.*key" core/src/persistence/migration_steps.rs
   ```

2. **Unit test** — verify export supports JSON and YAML output:
   ```bash
   cargo test -p agent-orchestrator --lib export_manifests_supports_json_and_yaml
   ```

3. **Code review** — verify store CLI dispatches CRUD to correct backend:
   ```bash
   rg -n "fn store_put|fn store_get|fn store_delete|fn store_list" core/src/service/store.rs
   ```

### Expected
- Primary key includes `(store_name, project_id, key)` — ensures project isolation
- Export supports both JSON and YAML output formats
- Store CLI has all 5 subcommands (get/put/delete/list/prune) dispatched to backend

---

## Scenario 5: Builtin CRD Count and Describe Validation

### Goal
Verify that WorkflowStore and StoreBackendProvider are registered as builtin CRDs.

### Steps

1. **Unit test** — verify builtin CRD registration count:
   ```bash
   cargo test -p agent-orchestrator --lib returns_eleven_definitions
   ```

2. **Code review** — verify both new kinds in CRD registry:
   ```bash
   rg -n "WorkflowStore|StoreBackendProvider" core/src/crd/builtin_defs.rs
   ```

### Expected
- Registry count includes WorkflowStore and StoreBackendProvider
- Both CRD kinds are registered in `builtin_defs.rs`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Store Put / Get / Delete / List | PASS | 2026-03-31 | claude | CRUD round-trip unit test |
| 2 | WorkflowStore CRD with Schema Validation | PASS | 2026-03-31 | claude | Retention + config serde |
| 3 | StoreBackendProvider CRD and Command Adapter | PASS | 2026-03-31 | claude | Config serde + CRD projection |
| 4 | Store List / Output Formats / Project Isolation | PASS | 2026-03-31 | claude | Project-scoped PK + output formats pass; S4 Step 1 file path corrected |
| 5 | Builtin CRD Count and Describe Validation | PASS | 2026-03-31 | claude | 11 builtin CRDs verified |
