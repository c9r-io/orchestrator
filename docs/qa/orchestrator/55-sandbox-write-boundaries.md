---
self_referential_safe: false
---

# Orchestrator - Sandbox Write Boundaries

**Module**: orchestrator
**Scope**: Validate that step-level sandbox profiles only permit writes to declared writable paths and reject writes outside those paths
**Scenarios**: 2
**Priority**: High

---

## Background

The macOS sandbox backend is wired through step-level `ExecutionProfile` resources. For `mode: sandbox` with `fs_mode: workspace_rw_scoped`, write access should be limited to:

- explicitly declared `writable_paths`
- orchestrator-managed log paths
- the process temp directory used by runtime plumbing

This means `/tmp` is not a valid denial target for QA, because it is intentionally left writable. The denial check in this doc uses a workspace-root file outside the declared writable subtree.

Entry point: `orchestrator`

---

## Scenario 1: Sandbox Denies Write Outside Declared Writable Paths

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- Runtime initialized.

### Goal

Ensure a sandboxed agent step cannot write to the workspace root when the profile only allows `docs/`.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-sandbox-write-deny"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   rm -f sandbox-denied.txt
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-deny-root-write --name "sandbox deny root write" --goal "sandbox deny root write" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Inspect the run result and stderr:
   ```bash
   orchestrator task get "${TASK_ID}"
   sqlite3 data/agent_orchestrator.db \
     "SELECT exit_code, stderr_path FROM command_runs ORDER BY started_at DESC LIMIT 1;"
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_denied' ORDER BY created_at DESC LIMIT 1;"
   STDERR_PATH=$(sqlite3 data/agent_orchestrator.db \
     "SELECT stderr_path FROM command_runs ORDER BY started_at DESC LIMIT 1;")
   test -n "${STDERR_PATH}" && cat "${STDERR_PATH}"
   test ! -f sandbox-denied.txt
   ```

### Expected

- The sandboxed `implement` run exits non-zero because it cannot write `sandbox-denied.txt` at workspace root.
- A `sandbox_denied` event exists for the task.
- Stderr contains a sandbox write denial such as `Operation not permitted`.
- `sandbox-denied.txt` is absent after the run.
- Note: current orchestrator finalization may still mark the task `completed` if no downstream QA/fix phase is configured; this does not invalidate the sandbox denial check.

---

## Scenario 2: Sandbox Still Allows Writes Inside Declared Writable Paths

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- Runtime initialized.

### Goal

Ensure the same sandbox profile can still write to a declared writable subtree.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-sandbox-write-allow"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   rm -f docs/sandbox-allowed.txt
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-allow-docs-write --name "sandbox allow docs write" --goal "sandbox allow docs write" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   cat docs/sandbox-allowed.txt
   ```
2. Confirm host-side state and cleanup:
   ```bash
   orchestrator task get "${TASK_ID}"
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_denied';"
   test -f docs/sandbox-allowed.txt
   rm -f docs/sandbox-allowed.txt
   ```

### Expected

- Task completes successfully.
- No `sandbox_denied` event is emitted for the run.
- `docs/sandbox-allowed.txt` exists and contains `allowed`.
- Cleanup removes the probe file after validation.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Denies Write Outside Declared Writable Paths | ☐ | | | |
| 2 | Sandbox Still Allows Writes Inside Declared Writable Paths | ☐ | | | |
