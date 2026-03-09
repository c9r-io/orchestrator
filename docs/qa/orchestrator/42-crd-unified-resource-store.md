# Unified CRD Resource Store — Builtin Type Migration

**Module**: orchestrator
**Scope**: ResourceStore unified pipeline, CrdProjectable projection, writeback to legacy fields, normalize_config CRD bootstrap, apply/delete store integration
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator previously maintained two parallel resource pipelines:
- **Builtin pipeline**: YAML → `OrchestratorResource` (strongly-typed `ResourceSpec` enum) → 9 independent `HashMap` fields
- **CRD pipeline**: YAML → `CustomResourceManifest` (`serde_json::Value`) → schema/CEL validation → single `custom_resources` HashMap

These were unified into a **single ResourceStore** that acts as the write point for all resources (builtin + user-defined CRDs). Builtin resource instances such as Agent, Workflow, Workspace, Project, RuntimePolicy, StepTemplate, EnvStore, and SecretStore are stored in the ResourceStore and projected back into the in-memory config snapshot. Project-scoped resources are written under `projects.<id>.*` and stored with namespaced keys in the ResourceStore.

**Key components**:
- `crd/store.rs` — `ResourceStore`: unified HashMap keyed by `"{Kind}/{project}/{name}"` for project-scoped resources
- `crd/projection.rs` — `CrdProjectable` trait: round-trip typed config ↔ `serde_json::Value`
- `crd/writeback.rs` — `write_back_single()`, `remove_from_legacy()`, `seed_store_from_legacy()`, `sync_legacy_to_store()`
- `crd/builtin_defs.rs` — builtin CRD definitions for supported builtin resource kinds
- `config_load/normalize.rs` — `normalize_config()`: ensures builtin CRDs exist, rebuilds store from legacy fields
- `resource/mod.rs` — `apply_to_store()`, `delete_from_store()`, `metadata_from_store()`

**Entry points**:
- `orchestrator apply -f <manifest.yaml>` — resources flow through ResourceStore
- `orchestrator get <kind>/<name> -o yaml` — reads from legacy fields (projection cache)
- `orchestrator delete <kind>/<name>` — removes from both store and legacy
- `orchestrator manifest export -o yaml` — exports from legacy fields

---

## Scenario 1: Builtin CRD Bootstrap on Normalize

### Preconditions
- Orchestrator binary is built
- A fresh or existing config database

### Goal
Verify that `normalize_config` ensures builtin CRD definitions exist and that the ResourceStore is populated from the normalized project-scoped config snapshot.

### Steps

1. Initialize the orchestrator:
   ```bash
   orchestrator init
   ```

2. Apply a simple agent to populate legacy fields:
   ```bash
   cat <<'EOF' | orchestrator apply -f -
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: test-bootstrap-agent
   spec:
     command: "echo {prompt}"
   EOF
   ```

3. Verify the agent exists:
   ```bash
   orchestrator get agent/test-bootstrap-agent -o yaml
   ```

4. Verify via unit tests that all 9 builtin CRDs are registered:
   ```
   cargo test --lib "config_load::normalize::tests::normalize_config_populates_builtin_crds"
   ```

5. Verify the ResourceStore is populated after normalization:
   ```
   cargo test --lib "config_load::normalize::tests::normalize_config_rebuilds_resource_store_from_legacy"
   ```

### Expected
- `init` succeeds with exit code 0
- Agent is created and retrievable
- Builtin CRD definitions are present after normalization
- ResourceStore contains entries matching the project-scoped config snapshot after normalization

---

## Scenario 2: CrdProjectable Round-Trip for All 9 Types

### Preconditions
- Orchestrator binary is built

### Goal
Verify that builtin config types can be converted to a CRD spec (`to_cr_spec`) and back (`from_cr_spec`) without data loss.

### Steps

1. Run all projection round-trip unit tests:
   ```bash
   cargo test --lib "crd::projection::tests"
   ```

2. Verify the following round-trip tests pass:
   - `agent_config_round_trip` — command + capabilities preserved
   - `workspace_config_round_trip` — root_path + qa_targets preserved
   - `step_template_config_round_trip` — prompt + description preserved
   - `env_store_config_round_trip` — data map + sensitive=false preserved
   - `secret_store_projection_round_trip` — data map + sensitive=true preserved
   - `runtime_policy_projection_round_trip` — runner shell + resume.auto preserved
   - `project_config_round_trip` — description preserved (nested maps not projected)
   - `workflow_config_round_trip` — steps (plan + self_test) + loop_policy preserved
   - `workflow_config_round_trip_preserves_loop_mode` — LoopMode::Fixed round-trips correctly

3. Verify malformed spec rejection:
   ```bash
   cargo test --lib "crd::projection::tests::from_cr_spec_rejects_malformed_agent_spec"
   ```

### Expected
- All 12 projection tests pass
- `from_cr_spec` with missing required fields returns `Err`
- No data loss in any round-trip for valid configs

---

## Scenario 3: Targeted Writeback — write_back_single and remove_from_legacy

### Preconditions
- Orchestrator binary is built

### Goal
Verify that `write_back_single` updates exactly one projected entry in the config snapshot without affecting others, and `remove_from_legacy` removes exactly one projected entry.

### Steps

1. Run all writeback unit tests:
   ```bash
   cargo test --lib "crd::writeback::tests"
   ```

2. Verify targeted writeback for each kind:
   - `write_back_single_agent` — updates `config.projects[project].agents["name"]`
   - `write_back_single_workflow` — updates `config.projects[project].workflows["name"]`
   - `write_back_single_workspace` — updates `config.projects[project].workspaces["name"]`
   - `write_back_single_project` — updates `config.projects["name"]`, preserves sub-resources
   - `write_back_single_defaults` — updates `config.defaults` singleton
   - `write_back_single_runtime_policy` — updates `config.runner` + `config.resume`
   - `write_back_single_step_template` — updates `config.step_templates["name"]`
   - `write_back_single_env_store` — updates `config.env_stores["name"]` with sensitive=false
   - `write_back_single_secret_store` — updates `config.env_stores["name"]` with sensitive=true

3. Verify removal for map-based kinds:
   - `remove_from_legacy_agent` — removes from `config.projects[project].agents`
   - `remove_from_legacy_workflow` — removes from `config.projects[project].workflows`
   - `remove_from_legacy_workspace` — removes from `config.projects[project].workspaces`
   - `remove_from_legacy_project` — removes from `config.projects`
   - `remove_from_legacy_step_template` — removes from `config.step_templates`
   - `remove_from_legacy_env_store` — removes from `config.env_stores`

4. Verify singleton removal is a no-op:
   - `remove_from_legacy_defaults_is_noop` — Defaults singleton cannot be removed

5. Verify EnvStore/SecretStore splitting:
   - `project_env_store_preserves_sensitive_stores` — env store projection does not overwrite secrets
   - `project_secret_store_preserves_non_sensitive_stores` — secret store projection does not overwrite env stores

### Expected
- All 27 writeback tests pass
- Single-entry writeback does not affect other entries in the same map
- Project writeback preserves sub-resources (workspaces, agents, workflows)
- EnvStore and SecretStore entries coexist correctly in `config.env_stores`

---

## Scenario 4: apply_to_store / delete_from_store Integration

### Preconditions
- Orchestrator binary is built

### Goal
Verify the unified apply/delete pipeline through ResourceStore with correct change detection and legacy field synchronization.

### Steps

1. Run apply/delete store integration tests:
   ```bash
   cargo test --lib "resource::tests::apply_to_store"
   cargo test --lib "resource::tests::delete_from_store"
   ```

2. Verify apply change detection:
   - `apply_to_store_returns_created_for_new_resource` — first apply returns `Created`
   - `apply_to_store_returns_unchanged_for_identical` — re-apply same spec returns `Unchanged`
   - `apply_to_store_returns_configured_for_changed` — changed spec returns `Configured`
   - `apply_to_store_seeds_from_legacy_for_correct_change_detection` — existing legacy resource detected as `Unchanged` (not `Created`)
   - `apply_to_store_increments_generation` — generation counter increases on apply

3. Verify delete behavior:
   - `delete_from_store_removes_from_both_store_and_legacy` — removed from ResourceStore AND legacy field
   - `delete_from_store_seeds_from_legacy_and_removes` — legacy-only resource can be deleted via store
   - `delete_from_store_returns_false_for_missing` — deleting non-existent resource returns false

4. Verify metadata reads from the namespaced store:
   ```bash
   cargo test --lib "resource::tests::metadata_from_store"
   ```
   - `metadata_from_store_returns_cr_metadata` — labels/annotations read from CustomResource
   - `metadata_from_store_falls_back_to_name_only` — returns name-only metadata when not in store

### Expected
- All 11 resource integration tests pass
- Change detection correctly distinguishes Created / Unchanged / Configured
- Legacy seeding ensures correct detection when store is empty but legacy has data
- Delete synchronizes both store and legacy fields
- Metadata reads from ResourceStore using project-scoped keys when applicable

---

## Scenario 5: ResourceStore Edge Cases — Key Isolation and Corruption Resilience

### Preconditions
- Orchestrator binary is built

### Goal
Verify ResourceStore correctness for edge cases: cross-kind key isolation, prefix substring safety, corrupted spec resilience, and singleton projection.

### Steps

1. Run ResourceStore edge case tests:
   ```bash
   cargo test --lib "crd::store::tests"
   ```

2. Verify cross-kind isolation:
   - `cross_kind_key_isolation` — same name under different kinds stored independently; removing one doesn't affect the other
   - `list_by_kind_does_not_match_prefix_substring` — listing "Foo" does not include "FooBar" entries

3. Verify generation counter edge cases:
   - `generation_does_not_increment_on_failed_remove` — removing non-existent entry keeps generation unchanged
   - `generation_increments_on_unchanged_put` — even unchanged put increments generation

4. Verify namespaced key format:
   - `get_namespaced_uses_three_segment_key` — `Kind/project/name` format for namespaced lookup

5. Verify singleton projection with real types:
   - `project_singleton_defaults_round_trip` — ConfigDefaults round-trips through store
   - `project_singleton_runtime_policy` — RuntimePolicyProjection round-trips through store
   - `project_singleton_returns_none_for_empty_store` — empty store returns None

6. Verify corruption resilience:
   - `project_map_skips_corrupted_specs` — unparseable specs silently skipped, valid entries preserved

7. Verify metadata change detection:
   - `put_detects_metadata_change_as_configured` — same spec but different labels returns `Configured`

### Expected
- All 20 ResourceStore tests pass
- Cross-kind key isolation prevents accidental overwrites
- Corrupted specs do not crash projection — silently skipped
- Singleton projection returns typed config or None
- Metadata changes are detected as `Configured`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Builtin CRD Bootstrap on Normalize | PASS | 2026-03-05 | claude | 44 normalize tests pass, 9 builtin CRDs confirmed |
| 2 | CrdProjectable Round-Trip for All 9 Types | PASS | 2026-03-05 | claude | 12 projection tests pass |
| 3 | Targeted Writeback — write_back_single and remove_from_legacy | PASS | 2026-03-05 | claude | 27 writeback tests pass |
| 4 | apply_to_store / delete_from_store Integration | PASS | 2026-03-05 | claude | 11 resource tests pass, legacy seeding verified |
| 5 | ResourceStore Edge Cases — Key Isolation and Corruption Resilience | PASS | 2026-03-05 | claude | 20 store tests pass |
