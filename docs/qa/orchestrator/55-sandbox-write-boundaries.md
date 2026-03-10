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

1. Apply an isolated project with a sandboxed writer agent:
   ```bash
   QA_PROJECT="qa-sandbox-write-deny"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   rm -f sandbox-denied.txt
   cat > /tmp/sandbox-write-deny.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: ExecutionProfile
   metadata:
     name: sandbox_docs_only
   spec:
     mode: sandbox
     fs_mode: workspace_rw_scoped
     writable_paths: [docs]
     network_mode: deny
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: sandbox-writer
   spec:
     capabilities: [implement]
     command: "set -e; echo blocked > sandbox-denied.txt; echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: sandbox-deny-root-write
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: sandbox_docs_only
         enabled: true
         scope: task
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-write-deny.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "sandbox deny root write" --goal "sandbox deny root write" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
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

1. Apply an isolated project with a sandboxed agent that writes to `docs/`:
   ```bash
   QA_PROJECT="qa-sandbox-write-allow"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   rm -f docs/sandbox-allowed.txt
   cat > /tmp/sandbox-write-allow.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: ExecutionProfile
   metadata:
     name: sandbox_docs_only
   spec:
     mode: sandbox
     fs_mode: workspace_rw_scoped
     writable_paths: [docs]
     network_mode: deny
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: "."
     qa_targets: [docs/qa]
     ticket_dir: docs/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: sandbox-writer
   spec:
     capabilities: [implement]
     command: "set -e; echo allowed > docs/sandbox-allowed.txt; echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: sandbox-allow-docs-write
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: sandbox_docs_only
         enabled: true
         scope: task
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-write-allow.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "sandbox allow docs write" --goal "sandbox allow docs write" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
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
