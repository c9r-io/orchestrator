---
self_referential_safe: false
---

# Orchestrator - Control Plane Security

**Module**: orchestrator
**Scope**: Validate secure TCP daemon bootstrap, host-user client config generation, role-based RPC authorization, insecure TCP escape hatch, and control-plane audit persistence
**Scenarios**: 6
**Priority**: Critical

---

## Background

The control-plane security change makes `orchestratord --bind <addr>` secure by default. A secure TCP daemon now:

- Bootstraps a local CA and server certificate under `data/control-plane/pki/`
- Generates a host-user client certificate and kubeconfig-style client config under `~/.orchestrator/control-plane/`
- Requires a client certificate for TCP access
- Applies RPC role checks from `data/control-plane/policy.yaml`
- Persists decisions to `control_plane_audit`

For request-rate, concurrency, and stream-occupancy protections added after the initial security hardening, see `docs/qa/orchestrator/65-grpc-control-plane-protection.md`.

Related paths:

- `crates/daemon/src/control_plane.rs`
- `crates/cli/src/client.rs`
- `core/src/migration.rs`
- `core/src/db.rs`

---

## Database Schema Reference

### Table: control_plane_audit
| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER | Autoincrement primary key |
| created_at | TEXT | RFC3339 timestamp |
| transport | TEXT | `tcp` or future transport label |
| remote_addr | TEXT | Peer socket address when available |
| rpc | TEXT | RPC method name |
| subject_id | TEXT | Client identity from URI SAN |
| authn_result | TEXT | Authentication result |
| authz_result | TEXT | Authorization result |
| role | TEXT | Effective subject role when known |
| reason | TEXT | Failure/decision note |
| tls_fingerprint | TEXT | SHA256 fingerprint of peer certificate |
| rejection_stage | TEXT | Classification: `cert_validation_failed`, `subject_not_found`, `subject_disabled`, `role_insufficient`, or NULL for allowed |
| traffic_class | TEXT | Traffic bucket for protection enforcement (m0017) |
| limit_scope | TEXT | Whether subject-scoped or global limits produced the decision (m0017) |
| decision | TEXT | Final decision label from the rate limiter (m0017) |
| reason_code | TEXT | Stable machine-readable reason code (m0017) |
| peer_exe | TEXT | Executable path of the peer process — UDS only, forensic audit (m0024) |

### UDS Policy: `{data_dir}/control-plane/uds-policy.yaml`

Optional policy file that restricts the maximum role available to UDS callers.  When absent, all same-UID callers get implicit Admin.

```yaml
max_role: operator        # read_only | operator | admin (default: admin)
audit_all_reads: true     # record ReadOnly RPCs in audit (default: false)
```

---

## Scenario 1: Secure TCP Bootstrap Generates Server And Client Materials

### Preconditions
- Repository root: `$ORCHESTRATOR_ROOT`
- Release binaries built:
  ```bash
  cargo build --release -p orchestratord -p orchestrator-cli
  ```
- Use an isolated home directory for the scenario:
  ```bash
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  ```

### Goal
Verify `--bind` bootstraps PKI, policy, and the default local client config on first startup.

### Steps
1. Start the daemon in secure TCP mode:
   ```bash
   ./target/release/orchestratord --foreground --bind 127.0.0.1:50051 --workers 1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Verify server-side materials:
   ```bash
   ls -l data/control-plane/pki
   test -f data/control-plane/pki/ca.crt
   test -f data/control-plane/pki/ca.key
   test -f data/control-plane/pki/server.crt
   test -f data/control-plane/pki/server.key
   test -f data/control-plane/policy.yaml
   ```
3. Verify client-side materials:
   ```bash
   ls -l "$HOME/.orchestrator/control-plane"
   test -f "$HOME/.orchestrator/control-plane/config.yaml"
   test -f "$HOME/.orchestrator/control-plane/client.crt"
   test -f "$HOME/.orchestrator/control-plane/client.key"
   test -f "$HOME/.orchestrator/control-plane/ca.crt"
   ```
4. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- `data/control-plane/pki/` contains CA and server certificate/key files.
- `data/control-plane/policy.yaml` exists and contains a default local admin subject.
- `$HOME/.orchestrator/control-plane/config.yaml` exists without any manual setup.
- No manual certificate provisioning is required for the first local user.

### Expected Data State
```sql
SELECT COUNT(*) AS audit_rows FROM control_plane_audit;
-- Expected: 0 or more rows; bootstrap itself does not require task data.
```

---

## Scenario 2: CLI Auto-Discovery Uses Generated Secure Client Config

### Preconditions
- Scenario 1 completed in the same shell, keeping isolated `HOME`
- Secure daemon running:
  ```bash
  ./target/release/orchestratord --foreground --bind 127.0.0.1:50051 --workers 1 &
  DAEMON_PID=$!
  sleep 3
  ```

### Goal
Verify the CLI automatically discovers `~/.orchestrator/control-plane/config.yaml` and connects over secure TCP.

### Steps
1. Run the version command without `ORCHESTRATOR_SOCKET`:
   ```bash
   unset ORCHESTRATOR_SOCKET
   ./target/release/orchestrator version
   ```
2. Run a read-only RPC:
   ```bash
   ./target/release/orchestrator task list -o json
   ```
3. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- `orchestrator version` reports both client and daemon versions.
- `task list -o json` succeeds without requiring explicit certificate flags.
- No UDS socket is required for these commands.

### Expected Data State
```sql
SELECT rpc, authn_result, authz_result, role, rejection_stage
FROM control_plane_audit
WHERE rpc IN ('Ping', 'TaskList')
ORDER BY id DESC
LIMIT 4;
-- Expected: recent rows show authn_result='succeeded', authz_result='allowed', role='admin', rejection_stage=NULL
```

---

## Scenario 3: Additional Operator Client Is Denied On Admin RPC

### Preconditions
- Isolated `HOME` still active
- Secure daemon running on `127.0.0.1:50051`
- No other daemon is already listening on `127.0.0.1:50051`

### Goal
Verify `issue-client` can create an extra operator identity and that the operator cannot call an admin-only RPC.

### Steps
1. Verify the test port is owned by the active secure daemon only:
   ```bash
   lsof -nP -iTCP:50051 -sTCP:LISTEN
   ```
2. Issue an operator client to a second isolated home:
   ```bash
   export OP_HOME="$(mktemp -d)"
   ./target/release/orchestratord control-plane issue-client \
     --bind 127.0.0.1:50051 \
     --subject spiffe://orchestrator/local-user/operator-qa \
     --role operator \
     --home "$OP_HOME"
   ```
3. Confirm the issued client config exists:
   ```bash
   test -f "$OP_HOME/.orchestrator/control-plane/operator-qa/config.yaml"
   ```
4. Call a read-only RPC using the operator config:
   ```bash
   ./target/release/orchestrator \
     --control-plane-config "$OP_HOME/.orchestrator/control-plane/operator-qa/config.yaml" \
     task list -o json
   ```
5. Attempt an admin RPC with the operator config:
   ```bash
   ./target/release/orchestrator \
     --control-plane-config "$OP_HOME/.orchestrator/control-plane/operator-qa/config.yaml" \
     debug --component config
   ```

### Expected
- `issue-client` generates a second client bundle under the provided home path.
- `task list` succeeds for the operator client.
- `debug --component config` fails with a permission-denied style gRPC error because `ConfigDebug` is an admin RPC.

### Troubleshooting

| Symptom | Likely Cause | Action |
|--------|--------------|--------|
| `invalid peer certificate: UnknownIssuer` before any RPC result | The command connected to a stale daemon already bound to the test port, not the daemon that issued the client bundle | Stop the old daemon or switch to a fresh unused port, then re-run the scenario from the secure daemon bootstrap step |

### Expected Data State
```sql
SELECT rpc, subject_id, authn_result, authz_result, role, reason, rejection_stage
FROM control_plane_audit
WHERE subject_id = 'spiffe://orchestrator/local-user/operator-qa'
ORDER BY id DESC
LIMIT 5;
-- Expected: one allowed row for `TaskList` with rejection_stage=NULL and one denied row for `ConfigDebug` with rejection_stage='role_insufficient', both with role='operator'
```

---

## Scenario 4: Insecure TCP Feature Gate And Default Build Rejection

### Preconditions
- Release binaries built with `dev-insecure` feature:
  ```bash
  cargo build --release -p orchestratord --features dev-insecure
  ```
- A second release binary built without `dev-insecure` feature:
  ```bash
  cargo build --release -p orchestratord
  ```

### Goal
Verify insecure TCP only starts with `--insecure-bind` on a `dev-insecure` build, and the default build rejects the flag entirely.

### Steps
1. Start the daemon in insecure TCP mode (requires `dev-insecure` build):
   ```bash
   ./target/release/orchestratord --foreground --insecure-bind 127.0.0.1:50052 --workers 1 > /tmp/orchestrator-insecure.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Inspect the startup log:
   ```bash
   grep -n "insecure TCP control-plane enabled" /tmp/orchestrator-insecure.log
   ```
3. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```
4. Attempt to start the default build with `--insecure-bind`:
   ```bash
   ./target/release/orchestratord --insecure-bind 127.0.0.1:9999 2>&1
   ```

### Expected
- The `dev-insecure` daemon starts on TCP only when `--insecure-bind` is passed.
- Startup logs contain an explicit unsafe warning.
- The default build exits with a non-zero exit code and clap error text such as `unexpected argument '--insecure-bind'`.

---

## Scenario 5: UDS Fallback, mTLS Enforcement, And Audit Classification

### Preconditions
- Secure daemon running on `127.0.0.1:50051` (started via Scenario 1 steps)
- At least one denied RPC attempt has been performed (from Scenario 3)

### Goal
Verify UDS mode works without client certificates, mandatory mTLS rejects unauthenticated TCP connections, and audit `rejection_stage` is populated for denial events.

### Steps
1. Start the daemon without `--bind` for UDS verification:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Verify the UDS socket exists and CLI connects:
   ```bash
   test -S data/orchestrator.sock
   export ORCHESTRATOR_SOCKET="$(pwd)/data/orchestrator.sock"
   ./target/release/orchestrator version
   ```
3. Stop the UDS daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   unset ORCHESTRATOR_SOCKET
   ```
4. Start a secure TCP daemon and attempt a connection without a client certificate:
   ```bash
   ./target/release/orchestratord --foreground --bind 127.0.0.1:50051 --workers 1 &
   DAEMON_PID=$!
   sleep 3
   curl -k https://127.0.0.1:50051 2>&1
   ```
5. Query audit records for rejection_stage classification:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT rejection_stage, COUNT(*) FROM control_plane_audit GROUP BY rejection_stage;"
   sqlite3 data/agent_orchestrator.db \
     "SELECT rpc, rejection_stage, reason FROM control_plane_audit WHERE rejection_stage IS NOT NULL ORDER BY id DESC LIMIT 5;"
   ```
6. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- The daemon creates `data/orchestrator.sock` and CLI connects over UDS without TLS materials.
- `curl` without a client certificate reports a TLS handshake error; the connection never reaches the gRPC layer.
- Allowed requests have `rejection_stage = NULL`.
- Denied requests show one of: `cert_validation_failed`, `subject_not_found`, `subject_disabled`, `role_insufficient`.
- The `role_insufficient` stage corresponds to operator-calls-admin-RPC denials from Scenario 3.

---

## Scenario 6: UDS Trust Boundary Hardening And Audit Enrichment

### Preconditions
- Release binaries built
- Isolated home directory and data directory:
  ```bash
  export QA_HOME="$(mktemp -d)"
  export HOME="$QA_HOME"
  export QA_DATA="$(mktemp -d)"
  ```

### Goal
Verify the UDS trust boundary hardening: exhaustive RPC role mapping, effective role in audit, `peer_exe` resolution, `audit_all_reads` option, and startup permission/policy advisories.

### Steps
1. Set overly permissive data directory permissions and start a UDS daemon:
   ```bash
   chmod 0755 "$QA_DATA"
   ORCHESTRATORD_DATA_DIR="$QA_DATA" \
     ./target/release/orchestratord --foreground --workers 1 > /tmp/orch-uds.log 2>&1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Verify startup log contains data_dir permission warning and UDS policy advisory:
   ```bash
   grep "group/world-accessible permissions" /tmp/orch-uds.log
   grep "no uds-policy.yaml found" /tmp/orch-uds.log
   ```
3. Call a read-only RPC that was previously implicit Admin (now ReadOnly):
   ```bash
   export ORCHESTRATOR_SOCKET="$QA_DATA/orchestrator.sock"
   ./target/release/orchestrator db status
   ```
4. Stop and restart with a UDS policy restricting to operator with full audit:
   ```bash
   kill "$DAEMON_PID"; wait "$DAEMON_PID" 2>/dev/null
   mkdir -p "$QA_DATA/control-plane"
   cat > "$QA_DATA/control-plane/uds-policy.yaml" <<'YAML'
   max_role: operator
   audit_all_reads: true
   YAML
   ORCHESTRATORD_DATA_DIR="$QA_DATA" \
     ./target/release/orchestratord --foreground --workers 1 &
   DAEMON_PID=$!
   sleep 3
   ```
5. Call a ReadOnly RPC, an Operator RPC, and an Admin RPC:
   ```bash
   ./target/release/orchestrator db status          # ReadOnly — should succeed
   ./target/release/orchestrator task list -o json   # ReadOnly — should succeed
   ./target/release/orchestrator daemon stop         # Admin (Shutdown) — should be denied
   ```
6. Query audit records for enrichment:
   ```bash
   sqlite3 "$QA_DATA/agent_orchestrator.db" \
     "SELECT rpc, authz_result, role, peer_exe FROM control_plane_audit ORDER BY id DESC LIMIT 10;"
   ```
7. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"; wait "$DAEMON_PID" 2>/dev/null
   ```

### Expected
- Startup log shows WARN for data_dir permissions (`0755` has group/world read+execute).
- Startup log shows INFO advisory about absent `uds-policy.yaml` (first start only).
- `db status` succeeds under `max_role: operator` because `DbStatus` maps to `ReadOnly`.
- `daemon stop` (Shutdown) is denied because `Shutdown` maps to `Admin` and policy caps at `operator`.
- Audit records include `role = 'operator'` (effective role from policy) and non-NULL `peer_exe` (the CLI binary path).
- With `audit_all_reads: true`, even `DbStatus` and `TaskList` produce audit rows.

### Expected Data State
```sql
-- ReadOnly RPCs are audited because audit_all_reads is true
SELECT rpc, authz_result, role, peer_exe IS NOT NULL AS has_exe
FROM control_plane_audit
WHERE transport = 'uds'
ORDER BY id DESC
LIMIT 5;
-- Expected: Shutdown → denied (role=operator, rejection_stage=uds_policy_denied),
--           DbStatus/TaskList → allowed (role=operator, has_exe=1)
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Secure TCP Bootstrap Generates Server And Client Materials | ✅ | 2026-03-12 | Claude | PKI + policy + client materials all bootstrapped correctly |
| 2 | CLI Auto-Discovery Uses Generated Secure Client Config | ✅ | 2026-03-12 | Claude | version + task list succeed over auto-discovered secure TCP config |
| 3 | Additional Operator Client Is Denied On Admin RPC | ✅ | 2026-03-12 | Claude | Operator task list allowed; debug denied with "permission denied" |
| 4 | Insecure TCP Feature Gate And Default Build Rejection | ✅ | 2026-03-12 | Claude | dev-insecure build logs warning; default build rejects flag with exit code 2 |
| 5 | UDS Fallback, mTLS Enforcement, And Audit Classification | ✅ | 2026-03-12 | Claude | UDS works without TLS; curl rejected at handshake; audit rejection_stage populated correctly |
| 6 | UDS Trust Boundary Hardening And Audit Enrichment | ⬜ | | | |
