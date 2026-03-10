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

1. Apply an isolated project with a low file-descriptor limit:
   ```bash
   QA_PROJECT="qa-sandbox-open-files-limit"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   cat > /tmp/sandbox-open-files-limit.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: ExecutionProfile
   metadata:
     name: sandbox_fd_limit
   spec:
     mode: sandbox
     fs_mode: workspace_readonly
     network_mode: deny
     max_open_files: 16
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
     name: fd-burner
   spec:
     capabilities: [implement]
     command: "python3 -c \"files=[]; [files.append(open('/dev/null','rb')) for _ in range(256)]; print('{\\\"confidence\\\":0.9,\\\"quality_score\\\":0.9,\\\"artifacts\\\":[]}')\""
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: sandbox-open-files-limit
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: sandbox_fd_limit
         enabled: true
         scope: task
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-open-files-limit.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "sandbox fd limit" --goal "sandbox fd limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
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

1. Apply an isolated project with a network probe agent:
   ```bash
   QA_PROJECT="qa-sandbox-network-deny"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   cat > /tmp/sandbox-network-deny.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: ExecutionProfile
   metadata:
     name: sandbox_network_deny
   spec:
     mode: sandbox
     fs_mode: workspace_readonly
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
     name: net-probe
   spec:
     capabilities: [implement]
     command: |
       python3 -c "import socket; socket.getaddrinfo('example.com', 443)"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: sandbox-network-deny
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: sandbox_network_deny
         enabled: true
         scope: task
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-network-deny.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "sandbox network deny" --goal "sandbox network deny" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
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

1. Apply an isolated project with an allowlist profile and a simple agent:
   ```bash
   QA_PROJECT="qa-sandbox-network-allowlist-unsupported"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   cat > /tmp/sandbox-network-allowlist.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: ExecutionProfile
   metadata:
     name: sandbox_network_allowlist
   spec:
     mode: sandbox
     fs_mode: workspace_readonly
     network_mode: allowlist
     network_allowlist:
       - example.com:443
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
     name: noop-agent
   spec:
     capabilities: [implement]
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: sandbox-network-allowlist
   spec:
     steps:
       - id: implement
         type: implement
         required_capability: implement
         execution_profile: sandbox_network_allowlist
         enabled: true
         scope: task
     loop:
       mode: once
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-network-allowlist.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --name "sandbox allowlist unsupported" --goal "sandbox allowlist unsupported" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
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
