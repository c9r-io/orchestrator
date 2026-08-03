#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
PROJECT="qa-expert-resources"
HOST_HOME="$HOME"
HOST_CARGO_HOME="${CARGO_HOME:-$HOST_HOME/.cargo}"
HOST_RUSTUP_HOME="${RUSTUP_HOME:-$HOST_HOME/.rustup}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in cargo jq mktemp rg sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui" >&2
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
gate_runlog_arm "scripts/qa/test-expert-resources-governed-editing.sh"

export HOME="$QA_HOME"
export CARGO_HOME="$HOST_CARGO_HOME"
export RUSTUP_HOME="$HOST_RUSTUP_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
unset ORCHESTRATOR_SOCKET
unset ORCHESTRATOR_CONTROL_PLANE_CONFIG
mkdir -p "$QA_ROOT/docs/qa" "$QA_ROOT/docs/ticket-v1"

FIXTURE="$QA_ROOT/expert-resources.yaml"
cat > "$FIXTURE" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: fr119-workspace
spec:
  root_path: .
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket-v1
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: fr119-workflow
spec:
  steps:
    - id: inspect
      type: inspect
      enabled: true
      command: "echo inspected"
  loop:
    mode: once
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: fr119-agent
spec:
  capabilities: [inspect]
  command: "echo '{\"confidence\":1.0,\"quality_score\":1.0,\"artifacts\":[]}'"
---
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: fr119-step
spec:
  prompt: Inspect the governed resource.
---
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: fr119-profile
spec:
  mode: sandbox
  fs_mode: workspace_readonly
  network_mode: deny
EOF

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --webhook-bind none --workers 1 \
    --uds-max-role admin > daemon.log 2>&1 &
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

"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null
for query in workspaces workflows agents steptemplates executionprofiles; do
  "$ORCH" get "$query" --project "$PROJECT" -o json > "$QA_ROOT/$query.json"
done
if jq -e 'index("fr119-workspace") != null' "$QA_ROOT/workspaces.json" >/dev/null &&
  jq -e 'index("fr119-workflow") != null' "$QA_ROOT/workflows.json" >/dev/null &&
  jq -e 'index("fr119-agent") != null' "$QA_ROOT/agents.json" >/dev/null &&
  jq -e 'index("fr119-step") != null' "$QA_ROOT/steptemplates.json" >/dev/null &&
  jq -e 'index("fr119-profile") != null' "$QA_ROOT/executionprofiles.json" >/dev/null; then
  pass "all five expert resource collections are queryable"
else
  fail "one or more expert resource collections are missing"
fi

(
  cd "$REPO_ROOT"
  cargo test -p orchestrator-integration-tests --test grpc_compat \
    apply_get_describe_roundtrip -- --exact
) > "$QA_ROOT/catalog-test.log" 2>&1
if rg -q 'test result: ok' "$QA_ROOT/catalog-test.log"; then
  pass "typed gRPC catalog round-trip returns stable summaries"
else
  fail "typed gRPC catalog round-trip failed"
fi

export FR119_LIVE_E2E=1
export FR119_PROJECT="$PROJECT"
export ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"
(
  cd "$REPO_ROOT"
  cargo test -p orchestrator-gui \
    live_expert_resources_cross_real_tauri_catalog_describe_apply_and_conflict \
    -- --nocapture
) 2>&1 | tee "$QA_ROOT/tauri-bridge.log"
if rg -q '^FR119_APPLY_REQUEST_ID=.+$' "$QA_ROOT/tauri-bridge.log" &&
  rg -q '^FR119_STALE_REJECTED=1$' "$QA_ROOT/tauri-bridge.log" &&
  rg -q 'test result: ok' "$QA_ROOT/tauri-bridge.log"; then
  pass "real Tauri bridge applies reviewed edits and rejects stale revisions"
else
  fail "real Tauri catalog/apply/conflict bridge failed"
fi

INVALID="$QA_ROOT/invalid-sensitive.yaml"
cat > "$INVALID" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: invalid-sensitive
spec:
  root_path: .
  ticket_dir: qa-resource-sensitive-marker
  malformed: [
EOF
if ! "$ORCH" apply --project "$PROJECT" -f "$INVALID" > "$QA_ROOT/invalid.log" 2>&1; then
  pass "invalid manifest is rejected by daemon validation"
else
  fail "invalid manifest unexpectedly applied"
fi

"$ORCH" audit list --project "$PROJECT" --action resource.apply -o json > "$QA_ROOT/audit.json"
if jq -e '
  ([.[] | select(.target_type == "resource" and .target_id == "Workspace/fr119-workspace" and .status == "succeeded")] | length) == 1 and
  ([.[] | select(.target_type == "resource" and .target_id == "Workspace/fr119-workspace" and .status == "failed")] | length) == 1 and
  ([.[] | select(.status == "failed")] | length) >= 2 and
  ([.[] | select(.target_id == "Workspace/fr119-workspace") | .request_id] | all(length > 0))
' "$QA_ROOT/audit.json" >/dev/null; then
  pass "successful, stale, and invalid applies retain canonical audit evidence"
else
  fail "resource apply action audit sequence is incomplete"
fi

DB="$QA_ROOT/runtime/agent_orchestrator.db"
if ! rg -n 'qa-resource-sensitive-marker' \
    "$QA_ROOT/audit.json" "$QA_ROOT/daemon.log" "$QA_ROOT/tauri-bridge.log" >/dev/null &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM control_action_audit WHERE COALESCE(target_id,'') || COALESCE(operator_reason,'') || COALESCE(error_code,'') LIKE '%qa-resource-sensitive-marker%';")" == "0" ]]; then
  pass "manifest sentinel is absent from audit, daemon logs, UI errors, and canonical requests"
else
  fail "resource manifest sentinel leaked into public or durable evidence"
fi

echo ""
echo "Expert Resources governed editing QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
