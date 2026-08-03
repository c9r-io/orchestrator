#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19103}"
PROJECT="qa-process-console-vertical"
HOST_HOME="$HOME"
HOST_CARGO_HOME="${CARGO_HOME:-$HOST_HOME/.cargo}"
HOST_RUSTUP_HOME="${RUSTUP_HOME:-$HOST_HOME/.rustup}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in cargo jq mktemp rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-process-console-vertical-flow.sh"

export HOME="$QA_HOME"
export CARGO_HOME="$HOST_CARGO_HOME"
export RUSTUP_HOME="$HOST_RUSTUP_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# FR-103 deterministic Process Console target\n' > "$QA_ROOT/fixtures/qa/vertical.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --webhook-bind none --workers 1 \
    > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"

for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon.log" >&2
  exit 1
fi

"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/process-console-vertical-flow.yaml" >/dev/null

export FR103_LIVE_E2E=1
export FR103_PROJECT="$PROJECT"
export FR103_TARGET="fixtures/qa/vertical.md"
(
  cd "$REPO_ROOT"
  cargo test -p orchestrator-gui live_failed_process_crosses_real_tauri_handlers_and_grpc \
    -- --nocapture
) 2>&1 | tee "$QA_ROOT/bridge-test.log"

TASK_ID="$(sed -n 's/^FR103_TASK_ID=//p' "$QA_ROOT/bridge-test.log" | tail -1)"
if [[ -n "$TASK_ID" ]] && rg -q 'test result: ok' "$QA_ROOT/bridge-test.log"; then
  pass "real Tauri IPC handlers completed the failed-process vertical flow over gRPC"
else
  fail "live Tauri bridge test did not return a task identifier"
fi

"$ORCH" attention list --task "$TASK_ID" --state resolved -o json > "$QA_ROOT/resolved.json"
if jq -e '.items | any(.state == "resolved")' "$QA_ROOT/resolved.json" >/dev/null; then
  pass "durable resume state resolved the source Attention item"
else
  fail "source Attention item is not durably resolved"
fi

"$ORCH" audit list --project "$PROJECT" -o json > "$QA_ROOT/audit.json"
if jq -e '
  ([.[] | select(.action == "handoff.generate" and .status == "succeeded")] | length) >= 1 and
  ([.[] | select(.action == "resume.plan" and .status == "succeeded")] | length) >= 2 and
  ([.[] | select(.action == "resume.execute" and .status == "failed")] | length) >= 1 and
  ([.[] | select(.action == "resume.execute" and .status == "succeeded")] | length) >= 1 and
  ([.[] | select(.action == "resume.execute") | .request_id] | all(length > 0))
' "$QA_ROOT/audit.json" >/dev/null; then
  pass "handoff, plan, rejected execution, and successful execution retain request IDs"
else
  fail "canonical recovery audit sequence is incomplete"
fi

HANDOFF_AT="$(jq -r '[.[] | select(.action == "handoff.generate" and .status == "succeeded")][0].created_at // empty' "$QA_ROOT/audit.json")"
RESUME_AT="$(jq -r '[.[] | select(.action == "resume.execute" and .status == "succeeded")][0].created_at // empty' "$QA_ROOT/audit.json")"
if [[ -n "$HANDOFF_AT" && -n "$RESUME_AT" && "$HANDOFF_AT" < "$RESUME_AT" ]]; then
  pass "durable audit order records handoff review before successful resume"
else
  fail "handoff and resume audit order is invalid"
fi

if ! rg -n 'prompt|transcript|stdout|stderr|token=|api[_-]?key' "$QA_ROOT/resolved.json" "$QA_ROOT/audit.json" >/dev/null; then
  pass "public Attention and audit evidence excludes raw sensitive payload fields"
else
  fail "public vertical-flow evidence contains a forbidden sensitive field"
fi

echo ""
echo "Process Console vertical flow QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
