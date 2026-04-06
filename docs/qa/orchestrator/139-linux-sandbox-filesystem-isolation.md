---
self_referential_safe: true
---

# QA-139: Linux Sandbox Filesystem Isolation

Verifies the Linux `linux_native` sandbox backend correctly implements filesystem isolation via mount namespaces and bind mounts for `workspace_readonly` and `workspace_rw_scoped` modes.

## Scenario 1: build_fs_isolation_inner_script returns None for Inherit

**Steps:**
1. `cargo test -p orchestrator-runner -- linux_fs_isolation::test_inherit_returns_none` (Linux only)
2. On non-Linux: verify by code inspection that `build_fs_isolation_inner_script` returns `None` for `ExecutionFsMode::Inherit`

**Expected:** Test passes; no mount-namespace script is generated for inherit mode.

## Scenario 2: workspace_readonly generates read-only mount script

**Steps:**
1. `cargo test -p orchestrator-runner -- linux_fs_isolation::test_workspace_readonly_mounts_ro` (Linux only)
2. On non-Linux: `rg "mount --make-rprivate" crates/orchestrator-runner/src/runner/sandbox_linux.rs` confirms the mount script contains:
   - `mount --make-rprivate /`
   - `mount --bind <workspace> <workspace>`
   - `mount -o remount,ro,bind <workspace> <workspace>`
3. Confirm no writable re-bind lines are generated (no `if [ -e` in readonly mode)

**Expected:** Script makes workspace read-only without any writable path exceptions.

## Scenario 3: workspace_rw_scoped with writable_paths generates selective re-bind

**Steps:**
1. `cargo test -p orchestrator-runner -- linux_fs_isolation::test_workspace_rw_scoped_with_writable_paths` (Linux only)
2. On non-Linux: verify by reading `sandbox_linux.rs` that for each `writable_path`:
   - `if [ -e <path> ]; then mount --bind <path> <path>; fi` is emitted

**Expected:** Workspace is read-only base, declared writable paths are re-mounted read-write.

## Scenario 4: workspace_rw_scoped without writable_paths

**Steps:**
1. `cargo test -p orchestrator-runner -- linux_fs_isolation::test_workspace_rw_scoped_without_paths` (Linux only)

**Expected:** Script mounts workspace read-only; no writable re-bind lines emitted.

## Scenario 5: Preflight validation checks unshare/mount for non-Inherit fs_mode

**Steps:**
1. `rg "unshare.*mount" crates/orchestrator-runner/src/runner/sandbox.rs` confirms preflight checks for `unshare` and `mount` binaries when `fs_mode != Inherit`
2. Verify the old hard-rejection message `"linux_native currently requires fs_mode=inherit"` no longer exists:
   `rg "currently requires fs_mode=inherit" crates/orchestrator-runner/` returns no results

**Expected:** Preflight validation dynamically checks for required binaries instead of blanket rejection.

## Scenario 6: workspace_root field propagation

**Steps:**
1. `rg "workspace_root" crates/orchestrator-runner/src/runner/profile.rs` confirms the field exists in `ResolvedExecutionProfile`
2. Verify `from_config()` stores `Some(workspace_root.to_path_buf())`
3. Verify `host()` sets `workspace_root: None`

**Expected:** workspace_root is correctly propagated for sandbox use.

## Scenario 7: Mount namespace composes inside network namespace

**Steps:**
1. `rg "unshare -m" crates/orchestrator-runner/src/runner/sandbox_linux.rs` confirms the pattern:
   `ip netns exec "$NETNS" unshare -m -- /bin/bash -c ...`

**Expected:** Mount namespace is created inside the network namespace, ensuring clean composition and automatic cleanup.
