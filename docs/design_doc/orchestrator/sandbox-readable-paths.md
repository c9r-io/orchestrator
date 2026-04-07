# Design: Sandbox Configurable Readable Paths (FR-093)

## Problem

`ExecutionProfile` already supports `writable_paths` for explicit write allowlisting in scoped fs modes (FR-044). There was no equivalent `readable_paths` for explicit read allowlisting. This caused friction when:

1. Pipeline variable spill files (or other artifacts) live outside the workspace and need to be readable by sandboxed agents (companion to FR-092)
2. Multi-workspace scenarios need cross-workspace artifact reads
3. Shared resources (CI cache, model weights) live outside any workspace

## Solution

Mirror the existing `writable_paths` pattern with three additions:

1. New `readable_paths` field on `ExecutionProfileConfig` and `ExecutionProfileSpec` (and resolved on `ResolvedExecutionProfile`)
2. New path expansion utility for `~` (home dir) and `$VAR`/`${VAR}` (env vars)
3. Sandbox enforcement on Linux (bind-mount RO) and `ORCHESTRATOR_READABLE_PATHS` env var for cross-platform agent wrapper consumption

### Configuration

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: with-shared-artifacts
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - .orchestrator/artifacts
  readable_paths:
    - /shared/artifacts
    - ~/.orchestratord/logs
    - $CI_CACHE/data
```

### Path Expansion (`runner/path_expand.rs`)

A small in-crate utility implements:
- Leading `~` or `~/...` → `$HOME/...`
- `$NAME` and `${NAME}` → environment variable lookup
- Unset env vars are left in place (best-effort)

This avoids pulling in `shellexpand` for what is essentially 60 lines of well-tested code.

### Resolution (`runner/profile.rs`)

`ResolvedExecutionProfile::from_config()` resolves both `writable_paths` and `readable_paths` by:
1. Applying `expand_path()` to each entry
2. If absolute → use as-is; if relative → join with `workspace_root`

### Linux Sandbox (`runner/sandbox_linux.rs`)

In `build_fs_isolation_inner_script()`, after the writable_paths bind-mount loop (in `WorkspaceRwScoped` mode), a new loop emits read-only bind mounts for `readable_paths`:

```bash
if [ -e /shared/cache ]; then
  mount --bind /shared/cache /shared/cache && \
  mount -o remount,ro,bind /shared/cache /shared/cache
fi
```

This applies in **both** `WorkspaceReadonly` and `WorkspaceRwScoped` modes (the loop runs after the writable section). The `if [ -e ]` guard makes optional paths non-fatal.

### macOS Sandbox (`runner/sandbox_macos.rs`)

The current macOS Seatbelt profile unconditionally emits `(allow file-read*)`, so `readable_paths` is a **no-op on macOS today**. The code includes a comment explaining this:

```rust
// FR-093: `readable_paths` is currently a no-op on macOS because the
// profile above unconditionally emits `(allow file-read*)`.
```

If the macOS profile ever becomes read-restrictive, the rule emission can be added with no API changes.

### Env Var Injection (`scheduler/phase_runner/setup.rs`)

When `readable_paths` is non-empty, `setup.rs` injects:

```
ORCHESTRATOR_READABLE_PATHS=/shared/cache:/shared/data
```

(colon-joined, similar to `$PATH`). Agent wrapper scripts can read this env var and apply it to their own sandboxes (Codex `--add-dir`, OpenCode config, etc.). The orchestrator itself is agent-agnostic and does not build agent-specific CLI flags.

### Validation (`config_load/validate/execution_profiles.rs`)

The host-profile sanity check now also rejects non-empty `readable_paths` on host-mode profiles, mirroring the existing `writable_paths` rejection.

## Key Design Decisions

1. **Mirror writable_paths exactly** for the data model — same Vec<String> shape, same skip_serializing_if, same resolution logic
2. **Path expansion as a separate utility** — keeps `from_config` simple and unit-testable
3. **macOS no-op acknowledged** — better to document than to emit redundant Seatbelt rules
4. **Env var as cross-cutting mechanism** — orchestrator's sandbox enforces what it can; agent CLIs that have their own sandboxes consume the env var

## Files Modified

- `crates/orchestrator-runner/src/runner/path_expand.rs` (NEW)
- `crates/orchestrator-runner/src/runner/mod.rs` (mod registration + Linux test fixture update)
- `crates/orchestrator-runner/src/runner/profile.rs` (struct field, resolution, tests)
- `crates/orchestrator-runner/src/runner/sandbox_linux.rs` (bind-mount loop + tests)
- `crates/orchestrator-runner/src/runner/sandbox_macos.rs` (no-op comment)
- `crates/orchestrator-config/src/config/execution_profile.rs` (struct field)
- `crates/orchestrator-config/src/cli_types.rs` (spec field)
- `crates/orchestrator-scheduler/src/scheduler/phase_runner/setup.rs` (env var injection)
- `crates/orchestrator-scheduler/src/scheduler/phase_runner/tests.rs` (struct literal update)
- `core/src/config_load/validate/execution_profiles.rs` (host-profile guard)
- `core/src/config_load/validate/tests.rs` (validation test)
- `core/src/resource/execution_profile.rs` (spec ↔ config mapping)
- `core/src/resource/test_fixtures.rs`, `core/src/resource/tests.rs` (struct literal updates)
