---
self_referential_safe: false
---

# Orchestrator - Sandbox Resource Limits Extended

**Module**: orchestrator
**Scope**: Validate deterministic sandbox resource-limit enforcement for CPU, memory, and processes on the active macOS backend
**Scenarios**: 3
**Priority**: High

---

## Background

Step-level execution isolation is treated as closed on the active macOS backend when these behaviors are reproducibly verifiable:

- Unix child processes enforce configured `ExecutionProfile` resource limits
- sandbox failures emit structured `sandbox_resource_exceeded` events

Related docs:

- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
- `scripts/qa/test-fr001-sandbox-matrix.sh`

Entry point: `orchestrator`

---

## Common Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- CLI built from latest source.
- Runtime initialized.

Common setup:

```bash
cargo build --release -p orchestratord -p orchestrator-cli
kill "$(cat data/daemon.pid 2>/dev/null)" 2>/dev/null || true
nohup ./target/release/orchestratord --foreground --workers 2 >/tmp/orchestratord-fr001.log 2>&1 &

QA_PROJECT="${QA_PROJECT:-qa-fr001-sandbox}"
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
```

The sandbox scenarios run through the daemon, not an in-process CLI path. If backend sandbox code changed since the daemon was started, rebuild and restart it before testing or the run can report stale behavior.

## Scenario 1: Sandbox Emits sandbox_resource_exceeded for max_cpu_seconds

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

### Troubleshooting

| Symptom | Likely Cause | Action |
|---|---|---|
| CPU probe runs but no `sandbox_resource_exceeded` event is persisted | QA hit an older daemon binary that predates the sandbox event-classification change | Rebuild `orchestratord` and `orchestrator-cli`, restart the daemon, then rerun the scenario |
| CPU-bound task exits for a generic runner failure without `resource_kind=cpu` | The wrong workflow or execution profile was selected | Re-apply `sandbox-execution-profiles.yaml`, then confirm the workflow id `sandbox-cpu-limit` is used verbatim |

## Scenario 2: Sandbox Emits sandbox_resource_exceeded for max_memory_mb

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

### Troubleshooting

| Symptom | Likely Cause | Action |
|---|---|---|
| Memory probe fails but no `sandbox_resource_exceeded` event is recorded | Daemon was not restarted after backend sandbox changes | Rebuild binaries, restart the daemon, and rerun the scenario |
| Event is emitted with the wrong `resource_kind` | QA executed the wrong fixture workflow | Re-apply the fixture bundle and rerun with workflow id `sandbox-memory-limit` |

## Scenario 3: Sandbox Emits sandbox_resource_exceeded for max_processes

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

### Troubleshooting

| Symptom | Likely Cause | Action |
|---|---|---|
| Process-spawn probe completes without limit enforcement | Task ran against a stale daemon or a non-sandboxed execution profile | Rebuild/restart the daemon and confirm the task uses `sandbox-process-limit` |
| The task fails, but the event stream lacks `sandbox_resource_exceeded` | Probe stderr was generated by an outdated backend without structured classification | Restart the daemon from the freshly built binaries and rerun the scenario |

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Emits sandbox_resource_exceeded for max_cpu_seconds | ☐ | | | |
| 2 | Sandbox Emits sandbox_resource_exceeded for max_memory_mb | ☐ | | | |
| 3 | Sandbox Emits sandbox_resource_exceeded for max_processes | ☐ | | | |
