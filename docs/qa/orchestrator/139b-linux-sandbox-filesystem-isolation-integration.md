---
self_referential_safe: true
---

# Orchestrator - Linux Sandbox Filesystem Isolation Integration

**Module**: Orchestrator
**Scope**: Resolved profile propagation and namespace composition
**Scenarios**: 2
**Priority**: High

## Scenario 1: workspace_root Field Propagation

### Steps
1. Run `rg "workspace_root" crates/orchestrator-runner/src/runner/profile.rs`.
2. Verify `from_config()` stores `Some(workspace_root.to_path_buf())`.
3. Verify `host()` sets `workspace_root: None`.

### Expected
- `workspace_root` is correctly propagated for sandbox use.

## Scenario 2: Mount Namespace Composes Inside Network Namespace

### Steps
1. Run `rg "unshare -m" crates/orchestrator-runner/src/runner/sandbox_linux.rs`.
2. Confirm the generated command nests `unshare -m` inside `ip netns exec`.

### Expected
- The mount namespace is created inside the network namespace and cleans up with the process.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | workspace_root field propagation | ☐ | | | |
| 2 | Mount namespace composes inside network namespace | ☐ | | | |
