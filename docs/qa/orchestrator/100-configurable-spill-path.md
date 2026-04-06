---
self_referential_safe: true
---
# QA: FR-092 Configurable Spill Path

Verifies that pipeline variable spill files are written to the workspace-configured `artifacts_dir` instead of `{data_dir}/logs/`.

## Scenario 1: Default artifacts_dir

**Steps:**

1. Verify the `WorkspaceConfig` struct accepts `artifacts_dir: None` (default):

```bash
cargo test --workspace -q 2>&1 | tail -3
# Expected: all tests pass
```

2. Verify workspace resolution creates default path `.orchestrator/artifacts`:

```bash
rg "root_path.join.*\.orchestrator/artifacts" core/src/config_load/workspace.rs
# Expected: two matches (full resolution and lightweight resolution)
```

**Expected result:** When `artifacts_dir` is not set, the system defaults to `{root_path}/.orchestrator/artifacts`.

## Scenario 2: Custom artifacts_dir in workspace spec

**Steps:**

1. Verify the YAML field is accepted:

```bash
rg 'artifacts_dir' crates/orchestrator-config/src/config/safety.rs
# Expected: `pub artifacts_dir: Option<String>` with serde(default)
```

2. Verify CRD schema includes the field:

```bash
rg 'artifacts_dir' core/src/crd/builtin_defs.rs
# Expected: `"artifacts_dir": { "type": "string" }`
```

**Expected result:** Workspace spec and CRD both accept optional `artifacts_dir`.

## Scenario 3: Spill functions use artifacts_dir

**Steps:**

1. Verify spill function signatures use `artifacts_dir`:

```bash
rg 'fn spill_large_var|fn spill_to_file' crates/orchestrator-scheduler/src/scheduler/item_executor/spill.rs
# Expected: both functions show `artifacts_dir: &Path` parameter
```

2. Verify all callsites use `task_ctx.artifacts_dir`:

```bash
rg 'artifacts_dir' crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch_builtin.rs crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs
# Expected: all callsites reference `task_ctx.artifacts_dir`, none reference `state.logs_dir`
```

3. Verify no spill callsites use `state.logs_dir`:

```bash
rg 'state\.logs_dir' crates/orchestrator-scheduler/src/scheduler/item_executor/
# Expected: no matches
```

**Expected result:** All pipeline variable spill operations use the workspace-configured artifacts path.

## Scenario 4: DB persistence of artifacts_dir

**Steps:**

1. Verify migration adds the column:

```bash
rg 'm0026_add_artifacts_dir' core/src/persistence/migration_steps.rs
# Expected: ALTER TABLE tasks ADD COLUMN artifacts_dir
```

2. Verify task creation persists the path:

```bash
rg 'artifacts_dir' core/src/task_ops.rs
# Expected: artifacts_dir in INSERT statements
```

3. Verify runtime loading reads the column:

```bash
rg 'artifacts_dir' core/src/task_repository/queries.rs
# Expected: COALESCE(artifacts_dir,'') in SELECT
```

**Expected result:** artifacts_dir is persisted at task creation and loaded at task resumption.

## Scenario 5: Backward compatibility — empty DB column fallback

**Steps:**

1. Verify fallback logic in runtime context loading:

```bash
rg 'artifacts_dir.*is_empty' crates/orchestrator-scheduler/src/scheduler/runtime.rs
# Expected: fallback to workspace_root.join(".orchestrator/artifacts") when empty
```

**Expected result:** Tasks created before this migration (empty `artifacts_dir` column) fall back to `{workspace_root}/.orchestrator/artifacts`.

## Scenario 6: Unit tests pass

**Steps:**

```bash
cargo test --workspace -q 2>&1 | grep "^test result"
```

**Expected result:** All test suites pass with 0 failures.
