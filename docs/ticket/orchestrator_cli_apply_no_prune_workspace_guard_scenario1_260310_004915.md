# Ticket: Default apply without prune still triggers workspace deletion guard

**Created**: 2026-03-10 00:49:15
**QA Document**: `docs/qa/orchestrator/00-command-contract.md`
**Scenario**: #1
**Status**: FAILED

---

## Test Content
Verify the CLI friction fix for default `apply` semantics: when a project already contains multiple resources and the user reapplies a manifest containing only a workflow, `apply` without `--prune` should remain additive and must not be blocked by unrelated historical/task references.

---

## Expected Result
`orchestrator apply -f /tmp/qa-friction-subset.yaml --project qa-friction-noprune-verify` succeeds without trying to delete `workspace/cli_probe_ws`, because deletion is supposed to be explicit via `--prune`.

---

## Actual Result
The command failed with a deletion-guard error for `workspace/cli_probe_ws`, even though `--prune` was not passed.

---

## Repro Steps
1. Start the freshly built daemon from repo root: `./target/release/orchestratord --foreground`
2. Apply `fixtures/manifests/bundles/cli-probe-fixtures.yaml` into project `qa-friction-noprune-verify`
3. Create a pending task against workflow `probe_active_output`
4. Apply a manifest containing only `probe_task_scoped` without `--prune`

---

## Evidence

**UI/CLI Output**:
```text
Error: apply/delete would remove workspace/cli_probe_ws in project qa-friction-noprune-verify, but 1 non-terminal task(s) still reference it
blocking tasks:
- ad82b0c4-f9e3-4e08-971a-93c4f75c002a status=pending
suggested fixes:
- orchestrator task list --project qa-friction-noprune-verify
- orchestrator task delete <task_id> --force
- rerun without --prune if deletion is not intended
```

**Service Logs**:
```text
daemon restarted from ./target/release/orchestratord --foreground
fixture apply succeeded in project qa-friction-noprune-verify
task creation succeeded for workflow probe_active_output
subsequent apply without --prune failed with workspace deletion guard
```

**DB Checks (if applicable)**:
```sql
not executed
```

---

## Analysis

**Root Cause**: The live CLI path is still producing a candidate config that appears to remove `workspace/cli_probe_ws` during a default apply of a workflow-only manifest. This contradicts the intended merge-only semantics and suggests a persistence or config reconstruction mismatch between the real daemon path and the unit-tested service path.
**Severity**: High
**Related Components**: Backend / Database / CLI
