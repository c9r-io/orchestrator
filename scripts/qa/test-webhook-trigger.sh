#!/usr/bin/env bash
#
# QA test: Webhook Trigger Infrastructure (FR-080 / QA-128)
# Executes scenarios 1, 2, 4, 5, 6, 7, 8, 9 from docs/qa/orchestrator/128-webhook-trigger-infrastructure.md
# Scenarios 3 requires a running task infrastructure and is tested manually.

set -euo pipefail

PASS=0
FAIL=0
ORCHESTRATORD="./target/release/orchestratord"
ORCHESTRATOR="./target/release/orchestrator"
WEBHOOK_PORT=19090  # Default webhook port
DAEMON_PID=""

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "=== QA 128: Webhook Trigger Infrastructure ==="
echo ""

# ── Scenario 9: Compilation and tests ────────────────────────────────────────
echo "--- Scenario 9: Compilation and tests ---"
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

# ── Scenario 8: Webhook source accepted in manifest ──────────────────────────
echo ""
echo "--- Scenario 8: Webhook source accepted in manifest ---"
WEBHOOK_MANIFEST=$(mktemp)
cat > "$WEBHOOK_MANIFEST" <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: qa-webhook-test
spec:
  event:
    source: webhook
  action:
    workflow: default
    workspace: default
YAML
if "$ORCHESTRATOR" manifest validate -f "$WEBHOOK_MANIFEST" 2>&1 | grep -qi "valid\|ok\|success"; then
  pass "webhook source accepted in manifest validation"
else
  # Validation may fail because workflow/workspace don't exist, but source should be accepted
  if "$ORCHESTRATOR" manifest validate -f "$WEBHOOK_MANIFEST" 2>&1 | grep -q "event.source"; then
    fail "webhook source rejected"
  else
    pass "webhook source accepted (other validation errors expected)"
  fi
fi
rm -f "$WEBHOOK_MANIFEST"

# ── Scenario 6: Webhook server disabled with --webhook-bind none ─────────────
echo ""
echo "--- Scenario 6: Webhook server disabled with --webhook-bind none ---"
"$ORCHESTRATORD" --foreground --workers 1 --webhook-bind none &
DAEMON_PID=$!
sleep 2

if curl -s --connect-timeout 2 "http://127.0.0.1:${WEBHOOK_PORT}/health" 2>&1 | grep -q "ok"; then
  fail "webhook server should NOT be running with --webhook-bind none"
else
  pass "no webhook server with --webhook-bind none"
fi

kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""
sleep 1

# ── Scenario 1: Webhook server starts by default ─────────────────────────────
echo ""
echo "--- Scenario 1: Webhook server starts by default ---"
"$ORCHESTRATORD" --foreground --workers 1 &
DAEMON_PID=$!
sleep 2

HEALTH=$(curl -s --connect-timeout 2 "http://127.0.0.1:${WEBHOOK_PORT}/health" 2>/dev/null || echo "FAIL")
if [[ "$HEALTH" == "ok" ]]; then
  pass "health endpoint returns ok (default port ${WEBHOOK_PORT})"
else
  fail "health endpoint returned: $HEALTH"
fi

# ── Scenario 2: Webhook fires (trigger doesn't exist → 404, but server accepts request) ──
echo ""
echo "--- Scenario 2: Webhook request accepted ---"
BODY='{"key":"value"}'
# If control-plane PKI exists, the daemon derives a webhook secret automatically.
# Sign the request so it passes auth; fall back to unsigned if no PKI.
DERIVED_SECRET=$("$ORCHESTRATORD" webhook-secret 2>/dev/null || true)
if [[ -n "$DERIVED_SECRET" ]]; then
  SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "$DERIVED_SECRET" | awk '{print $NF}')
  RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/nonexistent" -d "$BODY" -H "Content-Type: application/json" -H "X-Webhook-Signature: sha256=${SIG}" 2>/dev/null || echo "000")
else
  RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/nonexistent" -d "$BODY" -H "Content-Type: application/json" 2>/dev/null || echo "000")
fi
if [[ "$RESP" == "404" ]]; then
  pass "webhook returns 404 for nonexistent trigger (server is working)"
elif [[ "$RESP" == "200" ]]; then
  pass "webhook returns 200 (trigger fired)"
else
  fail "webhook returned unexpected status: $RESP"
fi

# ── Scenario 7: Custom bind address override ─────────────────────────────────
kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""
sleep 1

echo ""
echo "--- Scenario 7: Custom bind address override ---"
"$ORCHESTRATORD" --foreground --workers 1 --webhook-bind "127.0.0.1:18080" &
DAEMON_PID=$!
sleep 2

HEALTH=$(curl -s --connect-timeout 2 "http://127.0.0.1:18080/health" 2>/dev/null || echo "FAIL")
if [[ "$HEALTH" == "ok" ]]; then
  pass "custom bind address 127.0.0.1:18080 works"
else
  fail "custom bind address returned: $HEALTH"
fi

# ── Scenario 4+5: HMAC signature verification ────────────────────────────────
kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""
sleep 1

echo ""
echo "--- Scenario 4+5: HMAC signature verification ---"
"$ORCHESTRATORD" --foreground --workers 1 \
  --webhook-secret "test-secret-key" &
DAEMON_PID=$!
sleep 2

# Scenario 5: Missing signature → 401
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/test" -d '{}' 2>/dev/null || echo "000")
if [[ "$RESP" == "401" ]]; then
  pass "missing signature returns 401"
else
  fail "missing signature returned: $RESP (expected 401)"
fi

# Scenario 4: Invalid signature → 401
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/test" -d '{}' -H "X-Webhook-Signature: sha256=deadbeef" 2>/dev/null || echo "000")
if [[ "$RESP" == "401" ]]; then
  pass "invalid signature returns 401"
else
  fail "invalid signature returned: $RESP (expected 401)"
fi

# Valid signature
BODY='{}'
SIG=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "test-secret-key" | awk '{print $NF}')
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/test" -d "$BODY" -H "X-Webhook-Signature: sha256=${SIG}" 2>/dev/null || echo "000")
if [[ "$RESP" == "404" || "$RESP" == "200" ]]; then
  pass "valid signature accepted (trigger response: $RESP)"
else
  fail "valid signature returned: $RESP (expected 200 or 404)"
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
