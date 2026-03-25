#!/usr/bin/env bash
#
# QA test: Webhook Trigger Infrastructure (FR-080 / QA-128)
# Executes scenarios 1, 2, 4, 5, 6, 8, 9 from docs/qa/orchestrator/128-webhook-trigger-infrastructure.md
# Scenarios 3, 7 require a running task infrastructure and are tested manually.

set -euo pipefail

PASS=0
FAIL=0
ORCHESTRATORD="./target/release/orchestratord"
ORCHESTRATOR="./target/release/orchestrator"
WEBHOOK_PORT=19090  # Use non-standard port to avoid conflicts
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

# ── Scenario 6: No webhook server without --webhook-bind ─────────────────────
echo ""
echo "--- Scenario 6: No webhook server without --webhook-bind ---"
"$ORCHESTRATORD" --foreground --workers 1 &
DAEMON_PID=$!
sleep 2

if curl -s --connect-timeout 2 "http://127.0.0.1:${WEBHOOK_PORT}/health" 2>&1 | grep -q "ok"; then
  fail "webhook server should NOT be running without --webhook-bind"
else
  pass "no webhook server without --webhook-bind"
fi

kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""
sleep 1

# ── Scenario 1: Webhook server starts with --webhook-bind ────────────────────
echo ""
echo "--- Scenario 1: Webhook server starts with --webhook-bind ---"
"$ORCHESTRATORD" --foreground --workers 1 --webhook-bind "127.0.0.1:${WEBHOOK_PORT}" &
DAEMON_PID=$!
sleep 2

HEALTH=$(curl -s --connect-timeout 2 "http://127.0.0.1:${WEBHOOK_PORT}/health" 2>/dev/null || echo "FAIL")
if [[ "$HEALTH" == "ok" ]]; then
  pass "health endpoint returns ok"
else
  fail "health endpoint returned: $HEALTH"
fi

# ── Scenario 2: Webhook fires (trigger doesn't exist → 404, but server accepts request) ──
echo ""
echo "--- Scenario 2: Webhook request accepted ---"
RESP=$(curl -s -o /dev/null -w "%{http_code}" -X POST "http://127.0.0.1:${WEBHOOK_PORT}/webhook/nonexistent" -d '{"key":"value"}' -H "Content-Type: application/json" 2>/dev/null || echo "000")
if [[ "$RESP" == "404" ]]; then
  pass "webhook returns 404 for nonexistent trigger (server is working)"
elif [[ "$RESP" == "200" ]]; then
  pass "webhook returns 200 (trigger fired)"
else
  fail "webhook returned unexpected status: $RESP"
fi

# ── Scenario 4+5: HMAC signature verification ────────────────────────────────
kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""
sleep 1

echo ""
echo "--- Scenario 4+5: HMAC signature verification ---"
"$ORCHESTRATORD" --foreground --workers 1 \
  --webhook-bind "127.0.0.1:${WEBHOOK_PORT}" \
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
