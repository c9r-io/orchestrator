# Orchestrator - Control Plane Security

**Module**: orchestrator
**Scope**: Validate secure TCP daemon bootstrap, host-user client config generation, role-based RPC authorization, insecure TCP escape hatch, and control-plane audit persistence
**Scenarios**: 5
**Priority**: Critical

---

## Background

The control-plane security change makes `orchestratord --bind <addr>` secure by default. A secure TCP daemon now:

- Bootstraps a local CA and server certificate under `data/control-plane/pki/`
- Generates a host-user client certificate and kubeconfig-style client config under `~/.orchestrator/control-plane/`
- Requires a client certificate for TCP access
- Applies RPC role checks from `data/control-plane/policy.yaml`
- Persists decisions to `control_plane_audit`

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

---

## Scenario 1: Secure TCP Bootstrap Generates Server And Client Materials

### Preconditions
- Repository root: `/Volumes/Yotta/ai_native_sdlc`
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
SELECT rpc, authn_result, authz_result, role
FROM control_plane_audit
WHERE rpc IN ('Ping', 'TaskList')
ORDER BY id DESC
LIMIT 4;
-- Expected: recent rows show authn_result='succeeded' and authz_result='allowed' with role='admin'
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
SELECT rpc, subject_id, authn_result, authz_result, role, reason
FROM control_plane_audit
WHERE subject_id = 'spiffe://orchestrator/local-user/operator-qa'
ORDER BY id DESC
LIMIT 5;
-- Expected: one allowed read_only/operator row and one denied admin row with role='operator'
-- Expected: one allowed row for `TaskList` and one denied row for `ConfigDebug`, both with role='operator'
```

---

## Scenario 4: Insecure TCP Requires Explicit Unsafe Flag

### Preconditions
- Release binaries built

### Goal
Verify insecure TCP is no longer the default meaning of `--bind` and only starts when `--insecure-bind` is used explicitly.

### Steps
1. Start the daemon in insecure TCP mode:
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

### Expected
- The daemon starts on TCP only when `--insecure-bind` is passed.
- Startup logs contain an explicit unsafe warning.
- The warning makes the insecure path discoverable during QA and release review.

---

## Scenario 5: UDS Mode Remains Available Without Client Certificates

### Preconditions
- Remove explicit secure-client override for this scenario:
  ```bash
  unset ORCHESTRATOR_CONTROL_PLANE_CONFIG
  ```
- Keep `ORCHESTRATOR_SOCKET` pointed at the UDS path for the command under test

### Goal
Verify UDS mode still works as the low-friction local path without client-certificate handling.

### Steps
1. Start the daemon without `--bind`:
   ```bash
   ./target/release/orchestratord --foreground --workers 1 &
   DAEMON_PID=$!
   sleep 3
   ```
2. Verify the UDS socket exists:
   ```bash
   test -S data/orchestrator.sock
   ```
3. Force CLI to use the socket path and run a basic command:
   ```bash
   export ORCHESTRATOR_SOCKET="$(pwd)/data/orchestrator.sock"
   ./target/release/orchestrator version
   ```
4. Stop the daemon:
   ```bash
   kill "$DAEMON_PID"
   wait "$DAEMON_PID" 2>/dev/null
   unset ORCHESTRATOR_SOCKET
   ```

### Expected
- The daemon creates `data/orchestrator.sock` as before.
- The CLI can connect over UDS without providing any TLS materials.
- UDS remains the fallback local transport for development and recovery.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Secure TCP Bootstrap Generates Server And Client Materials | ☐ | | | |
| 2 | CLI Auto-Discovery Uses Generated Secure Client Config | ☐ | | | |
| 3 | Additional Operator Client Is Denied On Admin RPC | ☐ | | | |
| 4 | Insecure TCP Requires Explicit Unsafe Flag | ☐ | | | |
| 5 | UDS Mode Remains Available Without Client Certificates | ☐ | | | |
