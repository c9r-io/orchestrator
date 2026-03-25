---
self_referential_safe: false
---

# Orchestrator - gRPC Control Plane Protection

**Module**: orchestrator
**Scope**: Validate gRPC control-plane protection config bootstrap, secure-TCP subject rate limiting, stream occupancy limits, UDS fallback protection, and repeatable pressure validation
**Scenarios**: 5
**Priority**: High

---

## Background

The gRPC control plane now applies daemon-side protection budgets in addition to transport security and authorization. The daemon bootstraps `data/control-plane/protection.yaml`, classifies RPCs into `read`, `write`, `stream`, and `admin`, and records rejection decisions in `control_plane_audit`.

Related paths:

- `crates/daemon/src/protection.rs`
- `crates/daemon/src/main.rs`
- `crates/daemon/src/control_plane.rs`
- `core/src/db.rs`
- `scripts/qa/test-fr013-control-plane-protection.sh`

---

## Database Schema Reference

### Table: control_plane_audit
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER | Autoincrement primary key |
| created_at | TEXT | RFC3339 timestamp |
| transport | TEXT | `tcp` or `uds` |
| remote_addr | TEXT | Peer address when available |
| rpc | TEXT | RPC method name |
| subject_id | TEXT | mTLS URI SAN subject for secure TCP |
| authn_result | TEXT | Authentication result or `skipped` |
| authz_result | TEXT | Authorization result or `skipped` |
| role | TEXT | Effective role when known |
| reason | TEXT | Human-readable decision note |
| rejection_stage | TEXT | Auth rejection classifier when applicable |
| traffic_class | TEXT | `read`, `write`, `stream`, or `admin` |
| limit_scope | TEXT | `subject` or `global` |
| decision | TEXT | `rejected` for protection denials |
| reason_code | TEXT | `rate_limited`, `concurrency_limited`, `stream_limit_exceeded`, or `load_shed` |

---

## Scenario 1: Secure TCP Bootstrap Generates Protection Config With Defaults

### Preconditions
- Repository root: `$ORCHESTRATOR_ROOT`
- Release binaries built:
  ```bash
  cargo build --release -p orchestratord -p orchestrator-cli
  ```
- Isolated app root and home:
  ```bash
  export QA_ROOT="$(mktemp -d)"
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  ```

### Goal
Verify secure TCP startup bootstraps `protection.yaml` alongside the existing control-plane security materials.

### Steps
1. Start the secure daemon from the isolated app root:
   ```bash
   cd "$QA_ROOT"
   $ORCHESTRATOR_ROOT/target/release/orchestratord --foreground --bind 127.0.0.1:51051 --workers 1 > daemon.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Verify server-side control-plane files:
   ```bash
   test -f data/control-plane/policy.yaml
   test -f data/control-plane/protection.yaml
   sed -n '1,200p' data/control-plane/protection.yaml
   ```
3. Verify the secure client bundle exists:
   ```bash
   test -f "$HOME/.orchestrator/control-plane/config.yaml"
   test -f "$HOME/.orchestrator/control-plane/client.crt"
   ```
4. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

### Expected
- `data/control-plane/protection.yaml` is created automatically on first daemon start.
- The generated file contains top-level `defaults`, `global`, and `overrides` sections.
- Secure TCP bootstrap still generates the client bundle under `$HOME/.orchestrator/control-plane/`.

### Expected Data State
```sql
SELECT COUNT(*) AS audit_rows FROM control_plane_audit;
-- Expected: 0 or more rows. Protection config bootstrap does not require pre-existing task data.
```

---

## Scenario 2: Secure TCP Applies Subject Read Rate Limit And Audits Rejection

### Preconditions
- Fresh isolated app root and home:
  ```bash
  export QA_ROOT="$(mktemp -d)"
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  mkdir -p "$QA_ROOT/data/control-plane"
  cat > "$QA_ROOT/data/control-plane/protection.yaml" <<'YAML'
  defaults:
    read: { rate_per_sec: 1, burst: 1, max_in_flight: 8 }
    write: { rate_per_sec: 5, burst: 5, max_in_flight: 8 }
    stream: { rate_per_sec: 5, burst: 5, max_active_streams: 2 }
    admin: { rate_per_sec: 2, burst: 2, max_in_flight: 1 }
  global:
    read: { rate_per_sec: 10, burst: 10, max_in_flight: 32 }
    write: { rate_per_sec: 20, burst: 20, max_in_flight: 32 }
    stream: { rate_per_sec: 10, burst: 10, max_active_streams: 8 }
    admin: { rate_per_sec: 5, burst: 5, max_in_flight: 4 }
  overrides:
    TaskList:
      class: read
      subject: { rate_per_sec: 1, burst: 1 }
  YAML
  ```

### Goal
Verify two rapid `TaskList` calls from the same secure client hit the subject-scoped read budget and persist a structured audit row.

### Steps
1. Start the secure daemon:
   ```bash
   cd "$QA_ROOT"
   $ORCHESTRATOR_ROOT/target/release/orchestratord --foreground --bind 127.0.0.1:51052 --workers 1 > daemon.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Execute the first read call:
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task list -o json
   ```
3. Immediately execute the second read call:
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task list -o json 2>&1 | tee second-read.log
   ```
4. Inspect the latest audit rows:
   ```bash
   sqlite3 "$QA_ROOT/data/agent_orchestrator.db" \
     "SELECT rpc, transport, subject_id, traffic_class, limit_scope, decision, reason_code FROM control_plane_audit ORDER BY id DESC LIMIT 5;"
   ```
5. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

### Expected
- The first `task list` succeeds.
- The second `task list` fails with a gRPC `RESOURCE_EXHAUSTED` style error containing `reason_code=rate_limited`.
- Audit rows include `rpc='TaskList'`, `traffic_class='read'`, `limit_scope='subject'`, `decision='rejected'`, and `reason_code='rate_limited'`.

### Expected Data State
```sql
SELECT rpc, traffic_class, limit_scope, decision, reason_code
FROM control_plane_audit
WHERE rpc = 'TaskList'
ORDER BY id DESC
LIMIT 2;
-- Expected: latest row contains read/subject/rejected/rate_limited.
```

---

## Scenario 3: Secure TCP Enforces Active Stream Limit For TaskWatch

### Preconditions
- Fresh isolated app root and home:
  ```bash
  export QA_ROOT="$(mktemp -d)"
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  mkdir -p "$QA_ROOT/data/control-plane"
  cat > "$QA_ROOT/data/control-plane/protection.yaml" <<'YAML'
  defaults:
    read: { rate_per_sec: 20, burst: 20, max_in_flight: 32 }
    write: { rate_per_sec: 5, burst: 5, max_in_flight: 8 }
    stream: { rate_per_sec: 5, burst: 5, max_active_streams: 1 }
    admin: { rate_per_sec: 2, burst: 2, max_in_flight: 1 }
  global:
    read: { rate_per_sec: 50, burst: 50, max_in_flight: 64 }
    write: { rate_per_sec: 20, burst: 20, max_in_flight: 32 }
    stream: { rate_per_sec: 10, burst: 10, max_active_streams: 1 }
    admin: { rate_per_sec: 5, burst: 5, max_in_flight: 4 }
  overrides:
    TaskWatch:
      class: stream
      subject: { max_active_streams: 1 }
      global: { max_active_streams: 1 }
  YAML
  ```
- Create the mock QA target required by the fixture workspace:
  ```bash
  mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
  cat > "$QA_ROOT/fixtures/qa/watch-hold.md" <<'MD'
  # Watch Hold
  MD
  ```

### Goal
Verify `TaskWatch` consumes a stream permit until disconnect and the second concurrent watch is rejected.

### Steps
1. Start the secure daemon:
   ```bash
   cd "$QA_ROOT"
   $ORCHESTRATOR_ROOT/target/release/orchestratord --foreground --bind 127.0.0.1:51053 --workers 1 > daemon.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Apply the mock fixture into the isolated daemon:
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator apply \
     -f $ORCHESTRATOR_ROOT/fixtures/manifests/bundles/pause-resume-workflow.yaml \
     --project qa-protect-watch
   ```
3. Create a pending task:
   ```bash
   TASK_ID=$($ORCHESTRATOR_ROOT/target/release/orchestrator task create \
     --project qa-protect-watch \
     --workflow qa_sleep \
     --name "watch-hold" \
     --goal "hold watch stream" \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   echo "$TASK_ID"
   ```
4. Open the first watch in the background (with timeout to prevent stalling):
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task watch "$TASK_ID" --interval 1 --timeout 30 > first-watch.log 2>&1 &
   WATCH_PID=$!
   sleep 2
   ```
5. Attempt a second watch from the same client (with timeout):
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task watch "$TASK_ID" --interval 1 --timeout 10 2>&1 | tee second-watch.log || true
   ```
6. Query audit rows, then clean up:
   ```bash
   sqlite3 "$QA_ROOT/data/agent_orchestrator.db" \
     "SELECT rpc, traffic_class, limit_scope, decision, reason_code FROM control_plane_audit WHERE rpc = 'TaskWatch' ORDER BY id DESC LIMIT 5;"
   kill "$WATCH_PID" 2>/dev/null || true
   kill "$DAEMON_PID"
   wait "$WATCH_PID" 2>/dev/null || true
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

### Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Both watches succeed; audit rows show empty `traffic_class`/`decision`/`reason_code` | `protection.yaml` not placed at `$QA_ROOT/data/control-plane/protection.yaml` or uses default limits (`max_active_streams: 2`) | Verify the protection.yaml file exists and contains `max_active_streams: 1` for the TaskWatch override |
| Second watch hangs instead of returning `RESOURCE_EXHAUSTED` | Missing `--timeout` flag on streaming commands | Always use `--timeout` flag on `task watch` in non-interactive contexts |
| First watch exits before second watch starts | `sleep 2` too short or daemon slow to respond | Increase sleep to 3–5 seconds; verify first watch PID is still running before step 5 |

### Expected
- The first `task watch` stays open.
- The second `task watch` fails with a gRPC `RESOURCE_EXHAUSTED` style error containing `reason_code=stream_limit_exceeded`.

---

## Scenario 4: UDS Mode Still Applies Protection Using Local Fallback Identity

### Preconditions
- Fresh isolated app root and home:
  ```bash
  export QA_ROOT="$(mktemp -d)"
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  mkdir -p "$QA_ROOT/data/control-plane"
  cat > "$QA_ROOT/data/control-plane/protection.yaml" <<'YAML'
  defaults:
    read: { rate_per_sec: 1, burst: 1, max_in_flight: 8 }
    write: { rate_per_sec: 5, burst: 5, max_in_flight: 8 }
    stream: { rate_per_sec: 5, burst: 5, max_active_streams: 2 }
    admin: { rate_per_sec: 2, burst: 2, max_in_flight: 1 }
  global:
    read: { rate_per_sec: 1, burst: 1, max_in_flight: 8 }
    write: { rate_per_sec: 20, burst: 20, max_in_flight: 32 }
    stream: { rate_per_sec: 10, burst: 10, max_active_streams: 8 }
    admin: { rate_per_sec: 5, burst: 5, max_in_flight: 4 }
  overrides:
    TaskList:
      class: read
      global: { rate_per_sec: 1, burst: 1 }
  YAML
  ```

### Goal
Verify UDS mode is not exempt from protection and falls back to a non-subject identity without requiring certificates.

### Steps
1. Start the daemon in default UDS mode:
   ```bash
   cd "$QA_ROOT"
   $ORCHESTRATOR_ROOT/target/release/orchestratord --foreground --workers 1 > daemon.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Execute the first UDS read:
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task list -o json
   ```
3. Immediately execute the second UDS read:
   ```bash
   $ORCHESTRATOR_ROOT/target/release/orchestrator task list -o json 2>&1 | tee uds-second-read.log
   ```
4. Inspect audit rows:
   ```bash
   sqlite3 "$QA_ROOT/data/agent_orchestrator.db" \
     "SELECT transport, remote_addr, subject_id, rpc, traffic_class, limit_scope, decision, reason_code FROM control_plane_audit WHERE rpc = 'TaskList' ORDER BY id DESC LIMIT 5;"
   ```
5. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null || true
   ```

### Expected
- The first UDS `task list` succeeds without client certificates.
- The second UDS `task list` fails with `reason_code=rate_limited`.
- Audit rows use `transport='uds'`, `subject_id IS NULL`, and still record the protection rejection fields.
- The local fallback identity is still tracked through the subject-scoped budget, so `limit_scope='subject'` is acceptable in UDS mode.

### Expected Data State
```sql
SELECT transport, subject_id, traffic_class, limit_scope, decision, reason_code
FROM control_plane_audit
WHERE rpc = 'TaskList'
ORDER BY id DESC
LIMIT 2;
-- Expected: latest row contains uds/NULL/read/subject/rejected/rate_limited.
```

---

## Scenario 5: Secure TCP Pressure Script Rejects Fast And Preserves Daemon Availability

### Preconditions
- Repository root: `$ORCHESTRATOR_ROOT`
- Release binaries built:
  ```bash
  cargo build --release -p orchestratord -p orchestrator-cli
  ```

### Goal
Verify the middleware-based protection stack rejects excess `TaskList`, `TaskWatch`, and `Apply` traffic under repeated concurrent pressure without crashing the daemon.

### Steps
1. Run the pressure script:
   ```bash
   cd $ORCHESTRATOR_ROOT
   scripts/qa/test-fr013-control-plane-protection.sh
   ```
2. Observe the emitted audit sample at the end of the script.

### Expected
- The script exits `0`.
- The script records at least one rejected `TaskList` row with `reason_code='rate_limited'`.
- The script records at least one rejected `TaskWatch` row with `reason_code='stream_limit_exceeded'`.
- The script records at least one rejected `Apply` row with `reason_code='rate_limited'` or `reason_code='concurrency_limited'`.
- The final daemon health probe (`orchestrator debug`) succeeds, proving non-exhausted traffic can still reach the control plane.

### Expected Data State
```sql
SELECT rpc, traffic_class, limit_scope, decision, reason_code
FROM control_plane_audit
WHERE rpc IN ('TaskList', 'TaskWatch', 'Apply')
ORDER BY id DESC
LIMIT 20;
-- Expected: rejected rows are present for all three RPC groups with stable reason_code values.
```
## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Secure TCP Bootstrap Generates Protection Config With Defaults | PASS | 2026-03-12 | codex | Verified isolated secure startup generates `protection.yaml` and secure client bundle. |
| 2 | Secure TCP Applies Subject Read Rate Limit And Audits Rejection | PASS | 2026-03-12 | codex | Second `task list` returned `RESOURCE_EXHAUSTED` with `reason_code=rate_limited`; audit row carried `read/subject/rejected/rate_limited`. |
| 3 | Secure TCP Enforces Active Stream Limit For TaskWatch | PASS | 2026-03-12 | codex | Added explicit mock QA target precondition; second concurrent `task watch` returned `stream_limit_exceeded` and audit row recorded `stream/rejected`. |
| 4 | UDS Mode Still Applies Protection Using Local Fallback Identity | PASS | 2026-03-12 | codex | UDS path rate-limited without certificates; audit row showed `transport=uds`, empty `subject_id`, and `limit_scope=subject`. |
| 5 | Secure TCP Pressure Script Rejects Fast And Preserves Daemon Availability | PASS | 2026-03-12 | codex | `scripts/qa/test-fr013-control-plane-protection.sh` returned `0`; `TaskList` / `TaskWatch` / `Apply` all produced rejected audit rows while `orchestrator debug` still succeeded. |
