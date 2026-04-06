# Design: Pipeline Variable Spill Path Configurable (FR-092)

## Problem

Pipeline variable spill files were written to `{data_dir}/logs/{task_id}/{key}.txt`, which is outside the project workspace. Sandboxed agents (e.g., OpenCode) restrict file reads to the workspace directory, preventing them from accessing spill files containing plan output and other large pipeline variables. This caused unfair benchmark results between sandboxed and unrestricted agents.

## Solution

Added workspace-level `artifacts_dir` configuration so spill files are written inside the workspace tree.

### Configuration

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  artifacts_dir: ".orchestrator/artifacts"  # optional, relative to root_path
```

When omitted, defaults to `{root_path}/.orchestrator/artifacts`.

### Data Flow

```
WorkspaceConfig.artifacts_dir (Option<String>)
  -> ResolvedWorkspace.artifacts_dir (PathBuf, absolute)
  -> DB tasks.artifacts_dir (TEXT, absolute path string)
  -> TaskRuntimeContext.artifacts_dir (PathBuf)
  -> spill_large_var(artifacts_dir, ...) / spill_to_file(artifacts_dir, ...)
  -> {artifacts_dir}/{task_id}/{key}.txt
```

### Key Design Decisions

1. **DB pinning**: `artifacts_dir` is stored in the `tasks` table at creation time, ensuring task resumption uses the same path even if config changes. Follows the `ticket_dir`/`workspace_root` pattern.

2. **Eager directory creation**: The `artifacts_dir` is created during workspace resolution (`config_load/workspace.rs`), not lazily during spill. This avoids race conditions in concurrent task execution.

3. **Backward compatibility**: Existing tasks have an empty `artifacts_dir` column (via migration default). The runtime loader falls back to `{workspace_root}/.orchestrator/artifacts` for these tasks.

4. **Separation from daemon logs**: `InnerState.logs_dir` remains unchanged for daemon logging. Only pipeline variable spill uses the new `artifacts_dir`.

### Migration

- `m0026_add_artifacts_dir`: Adds `artifacts_dir TEXT NOT NULL DEFAULT ''` to the `tasks` table.

### Files Modified

- `crates/orchestrator-config/src/config/safety.rs` — `WorkspaceConfig.artifacts_dir`
- `crates/orchestrator-config/src/config/execution.rs` — `ResolvedWorkspace.artifacts_dir`, `TaskRuntimeContext.artifacts_dir`
- `crates/orchestrator-config/src/cli_types.rs` — `WorkspaceSpec.artifacts_dir`
- `core/src/config_load/workspace.rs` — Resolution logic
- `core/src/crd/builtin_defs.rs` — CRD schema
- `core/src/resource/workspace.rs` — Spec/config conversion
- `core/src/persistence/migration_steps.rs` — DB migration
- `core/src/task_ops.rs` — Task creation INSERT
- `core/src/task_repository/` — Runtime row loading
- `crates/orchestrator-scheduler/src/scheduler/runtime.rs` — Context loading
- `crates/orchestrator-scheduler/src/scheduler/item_executor/spill.rs` — Spill functions
- `crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch*.rs`, `apply.rs`, `accumulator.rs` — Callsites
