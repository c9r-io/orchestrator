# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Scope**: Validate deterministic sandbox resource-limit enforcement, network blocking, and explicit allowlist rejection on the active macOS backend
**Scenarios**: 3
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
cargo build --release -p orchestratord -p orchestrator-cli
kill "$(cat data/daemon.pid 2>/dev/null)" 2>/dev/null || true
nohup ./target/release/orchestratord --foreground --workers 2 >/tmp/orchestratord-fr001.log 2>&1 &

QA_PROJECT="${QA_PROJECT:-qa-fr001-sandbox}"
orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
```

The sandbox scenarios run through the daemon, not an in-process CLI path. If backend sandbox code changed since the daemon was started, rebuild and restart it before testing or the run can report stale behavior.

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

### Troubleshooting

| Symptom | Likely Cause | Action |
|---|---|---|
| `SANDBOX_PROBE resource=open_files ...` appears in stderr, but no `sandbox_resource_exceeded` event is persisted | QA hit an older daemon binary that predates the sandbox event-classification change | Rebuild `orchestratord` and `orchestrator-cli`, restart the daemon, then rerun the scenario |

## Scenario 2: Sandbox Emits sandbox_network_blocked for network_mode=deny

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

## Scenario 3: Unsupported network_mode=allowlist Fails Fast With Structured Event

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
| 2 | Sandbox Emits sandbox_network_blocked for network_mode=deny | ☐ | | | |
| 3 | Unsupported network_mode=allowlist Fails Fast With Structured Event | ☐ | | | |
