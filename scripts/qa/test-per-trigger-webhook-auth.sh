#!/usr/bin/env bash
#
# QA test: Per-Trigger Webhook Auth & CEL Filter (FR-081 / QA-129)
# Tests per-trigger secret from SecretStore, multi-key rotation, global fallback, and CEL filter.

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

PASS=0
FAIL=0
ORCHESTRATORD="./target/release/orchestratord"
ORCHESTRATOR="./target/release/orchestrator"
WEBHOOK_PORT=19091
DAEMON_PID=""

# Isolation scratch dirs; the HOME/data-dir overrides happen later, just
# before the first daemon start — the cargo scenarios need the real HOME (see
# test-webhook-trigger.sh for the nested-cargo mechanism).
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-per-trigger-webhook-auth.sh"

echo "=== QA 129: Per-Trigger Webhook Auth & CEL Filter ==="
echo ""

# ── Scenario 8: Compilation and tests ────────────────────────────────────────
echo "--- Scenario 8: Compilation and tests ---"
# Captured rather than piped. `grep -q` leaves on the first match, so a suite
# that printed `test result: FAILED` while cargo was still writing killed cargo
# with EPIPE; `set -o pipefail` handed that status to the `if`, the condition
# read false, and the gate reported `PASS: cargo test --workspace` over a failing
# suite (FR-145). Capturing the status too, because that is the question being
# asked and it is free once the output is in a variable.
if CARGO_TEST_OUT="$(cargo test --workspace 2>&1)"; then CARGO_TEST_STATUS=0; else CARGO_TEST_STATUS=$?; fi
if [[ "$CARGO_TEST_STATUS" -ne 0 ]] || grep -q "^test result: FAILED" <<< "$CARGO_TEST_OUT"; then
  fail "cargo test --workspace"
  # A swallowed diagnosis costs a whole re-run; print the tail of what failed.
  tail -40 <<< "$CARGO_TEST_OUT" | sed 's/^/    /' >&2
else
  pass "cargo test --workspace"
fi

if CARGO_CLIPPY_OUT="$(cargo clippy --workspace --all-targets -- -D warnings 2>&1)"; then CARGO_CLIPPY_STATUS=0; else CARGO_CLIPPY_STATUS=$?; fi
if [[ "$CARGO_CLIPPY_STATUS" -ne 0 ]] || grep -q "^error" <<< "$CARGO_CLIPPY_OUT"; then
  fail "cargo clippy"
else
  pass "cargo clippy clean"
fi

# ── Scenario 7: CEL filter unit tests ────────────────────────────────────────
echo ""
echo "--- Scenario 7: CEL filter unit tests ---"
CEL_TEST_OUT="$(cargo test --lib -p agent-orchestrator -- prehook::cel::tests 2>&1 || true)"
if grep -q "^test result: ok" <<< "$CEL_TEST_OUT"; then
  pass "CEL webhook filter unit tests (6 tests)"
else
  fail "CEL webhook filter unit tests"
fi

# Isolation: without these, the daemons below run against the operator's real
# ~/.orchestratord — measured during FR-160's residue audit. The webhook port
# isolates the listener only; the daemon's UDS lands under the data directory
# while the CLI's default discovery looks under $HOME, hence all three.
export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
export ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"

# ── Scenario 6: Global secret fallback ───────────────────────────────────────
echo ""
echo "--- Scenario 6: Global secret fallback ---"
"$ORCHESTRATORD" --foreground --workers 1 \
  --webhook-bind "127.0.0.1:${WEBHOOK_PORT}" \
  --webhook-secret "global-test-key" >/dev/null 2>&1 &
DAEMON_PID=$!
sleep 2

BODY='{}'
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "global-test-key" | awk '{print $NF}')
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${WEBHOOK_PORT}/webhook/nonexistent" \
  -d "$BODY" -H "X-Webhook-Signature: sha256=${SIG}" 2>/dev/null || echo "000")
if [[ "$RESP" == "404" || "$RESP" == "200" ]]; then
  pass "global secret fallback accepted (HTTP $RESP)"
else
  fail "global secret returned: $RESP"
fi

RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${WEBHOOK_PORT}/webhook/test" -d '{}' 2>/dev/null || echo "000")
if [[ "$RESP" == "401" ]]; then
  pass "missing signature with global secret → 401"
else
  fail "missing signature returned: $RESP (expected 401)"
fi

gate_daemon_stop "$DAEMON_PID"
DAEMON_PID=""
DAEMON_PID=""
sleep 1

# ── Scenario 3+4+5: Per-trigger secret + multi-key rotation ─────────────────
echo ""
echo "--- Scenario 3+4+5: Per-trigger secret + multi-key rotation ---"
"$ORCHESTRATORD" --foreground --workers 1 \
  --webhook-bind "127.0.0.1:${WEBHOOK_PORT}" >/dev/null 2>&1 &
DAEMON_PID=$!
sleep 2

# Apply resources one at a time via temp file
TMP="$QA_ROOT/qa-wh-081.yaml"

cat > "$TMP" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: test-signing-keys
spec:
  data:
    old_key: secret-old-value
    new_key: secret-new-value
EOF
"$ORCHESTRATOR" apply -f "$TMP" >/dev/null 2>&1

cat > "$TMP" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: auth-test
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: test-signing-keys
      signatureHeader: X-Custom-Sig
  action:
    workflow: test-wf
    workspace: default
EOF
"$ORCHESTRATOR" apply -f "$TMP" >/dev/null 2>&1

# Scenario 5: old key → accepted (404 = trigger fires but task creation may fail; auth passed)
BODY='{"test":"rotation"}'
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "secret-old-value" | awk '{print $NF}')
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${WEBHOOK_PORT}/webhook/auth-test" \
  -d "$BODY" -H "X-Custom-Sig: sha256=${SIG}" 2>/dev/null || echo "000")
if [[ "$RESP" != "401" ]]; then
  pass "old key accepted (multi-key rotation, HTTP $RESP)"
else
  fail "old key rejected with 401"
fi

# new key → accepted
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "secret-new-value" | awk '{print $NF}')
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${WEBHOOK_PORT}/webhook/auth-test" \
  -d "$BODY" -H "X-Custom-Sig: sha256=${SIG}" 2>/dev/null || echo "000")
if [[ "$RESP" != "401" ]]; then
  pass "new key accepted (multi-key rotation, HTTP $RESP)"
else
  fail "new key rejected with 401"
fi

# Scenario 4: wrong key → 401
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "wrong-secret" | awk '{print $NF}')
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://127.0.0.1:${WEBHOOK_PORT}/webhook/auth-test" \
  -d "$BODY" -H "X-Custom-Sig: sha256=${SIG}" 2>/dev/null || echo "000")
if [[ "$RESP" == "401" ]]; then
  pass "wrong key rejected (401)"
else
  fail "wrong key returned: $RESP (expected 401)"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Results ==="
echo "PASS: $PASS"
echo "FAIL: $FAIL"
echo ""
if [[ $FAIL -gt 0 ]]; then
  echo "SOME TESTS FAILED"
  exit 1
else
  echo "ALL TESTS PASSED"
  exit 0
fi
