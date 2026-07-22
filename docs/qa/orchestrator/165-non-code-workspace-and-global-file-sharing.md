# Orchestrator - Non-Code Workspace And Global File Sharing

**Module**: orchestrator  
**Scope**: FR-117 task Workspace, file-sharing ceiling, sandbox isolation, convergence, Slack pilot, and Console presentation  
**Scenarios**: 5  
**Priority**: High

---

## Background

The executable fixture uses deterministic shell agents and a local fake Slack API. It does not call a live AI provider or consume API credits.

Primary gate:

```bash
cargo build -p orchestratord -p orchestrator-cli
scripts/qa/test-non-code-workspace.sh
```

Fixture: `fixtures/manifests/bundles/non-code-workspace-fixture.yaml`.

---

## Scenario 1: Backward-Compatible Workspace Schema And Semantic Gates

### Preconditions

- Repository root is the current directory.

### Goal

Verify legacy code-repository manifests and explicit task manifests select distinct required fields.

### Steps

1. Run `cargo test -p agent-orchestrator resource::workspace`.
2. Run `cargo test -p agent-orchestrator config_load::workspace`.
3. Confirm tests cover `root_path` aliasing, missing code-repository `work_dir`, task QA fields, and task self-reference.

### Expected

- Legacy `root_path` loads and serializes as `work_dir`.
- `code_repo` keeps QA/ticket requirements.
- `task` accepts omitted `work_dir` and rejects QA/self-reference fields with stable codes.

---

## Scenario 2: Ceiling, Sandbox, And HOME Isolation

### Preconditions

- Apply the deterministic fixture only through `scripts/qa/test-non-code-workspace.sh`.

### Goal

Verify canonical subset authorization, read-only global Skills, and redirected HOME/XDG values.

### Steps

1. Run `cargo test -p orchestrator-config file_sharing`.
2. Run `cargo test -p orchestrator-runner strict_`.
3. Run the provenance suite in `docs/qa/orchestrator/167-global-skill-directory-provenance.md`.
4. Run the primary QA gate and inspect its HOME/Skill result.

### Expected

- Missing policy denies host sharing.
- Sibling-prefix, `..`, and symlink escapes fail.
- UID mismatch, group/world writes, and task-writable overlap fail before activation.
- The agent reads the global Skill but cannot mutate it.
- HOME and XDG paths equal the workspace, never the daemon user's original HOME.

---

## Scenario 3: Implicit Item, Preflight Rejections, And Cleanup

### Preconditions

- Repository root is the current directory.

### Goal

Verify task creation and lifecycle semantics without QA-file discovery.

### Steps

1. Run `cargo test -p agent-orchestrator task_workspace`.
2. Run `cargo test -p orchestrator-scheduler delete_task_impl_removes_task_and_log_files`.
3. Run the primary QA gate.

### Expected

- Exactly one `__TASK__` item exists internally.
- Explicit target files, Git checkpoints, unscoped execution, dynamic items, and ceiling escape fail closed.
- An omitted `work_dir` creates a unique `0700` HOME.
- Failed task creation and terminal/deleted tasks do not leave managed HOME directories.

### Expected Data State

```sql
SELECT qa_file_path FROM task_items WHERE task_id = '{task_id}';
-- Expected: __TASK__
```

---

## Scenario 4: Signed Slack Badge To Attention Vertical Flow

### Preconditions

- `cargo build -p orchestratord -p orchestrator-cli`
- `scripts/qa/test-non-code-workspace.sh` uses only the mock fixture and loopback fake Slack service.

### Goal

Verify the production source-routing path can target a non-code workspace.

### Steps

1. Run `scripts/qa/test-non-code-workspace.sh`.
2. Observe the seven reported assertions.
3. If diagnosis is needed, rerun with `KEEP_FR117_QA=1` and inspect the printed `QA_ROOT`.

### Expected

- A correctly signed `warehouse-check` reaction resolves a permalink and creates one task.
- The agent receives the source-derived goal, reads `inventory.txt`, reads the global Skill, and creates a reply suggestion.
- Successful driver termination verifies the implicit item without QA finalize.
- A low-confidence step creates Attention history for human review.

---

## Scenario 5: Process Console Non-Code Presentation

### Preconditions

- `cd gui && npm ci` has completed.

### Goal

Verify the Console exposes task semantics without leaking storage implementation labels.

### Steps

1. Run `cd gui && npm test -- --run`.
2. Run `cd gui && npm run test:e2e -- process-console.spec.ts --grep 'non-code process'`.
3. Open the mocked non-code process in the Playwright trace if the test fails.

### Expected

- Process overview shows “Workspace type: Task”.
- Expert workflow labels the item “Task”.
- `__TASK__` is absent from visible UI.
- Existing code-repository Process Console tests remain unchanged.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Schema and semantic gates | ✅ | 2026-07-22 | Codex | Rust targeted tests passed |
| 2 | Ceiling, sandbox, and HOME | ✅ | 2026-07-22 | Codex | macOS strict sandbox and live pilot passed |
| 3 | Implicit item and cleanup | ✅ | 2026-07-22 | Codex | Private HOME and cleanup passed |
| 4 | Slack-to-Attention vertical flow | ✅ | 2026-07-22 | Codex | 7/7 controlled assertions passed |
| 5 | Process Console UI | ✅ | 2026-07-22 | Codex | Vitest and Playwright passed |
