# Ticket: apply on another project is blocked by deletion guard from qa-friction-noprune-verify

**Created**: 2026-03-10 00:50:55
**QA Document**: `docs/qa/orchestrator/41-project-scoped-agent-selection.md`
**Scenario**: #2
**Status**: FAILED

---

## Test Content
Verify that project-scoped `apply` and `apply --prune` operate only on the requested project and are not blocked by resources/tasks from a different project.

---

## Expected Result
Applying manifests into `qa-friction-prune-verify`, `qa-friction-cross-a`, `qa-friction-cross-b`, or `qa-friction-prune-clean` should only evaluate deletions and task references inside the target project.

---

## Actual Result
Subsequent `apply` commands for other projects failed with a guard message referencing `qa-friction-noprune-verify` and `workspace/cli_probe_ws`.

---

## Repro Steps
1. Restart daemon from repo root using the newly built binary
2. Create project `qa-friction-noprune-verify`, apply the probe fixture, and create a task against `probe_active_output`
3. Run `apply` or `apply --prune` for a different project such as `qa-friction-prune-clean`

---

## Evidence

**UI/CLI Output**:
```text
Error: apply/delete would remove workspace/cli_probe_ws in project qa-friction-noprune-verify, but 1 non-terminal task(s) still reference it
blocking tasks:
- ad82b0c4-f9e3-4e08-971a-93c4f75c002a status=running
suggested fixes:
- orchestrator task list --project qa-friction-noprune-verify
- orchestrator task delete <task_id> --force
- rerun without --prune if deletion is not intended
```

**Service Logs**:
```text
2026-03-09T15:49:09Z INFO claimed task worker=1 task_id=ad82b0c4-f9e3-4e08-971a-93c4f75c002a
```

**DB Checks (if applicable)**:
```sql
not executed
```

---

## Analysis

**Root Cause**: The real daemon path appears to build a candidate config that removes resources outside the requested project, or loads a config snapshot that is already missing those projects. As a result, deletion guards for `qa-friction-noprune-verify` fire even when the user is applying to a different project.
**Severity**: High
**Related Components**: Backend / Database / CLI
