#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19112}"
PROJECT="qa-source-automation-ui"
HOST_HOME="$HOME"
HOST_CARGO_HOME="${CARGO_HOME:-$HOST_HOME/.cargo}"
HOST_RUSTUP_HOME="${RUSTUP_HOME:-$HOST_HOME/.rustup}"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
DAEMON_PID=""
PASS=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
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

for command in cargo jq mktemp npm rg; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 1; }
done

cd "$REPO_ROOT"
if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui
fi

if [[ "${SKIP_FRONTEND:-0}" != "1" ]]; then
  (
    cd gui
    npm run test:coverage
    npm run test:e2e
    npm run build
  )
  pass "Vitest coverage, Playwright UI/accessibility, and production build"
fi

export HOME="$QA_HOME"
export CARGO_HOME="$HOST_CARGO_HOME"
export RUSTUP_HOME="$HOST_RUSTUP_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
unset ORCHESTRATOR_SOCKET
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --webhook-bind none --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  exit 1
fi
"$ORCH" secret key bootstrap >/dev/null 2>&1 || true
for _ in {1..60}; do
  "$ORCH" secret key status -o json 2>/dev/null | jq -e '.active_key != null' >/dev/null && break
  sleep 0.25
done
if ! "$ORCH" secret key status -o json 2>/dev/null | jq -e '.active_key != null' >/dev/null; then
  echo "isolated daemon did not create an active secret encryption key" >&2
  exit 1
fi

"$ORCH" apply --project "$PROJECT" -f "$REPO_ROOT/fixtures/manifests/bundles/source-task-routing-fixture.yaml" >/dev/null
cat > "$QA_ROOT/second-automation.yaml" <<'YAML'
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: docs
    invocation: "$docs"
    args: ["--concise"]
  action:
    workflow: source-routing-fixture
    workspace: source-routing-fixture
    start: false
  goalTemplate: "{skill_invocation}: document {source_message_url}"
  allowedVariables: [skill_invocation, source_message_url]
---
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-document
spec:
  triggerRef: slack-routing
  match:
    eventKind: reaction_added
    reaction: agent-document
    targetKind: message
    channels: [C_QA_ROUTING]
  templateRef: docs-from-slack
  allowedActorRoles: [operator]
  suspend: false
YAML
"$ORCH" apply --project "$PROJECT" -f "$QA_ROOT/second-automation.yaml" >/dev/null

FR112_LIVE_E2E=1 FR112_PROJECT="$PROJECT" \
  cargo test -p orchestrator-gui live_source_automation_crosses_real_tauri_handlers_and_grpc -- --nocapture \
  2>&1 | tee "$QA_ROOT/bridge-test.log"
rg -q 'FR112_BRIDGE_OK=1' "$QA_ROOT/bridge-test.log"
pass "real Tauri IPC serializes catalog, preview, simulation, CAS, suspend, and resume"

if rg -n 'normalized_json|signing.secret|bot.token|message body' gui/src gui/tests \
    --glob '!**/*.test.tsx' --glob '!**/*.spec.ts' >/dev/null; then
  echo "forbidden raw Slack or credential field found in production frontend" >&2
  exit 1
fi
pass "production frontend excludes raw Slack payload and credential fields"

kill "$DAEMON_PID" 2>/dev/null || true
wait "$DAEMON_PID" 2>/dev/null || true
DAEMON_PID=""

if [[ "${SKIP_DEPENDENCY_GATES:-0}" != "1" ]]; then
  "$SCRIPT_DIR/test-source-automation-operations.sh"
  pass "durable source automation routing, replay, privacy, and restart gates"
fi

echo "Source automation UI QA passed: $PASS gates"
