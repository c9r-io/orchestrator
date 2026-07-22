# File-Sharing Ceiling Authorization

**Scope**: FR-117 daemon-owned host path authority  
**Scenarios**: 5  
**Risk**: Critical  
**ASVS**: V4 Access Control

## Security Invariant

Every user-declared task Workspace, readable path, writable path, and global Skill path must be a canonical child of one `fileSharing.shareableRoots` entry. The ceiling is an authorization boundary, not a union with project permissions. Missing configuration denies all host sharing.

**Adversary**: the untrusted agent process (potentially prompt-injected by attacker-influenced content such as an incoming Slack message) and an untrusted project manifest attempting to expand host authority. With no repository boundary in a task workspace, this ceiling is one of only two remaining constraints (the other is HOME isolation, `../file-security/02-workspace-home-isolation.md`). The operator — via `file-sharing.yaml` — is the sole authority owner; a manifest can never widen it.

---

## Scenario 1: Missing Policy Denies Host Sharing

### Steps

1. Run `cargo test -p orchestrator-config missing_policy_is_deny_all`.
2. Run `cargo test -p agent-orchestrator task_workspace_rejects_targets_git_checkpoints_and_ceiling_escape`.

### Expected

- Empty policy contains no roots or global Skills.
- A task profile requesting `/tmp` fails with `FILE_SHARING_PATH_OUTSIDE_CEILING`.

---

## Scenario 2: Subset Comparison Rejects Prefix Confusion And Traversal

### Steps

1. Run `cargo test -p orchestrator-config ceiling_is_subset_not_union_or_prefix`.
2. Run `cargo test -p orchestrator-config parent_traversal_is_rejected_before_normalization`.

### Expected

- `/shared/nested` is allowed beneath `/shared`.
- `/shared-escape` is rejected.
- Any lexical `..` component is rejected before normalization.

---

## Scenario 3: Symlink Escape Is Evaluated After Canonicalization

### Steps

1. On Unix, run `cargo test -p orchestrator-config symlink_escape_is_checked_after_canonicalization`.

### Expected

- A symlink located under an allowed root but targeting a private sibling is rejected.

---

## Scenario 4: Global Skill Is Read-Only At Runtime

### Steps

1. Run `scripts/qa/test-non-code-workspace.sh`.
2. Confirm the “global Skill remains read-only” assertion passes.

### Expected

- Skill content is readable.
- A write attempt is denied even when the Skill directory is nested below a writable workspace.
- No mutation file remains.

## Scenario 5: Global Skill Directory Provenance

Global Skills are mounted read-only into *every* task sandbox, so the directory is a shared supply-chain surface: whoever can write it injects code into all tasks.

### Steps

1. Confirm the configured `fileSharing.globalSkills` directory is owned by and writable only by the daemon user (not group/world-writable, not a task-writable path).
2. Confirm no `globalSkills` entry points inside a task `work_dir` or any writable profile path.

### Expected

- The global Skill directory's provenance is trusted at the same level as the daemon binary.
- A `globalSkills` path that is also task-writable is treated as a misconfiguration, not a convenience.

## Operational Rule

Keep roots narrow and restart the daemon after editing `{data_dir}/file-sharing.yaml`. Never authorize the complete user home as a convenience fallback. Never point `globalSkills` at a directory writable by tasks or untrusted users.
