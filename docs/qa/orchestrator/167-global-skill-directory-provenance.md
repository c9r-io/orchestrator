---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - Global Skill Directory Provenance

**Module**: orchestrator  
**Scope**: FR-117-A daemon-user ownership, permission-bit, and task-writable overlap enforcement for `fileSharing.globalSkills`  
**Scenarios**: 4  
**Priority**: High

---

## Background

Every configured global Skill directory is exposed read-only to every task sandbox. Configuration loading must reject an untrusted directory before any task can consume its contents. The gate returns `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED` with an actionable `suggested_fix` and requires no daemon, database, network, or external identity provider for its unit coverage.

---

## Scenario 1: Owner And Permission Bits Fail Closed

### Preconditions

- Run on Unix for UID and mode-bit assertions.
- Repository root is the current directory.

### Goal

Verify only the daemon effective user may own the directory and neither group nor world may write it.

### Steps

1. Run `cargo test -p orchestrator-config global_skill_owned_by_another_uid_is_rejected`.
2. Run `cargo test -p orchestrator-config group_writable_global_skill_is_rejected`.
3. Run `cargo test -p orchestrator-config world_writable_global_skill_is_rejected`.

### Expected

- A mismatched UID is rejected.
- Modes containing `0020` or `0002` write bits are rejected.
- Every rejection contains `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`, an authorization category, and an actionable fix.

---

## Scenario 2: Task-Writable Overlap Is Rejected At Config Load

### Preconditions

- Repository root is the current directory.

### Goal

Verify a global Skill cannot be equal to, contain, or be contained by a task write boundary.

### Steps

1. Run `cargo test -p orchestrator-config global_skill_overlapping_task_writable_path_is_rejected`.
2. Run `cargo test -p agent-orchestrator global_skill_inside_task_work_dir_is_rejected_during_config_load`.
3. Run `cargo test -p agent-orchestrator global_skill_overlapping_writable_profile_is_rejected_during_config_load`.
4. Run `cargo test -p agent-orchestrator global_skill_inside_managed_task_homes_is_rejected_during_config_load`.

### Expected

- A global Skill below a task `work_dir` is rejected before the configuration becomes active.
- A writable ExecutionProfile path below a global Skill is also rejected, protecting the whole Skill subtree.
- Diagnostics identify the Workspace or ExecutionProfile source and use `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`.

---

## Scenario 3: Trusted Isolated Directory Remains Supported

### Preconditions

- Repository root is the current directory.

### Goal

Verify the hardening preserves the intended owner-only supply path and defines unsupported-platform behavior.

### Steps

1. On Unix, run `cargo test -p orchestrator-config trusted_isolated_global_skill_is_accepted`.
2. Inspect the `#[cfg(not(unix))]` test `non_unix_global_skill_is_rejected_when_provenance_cannot_be_verified` in `crates/orchestrator-config/src/file_sharing.rs`.
3. Run `cargo test -p orchestrator-config file_sharing`.

### Expected

- A daemon-owned `0755` global Skill outside task-writable paths resolves successfully.
- Empty `globalSkills` remains valid on every platform.
- A configured global Skill fails closed on non-Unix because Unix provenance cannot be verified.

---

## Scenario 4: Existing Non-Code Vertical Flow Uses An Isolated Skill Root

### Preconditions

- Build with `cargo build -p orchestratord -p orchestrator-cli`.
- The deterministic mock fixture is `fixtures/manifests/bundles/non-code-workspace-fixture.yaml`.

### Goal

Verify the provenance gate composes with the existing sandbox read-only contract and Slack-to-task pilot.

### Steps

1. Run `scripts/qa/test-non-code-workspace.sh`.
2. Confirm the fixture creates sibling `workspace` and `global-skills` directories below its private QA root.
3. Confirm all seven assertions pass.

### Expected

- Configuration apply accepts the isolated daemon-owned Skill directory.
- The sandbox reads the Skill and rejects mutation.
- Signed Slack routing, HOME/XDG isolation, implicit-item convergence, Attention, and cleanup remain green.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Owner and permission bits fail closed | ✅ | 2026-07-23 | Codex | Targeted `orchestrator-config` tests passed |
| 2 | Task-writable overlap rejected | ✅ | 2026-07-23 | Codex | Config crate and core config-load tests passed |
| 3 | Trusted isolated directory and platform policy | ✅ | 2026-07-23 | Codex | Unix positive path and complete file-sharing suite passed |
| 4 | Existing non-code vertical flow | ✅ | 2026-07-23 | Codex | Isolated QA passed 7/7 |
