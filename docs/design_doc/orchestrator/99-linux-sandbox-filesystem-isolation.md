# Design Doc 99: Linux Sandbox Filesystem Isolation

## Status: Implemented

## Context

The `linux_native` sandbox backend supported network isolation (network namespaces + nftables) but had no filesystem isolation. Any `fs_mode` other than `inherit` was rejected at preflight validation. macOS Seatbelt already supported `workspace_readonly` and `workspace_rw_scoped`, creating a cross-platform capability gap.

## Design

### Approach: Mount Namespaces + Bind Mounts

Linux mount namespaces (`CLONE_NEWNS` via `unshare -m`) provide per-process mount tables.
Bind mounts remap existing paths within the namespace without kernel module dependencies (unlike OverlayFS).

### Execution Flow

For `fs_mode != inherit`, the sandbox script wraps the command execution:

```
ip netns exec "$NETNS" unshare -m -- /bin/bash -c '<inner-script>'
```

The inner script:
1. `mount --make-rprivate /` -- prevents mount propagation to parent namespace
2. `mount --bind <workspace> <workspace>` -- bind workspace onto itself
3. `mount -o remount,ro,bind <workspace> <workspace>` -- make it read-only
4. (For `workspace_rw_scoped` only) Re-bind each `writable_path` read-write
5. `exec <shell> <shell_arg> <command>` -- run the actual command

### Composition with Network Namespace

Mount namespace is created **inside** the network namespace. This ensures:
- Network rules are already in effect when mount operations begin
- Mount changes cannot propagate to the parent or host namespace
- Cleanup of network namespace also tears down mount namespace

### Prerequisites

When `fs_mode != inherit`, preflight validation checks for `unshare` and `mount` binaries in PATH, in addition to existing `ip`, `nft`, and root requirements.

### workspace_root in ResolvedExecutionProfile

A `workspace_root: Option<PathBuf>` field was added to `ResolvedExecutionProfile` so the mount script knows the exact workspace path to make read-only. Set by `from_config()`, `None` for the built-in `host()` profile.

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Bind-mount over OverlayFS | Simpler, no kernel module dependency, sufficient for deny-write use case |
| `mount --make-rprivate /` | systemd defaults to `shared` propagation; rprivate prevents host leakage |
| Mount ns inside net ns | Cleanest composition; mount changes auto-cleanup with net ns teardown |
| Guard writable_paths with `[ -e ]` | Optional paths shouldn't abort the sandbox script |
| No pivot_root/chroot | Bind-mount isolation is sufficient for workspace scoping; full root isolation out of scope |

## Files Modified

- `crates/orchestrator-runner/src/runner/profile.rs` -- added `workspace_root` field
- `crates/orchestrator-runner/src/runner/sandbox.rs` -- replaced hard fs_mode rejection with `unshare`/`mount` binary checks
- `crates/orchestrator-runner/src/runner/sandbox_linux.rs` -- added `build_fs_isolation_inner_script()`, integrated into `build_linux_sandbox_script()`
- `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` -- updated capability description

## References

- FR-091: `docs/feature_request/FR-091-linux-sandbox-filesystem-isolation.md`
- Parent design: `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md`
- QA: `docs/qa/orchestrator/139-linux-sandbox-filesystem-isolation.md`
