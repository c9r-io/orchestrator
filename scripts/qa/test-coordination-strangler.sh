#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19324}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/coordination-strangler-parity.yaml"
FAKE_CLAUDE="$REPO_ROOT/scripts/qa/fixtures/fake-claude-strangler.sh"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
DAEMON_PID=""
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ "$FAIL" -gt 0 || "${KEEP_FR124_QA:-0}" == "1" ]]; then
    echo "FR-124 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

for command in cargo git jq mktemp rg ruby sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR124_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "FR-124 QA requires a clean worktree (or FR124_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

ruby scripts/qa/coordination-governance.rb --test-fixtures >/dev/null
pass "inventory, ratchet, rejection, governance, and safety fixtures pass"

cargo build -p orchestratord -p orchestrator-cli >/dev/null
cargo build -p orchestrator-runner --bin orch-mcp-tools >/dev/null
cargo test -p orchestrator-scheduler authenticated_host_executes_real_coordination_tools \
  >/dev/null
pass "typed coordination host and all real tool contracts pass offline"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
export FR124_FAKE_TRACE="$QA_ROOT/workspace/.fr124-fake-trace"
mkdir -p "$QA_ROOT/workspace/docs/qa" "$QA_ROOT/workspace/docs/ticket" \
  "$QA_ROOT/workspace/src" "$QA_ROOT/workspace/scripts/qa" "$QA_ROOT/bin"
cp "$FAKE_CLAUDE" "$QA_ROOT/bin/fake-claude-strangler"
chmod 700 "$QA_ROOT/bin/fake-claude-strangler"
export PATH="$QA_ROOT/bin:$PATH"
printf '# FR-124 parity target\n' > "$QA_ROOT/workspace/docs/qa/pilot.md"
printf '%s\n' '[package]' 'name = "strangler-parity"' 'version = "0.1.0"' \
  'edition = "2024"' > "$QA_ROOT/workspace/Cargo.toml"
printf '%s\n' '#[test]' 'fn parity_passes() { assert_eq!(2 + 2, 4); }' \
  > "$QA_ROOT/workspace/src/lib.rs"
(
  cd "$QA_ROOT/workspace"
  git init -q
  git config user.email qa@example.invalid
  git config user.name "FR-124 QA"
  git add .
  git commit -qm baseline
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 --webhook-bind none \
    --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,260p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  exit 1
fi

PROJECT="qa-coordination-strangler"
(
  cd "$QA_ROOT/workspace"
  "$ORCH" apply --project "$PROJECT" -f "$FIXTURE" > "$QA_ROOT/apply.out"
)
if [[ "$(rg -c '^workflow/' "$QA_ROOT/apply.out")" -eq 14 ]]; then
  pass "all seven independent legacy/tool pairs apply"
else
  cat "$QA_ROOT/apply.out" >&2
  fail "parity matrix did not apply all fourteen workflows"
fi

create_and_wait() {
  local workflow="$1"
  local task_id status
  task_id="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace strangler-parity \
      --workflow "$workflow" --target-file docs/qa/pilot.md \
      --goal "FR-124 independent parity" --name "$workflow" --no-start |
      rg -o '[0-9a-f-]{36}' | head -1
  )"
  "$ORCH" task start "$task_id" >/dev/null
  status="pending"
  for _ in {1..240}; do
    status="$("$ORCH" task info "$task_id" -o json | jq -r '.task.status')"
    [[ "$status" =~ ^(completed|failed|cancelled)$ ]] && break
    sleep 0.25
  done
  printf '%s|%s\n' "$task_id" "$status"
}

DB="$QA_ROOT/data/agent_orchestrator.db"
declare -a CASES=(command qa plan fullqa bootstrap promotion evolution)
declare -A PRODUCTION=(
  [command]="command_rules"
  [qa]="qa_loop"
  [plan]="plan_execute"
  [fullqa]="full-qa"
  [bootstrap]="self-bootstrap"
  [promotion]="promotion"
  [evolution]="self-evolution"
)
EVIDENCE='[]'
for name in "${CASES[@]}"; do
  legacy="$(create_and_wait "parity-${name}-legacy")"
  tools="$(create_and_wait "parity-${name}-tools")"
  legacy_id="${legacy%%|*}"
  tools_id="${tools%%|*}"
  legacy_status="${legacy##*|}"
  tools_status="${tools##*|}"
  if [[ "$legacy_status" == "completed" && "$tools_status" == "completed" ]]; then
    pass "${PRODUCTION[$name]} legacy/tool terminal parity is completed"
  else
    fail "${PRODUCTION[$name]} diverged: legacy=$legacy_status tools=$tools_status"
  fi
  tool_events="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$tools_id' AND event_type IN ('driver_tool_use','driver_tool_result','coordination_tool_started','coordination_tool_completed');")"
  if [[ "$name" == "command" || "$tool_events" -ge 4 ]]; then
    pass "${PRODUCTION[$name]} has typed event evidence"
  else
    fail "${PRODUCTION[$name]} lacks complete typed event evidence"
  fi
  EVIDENCE="$(jq -c \
    --arg workflow "${PRODUCTION[$name]}" \
    --arg legacy_task "$legacy_id" \
    --arg tool_task "$tools_id" \
    --arg terminal "$tools_status" \
    --argjson tool_events "$tool_events" \
    '. + [{workflow:$workflow,legacy_task:$legacy_task,tool_task:$tool_task,terminal:$terminal,typed_event_count:$tool_events}]' \
    <<<"$EVIDENCE")"
done

if rg -q $'^resume\\t.*SESSION_RESUME' "$FR124_FAKE_TRACE" &&
   ! rg -q $'^resume\\t.*SESSION_INIT' "$FR124_FAKE_TRACE"; then
  pass "provider session continuation is opt-in and fresh steps stay isolated"
else
  cat "$FR124_FAKE_TRACE" >&2
  fail "provider session resume boundary is incorrect"
fi

BOOTSTRAP_TOOL_ID="$(jq -r '.[] | select(.workflow=="self-bootstrap") | .tool_task' <<<"$EVIDENCE")"
BOOTSTRAP_CYCLES="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$BOOTSTRAP_TOOL_ID' AND event_type='cycle_started';")"
BOOTSTRAP_SELF_TESTS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$BOOTSTRAP_TOOL_ID' AND payload_json LIKE '%self_test%';")"
if [[ "$BOOTSTRAP_CYCLES" -eq 2 && "$BOOTSTRAP_SELF_TESTS" -gt 0 ]] &&
   rg -q 'binary_snapshot: true' docs/workflow/self-bootstrap.yaml &&
   rg -q 'builtin: self_restart' docs/workflow/self-bootstrap.yaml &&
   rg -q 'self_referential: true' docs/workflow/self-bootstrap.yaml &&
   rg -q 'watchdog' docs/guide docs/design_doc scripts; then
  pass "self-bootstrap retains two cycles, self-test, snapshot, restart, self-reference, and watchdog evidence"
else
  fail "self-bootstrap survival-mechanism regression"
fi

jq -n \
  --arg schemaVersion "1" \
  --argjson workflows "$EVIDENCE" \
  --argjson sourceTouches "$(ruby scripts/qa/coordination-governance.rb |
    sed -n '/^{/,$p' | jq '.sourceTouches')" \
  '{schemaVersion:($schemaVersion|tonumber),workflows:$workflows,sourceTouches:$sourceTouches}' \
  > "$QA_ROOT/coordination-strangler-evidence.json"

if [[ "$FAIL" -ne 0 ]]; then
  sed -n '1,360p' "$QA_ROOT/daemon.log" >&2
  echo "FR-124 QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "FR-124 QA: $PASS passed, 0 failed"
cat "$QA_ROOT/coordination-strangler-evidence.json"
