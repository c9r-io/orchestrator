#!/usr/bin/env bash
# QA-110b S2: Verify orchestrator check displays custom health_policy correctly.
# Requires a running orchestratord instance.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="${ORCH:-orchestrator}"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

cleanup() {
  echo ""
  echo "Cleaning up test projects..."
  for proj in qa-hp-s2 qa-hp-s3 qa-hp-s1; do
    $ORCH delete agent --project "$proj" --force 2>/dev/null || true
    $ORCH delete workflow --project "$proj" --force 2>/dev/null || true
    $ORCH delete workspace --project "$proj" --force 2>/dev/null || true
  done
}
trap cleanup EXIT

# ─── Scenario 1: Custom thresholds ────────────────────────────────────
echo ""
echo "═══ S2-a: Custom health_policy thresholds ═══"

$ORCH apply -f fixtures/manifests/bundles/qa110-s2-fixture.yaml --project qa-hp-s2 >/dev/null 2>&1
check_out=$($ORCH check --project qa-hp-s2 2>&1)

if echo "$check_out" | grep -q 'health policy = custom (duration=1h, threshold=5, cap_success=0.3)'; then
  pass "custom-fail agent displays custom thresholds"
else
  fail "expected custom thresholds in check output"
  echo "  Got: $check_out"
fi

# ─── Scenario 2: Disease DISABLED ─────────────────────────────────────
echo ""
echo "═══ S2-b: Disease DISABLED display ═══"

$ORCH apply -f fixtures/manifests/bundles/qa110-s3-fixture.yaml --project qa-hp-s3 >/dev/null 2>&1
check_out=$($ORCH check --project qa-hp-s3 2>&1)

if echo "$check_out" | grep -q 'disease DISABLED'; then
  pass "nodisease-fail agent displays disease DISABLED"
else
  fail "expected 'disease DISABLED' in check output"
  echo "  Got: $check_out"
fi

# ─── Scenario 3: Default policy baseline ──────────────────────────────
echo ""
echo "═══ S2-c: Default health_policy baseline ═══"

$ORCH apply -f fixtures/manifests/bundles/qa110-s1-fixture.yaml --project qa-hp-s1 >/dev/null 2>&1
check_out=$($ORCH check --project qa-hp-s1 2>&1)

if echo "$check_out" | grep -q 'health policy = default (duration=5h, threshold=2, cap_success=0.5)'; then
  pass "default-agent-fail displays default policy"
else
  fail "expected default policy in check output"
  echo "  Got: $check_out"
fi

# ─── Summary ──────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════"
echo "  QA-110b S2 Summary"
echo "  PASS: $PASS / 3"
echo "  FAIL: $FAIL / 3"
echo "═══════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
