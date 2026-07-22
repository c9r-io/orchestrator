# Task Workspace HOME Isolation

**Scope**: FR-117 task cwd/HOME confinement and lifecycle  
**Scenarios**: 4  
**Risk**: Critical  
**ASVS**: V12 File And Resource Handling

## Security Invariant

A task agent must not inherit or read the daemon user's ambient HOME. HOME, XDG locations, and temporary files are redirected beneath the resolved task workspace. Daemon-managed homes are private and ephemeral.

**Adversary**: the untrusted agent process (potentially prompt-injected). Its likely objective after injection is to reach the daemon user's real home — `~/.ssh`, credential files, provider tokens under `~/.config` — by dereferencing `$HOME`, `~`, or inherited `XDG_*`. This invariant removes those references at the source (forced env) and, on Linux, masks the ambient home with tmpfs so even a hardcoded absolute path resolves to nothing. It is the second of the two remaining constraints once the repository boundary is gone (the first is the file-sharing ceiling, `../authorization/02-file-sharing-ceiling.md`).

---

## Scenario 1: Environment Redirection

### Steps

1. Run `scripts/qa/test-non-code-workspace.sh`.
2. Inspect retained evidence only when using `KEEP_FR117_QA=1`.

### Expected

- `HOME={work_dir}`.
- Every `XDG_*` and `TMPDIR` value is beneath `{work_dir}`.
- Evidence contains no original daemon HOME.

---

## Scenario 2: macOS Strict Read Profile

### Steps

1. On macOS, run `cargo test -p orchestrator-runner strict_macos_profile_does_not_grant_ambient_home_reads`.

### Expected

- Seatbelt has no ambient `(allow file-read*)` rule.
- Only system, workspace, and explicitly readable roots are granted.
- Read-only paths receive explicit write denial.

---

## Scenario 3: Linux Ambient HOME Masking

### Steps

1. On Linux, run `cargo test -p orchestrator-runner strict_profile_masks_host_home_and_restores_only_allowed_subpaths`.

### Expected

- The host HOME is covered by a private `tmpfs`.
- Only workspace and explicitly approved paths are rebound into the namespace.

---

## Scenario 4: Private Allocation And Cleanup

### Steps

1. Run `cargo test -p agent-orchestrator task_workspace_materializes_one_implicit_item_and_private_home`.
2. Run `cargo test -p orchestrator-scheduler delete_task_impl_removes_task_and_log_files`.
3. Run `scripts/qa/test-non-code-workspace.sh`.

### Expected

- Each omitted `work_dir` gets a distinct `{data_dir}/task-homes/{task_id}` with mode `0700`.
- Failed creation cleans allocations through a guard.
- Terminal tasks remove managed HOME; task deletion also removes managed HOME and artifacts.
- User-owned persistent `work_dir` is never deleted.
