# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Scope**: Validate sandbox resource limit enforcement, structured network-blocked events, and explicit rejection of unsupported network allowlists
**Scenarios**: 3
**Priority**: High

---

## Background

The orchestrator now closes the remaining FR-001 sandbox gaps on the active macOS backend:

- Unix child processes apply resource limits from `ExecutionProfile`
- sandbox failures emit structured `sandbox_resource_exceeded` and `sandbox_network_blocked` events
- `network_mode=allowlist` is rejected explicitly when the current backend cannot enforce it

This document extends the existing execution-profile and sandbox-write QA coverage:

- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`

Entry point: `orchestrator`

---

## Scenario 1: Sandbox Emits sandbox_resource_exceeded for max_open_files

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure a sandboxed step with a low `max_open_files` limit fails with `sandbox_resource_exceeded`.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-sandbox-open-files-limit"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-open-files-limit --name "sandbox fd limit" --goal "sandbox fd limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Inspect the event and stderr:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
   STDERR_PATH=$(sqlite3 data/agent_orchestrator.db \
     "SELECT stderr_path FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 1;")
   test -n "${STDERR_PATH}" && cat "${STDERR_PATH}"
   ```

### Expected

- The run exits non-zero.
- An event row exists with `event_type='sandbox_resource_exceeded'`.
- The payload includes `resource_kind` containing `open_files`.
- Stderr contains an error such as `Too many open files`.

---

## Scenario 2: Sandbox Emits sandbox_network_blocked for network_mode=deny

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure a sandboxed step cannot perform outbound network access when `network_mode=deny`.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-sandbox-network-deny"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-deny --name "sandbox network deny" --goal "sandbox network deny" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Verify the network event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
   ```

### Expected

- The run exits non-zero.
- An event row exists with `event_type='sandbox_network_blocked'`.
- The payload includes `reason` containing `network_blocked`.
- The payload includes a `stderr_excerpt` showing DNS or connection blocking.
- `network_target` is `example.com` when host extraction succeeds; if not, the event is still valid as long as the stderr excerpt is present.

---

## Scenario 3: Unsupported network_mode=allowlist Fails Fast With Structured Event

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- CLI built from latest source.
- Runtime initialized.

### Goal

Ensure the current backend rejects unsupported allowlist network profiles explicitly instead of silently running them.

### Steps

1. Apply the reusable sandbox execution fixture bundle:
   ```bash
   QA_PROJECT="qa-sandbox-network-allowlist-unsupported"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-allowlist --name "sandbox allowlist unsupported" --goal "sandbox allowlist unsupported" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Inspect the structured failure:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
   orchestrator task get "${TASK_ID}"
   ```

### Expected

- Task execution does not silently fall back to host or unrestricted sandbox networking.
- An event row exists with `event_type='sandbox_network_blocked'`.
- The payload includes `reason_code` or `reason` indicating `unsupported_backend_feature`.
- The task run reports a failed sandboxed execution.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Emits sandbox_resource_exceeded for max_open_files | ☐ | | | |
| 2 | Sandbox Emits sandbox_network_blocked for network_mode=deny | ☐ | | | |
| 3 | Unsupported network_mode=allowlist Fails Fast With Structured Event | ☐ | | | |
