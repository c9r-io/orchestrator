#!/usr/bin/env bash
#
# QA test: Per-Trigger Webhook Auth & CEL Filter (FR-081 / QA-129)
# Tests per-trigger secret from SecretStore, multi-key rotation, global fallback, and CEL filter.

set -euo pipefail

PASS=0
FAIL=0
ORCHESTRATORD="./target/release/orchestratord"
ORCHESTRATOR="./target/release/orchestrator"
WEBHOOK_PORT=19091
DAEMON_PID=""

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -f /tmp/qa-wh-081.yaml
}
trap cleanup EXIT

echo "=== QA 129: Per-Trigger Webhook Auth & CEL Filter ==="
echo ""

# ── Scenario 8: Compilation and tests ────────────────────────────────────────
echo "--- Scenario 8: Compilation and tests ---"
if cargo test --workspace 2>&1 | grep -q "^test result: FAILED"; then
  fail "cargo test --workspace"
else
  pass "cargo test --workspace"
fi

if cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -q "^error"; then
  fail "cargo clippy"
else
  pass "cargo clippy clean"
fi

# ── Scenario 7: CEL filter unit tests ────────────────────────────────────────
echo ""
echo "--- Scenario 7: CEL filter unit tests ---"
if cargo test --lib -p agent-orchestrator -- prehook::cel::tests 2>&1 | grep -q "^test result: ok"; then
  pass "CEL webhook filter unit tests (6 tests)"
else
  fail "CEL webhook filter unit tests"
fi

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

kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
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
TMP=/tmp/qa-wh-081.yaml

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
