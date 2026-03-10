# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Scope**: Validate deterministic sandbox resource-limit enforcement, network blocking, and explicit allowlist rejection on the active macOS backend
**Scenarios**: 6
**Priority**: High

---

## Background

Step-level execution isolation is treated as closed on the active macOS backend when these behaviors are reproducibly verifiable:

- Unix child processes enforce configured `ExecutionProfile` resource limits
- sandbox failures emit structured `sandbox_resource_exceeded` and `sandbox_network_blocked` events
- `network_mode=allowlist` fails fast as an explicit unsupported-backend condition instead of silently degrading

The fixture bundle uses `orchestrator debug sandbox-probe ...` commands so QA can trigger stable resource and network outcomes without depending on ad-hoc shell or Python behavior.

Related docs:

- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`
- `scripts/qa/test-fr001-sandbox-matrix.sh`

Entry point: `orchestrator`

---

## Common Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- CLI built from latest source.
- Runtime initialized.

Common setup:

```bash
QA_PROJECT="${QA_PROJECT:-qa-fr001-sandbox}"
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
```

## Scenario 1: Sandbox Emits sandbox_resource_exceeded for max_open_files

### Goal

Ensure a sandboxed step with a low `max_open_files` limit fails deterministically.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-open-files-limit --name "sandbox fd limit" --goal "sandbox fd limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The run exits non-zero.
- The latest event is `sandbox_resource_exceeded`.
- Payload contains `reason_code=open_files_limit_exceeded`.
- Payload contains `resource_kind=open_files`.

## Scenario 2: Sandbox Emits sandbox_resource_exceeded for max_cpu_seconds

### Goal

Ensure a sandboxed CPU burn is terminated by the configured CPU limit.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-cpu-limit --name "sandbox cpu limit" --goal "sandbox cpu limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The run exits non-zero.
- The latest event is `sandbox_resource_exceeded`.
- Payload contains `reason_code=cpu_limit_exceeded`.
- Payload contains `resource_kind=cpu`.

## Scenario 3: Sandbox Emits sandbox_resource_exceeded for max_memory_mb

### Goal

Ensure a sandboxed memory allocation probe fails under the configured memory limit.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-memory-limit --name "sandbox memory limit" --goal "sandbox memory limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The run exits non-zero.
- The latest event is `sandbox_resource_exceeded`.
- Payload contains `reason_code=memory_limit_exceeded`.
- Payload contains `resource_kind=memory`.

## Scenario 4: Sandbox Emits sandbox_resource_exceeded for max_processes

### Goal

Ensure a sandboxed process-spawn probe fails under the configured process limit.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-process-limit --name "sandbox process limit" --goal "sandbox process limit" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The run exits non-zero.
- The latest event is `sandbox_resource_exceeded`.
- Payload contains `reason_code=processes_limit_exceeded`.
- Payload contains `resource_kind=processes`.

## Scenario 5: Sandbox Emits sandbox_network_blocked for network_mode=deny

### Goal

Ensure a sandboxed step cannot perform outbound network access when `network_mode=deny`.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-deny --name "sandbox network deny" --goal "sandbox network deny" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The run exits non-zero.
- The latest event is `sandbox_network_blocked`.
- Payload contains `reason_code=network_blocked`.
- Payload contains `stderr_excerpt`.
- `network_target` is best-effort; `example.com` is preferred but not required.

## Scenario 6: Unsupported network_mode=allowlist Fails Fast With Structured Event

### Goal

Ensure the current backend rejects `network_mode=allowlist` explicitly instead of silently running it.

### Steps

```bash
TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-allowlist --name "sandbox allowlist unsupported" --goal "sandbox allowlist unsupported" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
orchestrator task start "${TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
orchestrator task get "${TASK_ID}"
```

### Expected

- Task execution does not silently fall back to host or unrestricted sandbox networking.
- The latest event is `sandbox_network_blocked`.
- Payload contains `reason_code=unsupported_backend_feature`.
- The task run reports a failed sandboxed execution.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Emits sandbox_resource_exceeded for max_open_files | ☐ | | | |
| 2 | Sandbox Emits sandbox_resource_exceeded for max_cpu_seconds | ☐ | | | |
| 3 | Sandbox Emits sandbox_resource_exceeded for max_memory_mb | ☐ | | | |
| 4 | Sandbox Emits sandbox_resource_exceeded for max_processes | ☐ | | | |
| 5 | Sandbox Emits sandbox_network_blocked for network_mode=deny | ☐ | | | |
| 6 | Unsupported network_mode=allowlist Fails Fast With Structured Event | ☐ | | | |
