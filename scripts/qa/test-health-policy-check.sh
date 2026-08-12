#!/usr/bin/env bash
# QA-110b S2: Verify orchestrator check displays custom health_policy correctly.
#
# Starts its own daemon. It used to require an ambient one, which is why it was
# one of three gates that had never run since the freshness ledger was built:
# "requires a running orchestratord instance" is a precondition nobody satisfies
# by accident, and a release precondition that cannot be executed is not a
# precondition. FR-165 closed that by giving it the same shape as the other 33 —
# an ephemeral daemon over a temporary data directory, so nothing it does can
# reach the operator's own database. The three projects it used to delete by name
# on exit no longer exist anywhere but in that directory.
set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19231}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-health-policy-check.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

if ! gate_daemon_wait_ready "$ORCH"; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

# ─── Scenario 1: Custom thresholds ────────────────────────────────────
echo ""
echo "═══ S2-a: Custom health_policy thresholds ═══"

$ORCH apply -f fixtures/manifests/bundles/qa110-s2-fixture.yaml --project qa-hp-s2 >/dev/null 2>&1
check_out=$($ORCH check --project qa-hp-s2 2>&1)

if grep -q 'health policy = custom (duration=1h, threshold=5, cap_success=0.3)' <<< "$check_out"; then
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

if grep -q 'disease DISABLED' <<< "$check_out"; then
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

if grep -q 'health policy = default (duration=5h, threshold=2, cap_success=0.5)' <<< "$check_out"; then
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
