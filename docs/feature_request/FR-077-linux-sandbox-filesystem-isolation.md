# FR-077: Linux Sandbox Filesystem Isolation Backend

## Priority: P3

## Status: Planning

## Background

The `linux_native` sandbox backend (`crates/orchestrator-runner/src/runner/sandbox_linux.rs`) currently supports network isolation via network namespaces and nftables, but does not implement filesystem isolation. The code at `sandbox.rs:259-263` explicitly rejects any `fs_mode` other than `inherit`:

```
linux_native currently requires fs_mode=inherit until a Linux filesystem backend is implemented
```

macOS Seatbelt already supports `workspace_readonly` and `workspace_rw_scoped` via its sandbox profile language. This creates a cross-platform capability asymmetry documented in the user guide capability matrix.

## Requirements

### 1. Support `fs_mode: workspace_readonly`
- Mount the workspace directory as read-only for the sandboxed process
- Candidate approach: mount namespaces with bind-mount + `MS_RDONLY` remount

### 2. Support `fs_mode: workspace_rw_scoped`
- Allow write access only to paths listed in `writable_paths`
- Candidate approach: read-only base + bind-mount writable overlays for declared paths

### 3. Prerequisites
- Must work with existing `linux_native` prerequisites (root, `ip`, `nft`)
- Should compose cleanly with network namespace isolation (same process spawn)
- Must fail fast with clear error if kernel features are unavailable

## Design Considerations

- **Mount namespaces** (`CLONE_NEWNS` + `MS_PRIVATE` propagation) are the natural Linux primitive
- **OverlayFS** could provide copy-on-write semantics but adds kernel module dependency
- **bind-mount approach** is simpler: `mount --bind` + `mount -o remount,ro` for read-only paths
- Consider whether `pivot_root` or `chroot` is needed for stronger isolation
- Must handle `/tmp`, `/dev`, `/proc` access for process functionality

## References

- Design doc: `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` (line 77)
- Implementation: `crates/orchestrator-runner/src/runner/sandbox.rs:259-263`
- macOS reference: `crates/orchestrator-runner/src/runner/sandbox_macos.rs` (fs_mode handling)
- Governance: `docs/report/sandbox-network-enforcement-governance.md`
