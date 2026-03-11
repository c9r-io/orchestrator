# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Scope**: Validate deterministic sandbox resource-limit enforcement, network blocking, Linux allowlist enforcement, and explicit macOS allowlist rejection
**Scenarios**: 4
**Priority**: High

---

## Background

Step-level execution isolation is treated as closed when these behaviors are reproducibly verifiable:

- Unix child processes enforce configured `ExecutionProfile` resource limits
- sandbox failures emit structured `sandbox_resource_exceeded` and `sandbox_network_blocked` events
- Linux `network_mode=allowlist` can allow one destination and block another deterministically
- macOS `network_mode=allowlist` fails fast as an explicit unsupported-backend condition instead of silently degrading

The fixture bundle uses `orchestrator debug sandbox-probe ...` commands so QA can trigger stable resource and network outcomes without depending on ad-hoc shell or Python behavior.

Related docs:

- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`
- `scripts/qa/test-fr001-sandbox-matrix.sh`

Entry point: `orchestrator`

---

## Common Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available for scenarios 1-3.
- Linux environment with daemon running as `root`, and `ip` + `nft` available for scenario 4.
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
TASK_CREATE_OUTPUT=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-open-files-limit --name "sandbox fd limit" --goal "sandbox fd limit" --no-start)
TASK_ID=$(printf '%s\n' "${TASK_CREATE_OUTPUT}" | grep -oE '[0-9a-f-]{36}' | tail -1)
orchestrator task start "${TASK_ID}" || true
for _ in $(seq 1 30); do
  TASK_INFO_OUTPUT=$(orchestrator task info "${TASK_ID}")
  case "${TASK_INFO_OUTPUT}" in
    *"Status: completed"*|*"Status: failed"*) break ;;
  esac
  sleep 1
done
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_resource_exceeded' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The step emits `sandbox_resource_exceeded`.
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
TASK_CREATE_OUTPUT=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-deny --name "sandbox network deny" --goal "sandbox network deny" --no-start)
TASK_ID=$(printf '%s\n' "${TASK_CREATE_OUTPUT}" | grep -oE '[0-9a-f-]{36}' | tail -1)
orchestrator task start "${TASK_ID}" || true
for _ in $(seq 1 30); do
  TASK_INFO_OUTPUT=$(orchestrator task info "${TASK_ID}")
  case "${TASK_INFO_OUTPUT}" in
    *"Status: completed"*|*"Status: failed"*) break ;;
  esac
  sleep 1
done
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- The step emits `sandbox_network_blocked`.
- The latest event is `sandbox_network_blocked`.
- Payload contains `reason_code=network_blocked`.
- Payload contains `stderr_excerpt`.
- `network_target` is best-effort; `example.com` is preferred but not required.

## Scenario 3: Unsupported network_mode=allowlist Fails Fast With Structured Event

### Goal

Ensure the current backend rejects `network_mode=allowlist` explicitly instead of silently running it.

### Steps

```bash
TASK_CREATE_OUTPUT=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-network-allowlist --name "sandbox allowlist unsupported" --goal "sandbox allowlist unsupported" --no-start)
TASK_ID=$(printf '%s\n' "${TASK_CREATE_OUTPUT}" | grep -oE '[0-9a-f-]{36}' | tail -1)
orchestrator task start "${TASK_ID}" || true
for _ in $(seq 1 30); do
  TASK_INFO_OUTPUT=$(orchestrator task info "${TASK_ID}")
  case "${TASK_INFO_OUTPUT}" in
    *"Status: completed"*|*"Status: failed"*) break ;;
  esac
  sleep 1
done
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
orchestrator task get "${TASK_ID}"
```

### Expected

- Task execution does not silently fall back to host or unrestricted sandbox networking.
- The latest event is `sandbox_network_blocked`.
- Payload contains `reason_code=unsupported_backend_feature`.
- The task run reports a failed sandboxed execution.

## Scenario 4: Linux Allowlist Allows One TCP Target And Blocks Another

### Goal

Ensure Linux `linux_native` enforces a real allowlist boundary.

### Steps

```bash
READY_FILE="$(mktemp)"
./target/release/orchestrator debug sandbox-probe tcp-serve --bind 0.0.0.0 --port 18080 --ready-file "${READY_FILE}" &
SERVER_PID=$!
trap 'kill "${SERVER_PID}" 2>/dev/null || true; rm -f "${READY_FILE}"' EXIT
while [ ! -s "${READY_FILE}" ]; do sleep 1; done

cat >/tmp/sandbox-allowlist-linux.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_network_allowlist_linux
spec:
  mode: sandbox
  fs_mode: inherit
  network_mode: allowlist
  network_allowlist:
    - 10.203.0.1:18080
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: net-allow
spec:
  capabilities: [sandbox_allow_linux]
  command: "exec ./target/release/orchestrator debug sandbox-probe tcp-connect --host 10.203.0.1 --port 18080"
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: net-block
spec:
  capabilities: [sandbox_block_linux]
  command: "exec ./target/release/orchestrator debug sandbox-probe tcp-connect --host 10.203.0.1 --port 18081"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: sandbox-network-allowlist-linux-allow
spec:
  steps:
    - id: implement
      type: implement
      required_capability: sandbox_allow_linux
      execution_profile: sandbox_network_allowlist_linux
      enabled: true
      scope: task
  loop:
    mode: once
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: sandbox-network-allowlist-linux-block
spec:
  steps:
    - id: implement
      type: implement
      required_capability: sandbox_block_linux
      execution_profile: sandbox_network_allowlist_linux
      enabled: true
      scope: task
  loop:
    mode: once
YAML

orchestrator apply --project "${QA_PROJECT}" -f /tmp/sandbox-allowlist-linux.yaml
ALLOW_WORKFLOW_ID="sandbox-network-allowlist-linux-allow"
BLOCK_WORKFLOW_ID="sandbox-network-allowlist-linux-block"
ALLOW_TASK_CREATE_OUTPUT=$(orchestrator task create --project "${QA_PROJECT}" --workflow "${ALLOW_WORKFLOW_ID}" --name "sandbox allow allow" --goal "sandbox allow allow" --no-start)
ALLOW_TASK_ID=$(printf '%s\n' "${ALLOW_TASK_CREATE_OUTPUT}" | grep -oE '[0-9a-f-]{36}' | tail -1)
BLOCK_TASK_CREATE_OUTPUT=$(orchestrator task create --project "${QA_PROJECT}" --workflow "${BLOCK_WORKFLOW_ID}" --name "sandbox allow block" --goal "sandbox allow block" --no-start)
BLOCK_TASK_ID=$(printf '%s\n' "${BLOCK_TASK_CREATE_OUTPUT}" | grep -oE '[0-9a-f-]{36}' | tail -1)
orchestrator task start "${ALLOW_TASK_ID}"
orchestrator task start "${BLOCK_TASK_ID}" || true
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events WHERE task_id='${BLOCK_TASK_ID}' AND event_type='sandbox_network_blocked' ORDER BY created_at DESC LIMIT 1;"
```

### Expected

- `ALLOW_TASK_ID` completes without a sandbox network event.
- `BLOCK_TASK_ID` emits `sandbox_network_blocked`.
- The blocked payload contains `reason_code=network_allowlist_blocked`.
- The blocked payload contains `network_target=10.203.0.1:18081` or an equivalent best-effort target.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Emits sandbox_resource_exceeded for max_open_files | PASS | 2026-03-11 | codex | macOS verified: `reason_code=open_files_limit_exceeded`, `resource_kind=open_files` |
| 2 | Sandbox Emits sandbox_network_blocked for network_mode=deny | PASS | 2026-03-11 | codex | macOS verified: `reason_code=network_blocked`, `network_target=example.com` |
| 3 | Unsupported network_mode=allowlist Fails Fast With Structured Event | PASS | 2026-03-11 | codex | macOS verified: `reason_code=unsupported_backend_feature`, `backend=macos_seatbelt`; CLI broken-pipe defect filed separately |
| 4 | Linux Allowlist Allows One TCP Target And Blocks Another | NOT RUN | 2026-03-11 | codex | Requires Linux `root` host with `ip` and `nft`; unavailable on this macOS environment |
