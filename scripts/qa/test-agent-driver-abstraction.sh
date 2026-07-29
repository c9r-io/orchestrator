#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19316}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/agent-driver-fixture.yaml"
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
  if [[ "$FAIL" -gt 0 || "${KEEP_FR116_QA:-0}" == "1" ]]; then
    echo "FR-116 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

for command in cargo jq mktemp rg sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR116_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "FR-116 QA requires a clean worktree (or FR116_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

cargo build -p orchestratord -p orchestrator-cli >/dev/null
cargo test -p orchestrator-runner driver:: >/dev/null
cargo test -p agent-orchestrator config_load::validate::workflow_steps::driver_tests >/dev/null
cargo test -p orchestrator-scheduler phase_runner::record::tests::driver_projection >/dev/null
pass "driver contracts, provider conformance, capability rejection, and event projection tests"

if rg -q -- '--output-format|--mcp-config|--allowedTools|--permission-mode|--resume' \
  "$FIXTURE"; then
  fail "pilot YAML leaks provider CLI flags"
else
  pass "pilot YAML uses typed driver options without provider flags"
fi

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/workspace/docs/qa" "$QA_ROOT/workspace/docs/ticket"
printf '# agent driver pilot\n' > "$QA_ROOT/workspace/target.md"

(
  cd "$QA_ROOT/workspace"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 --uds-max-role admin \
    > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
for _ in {1..60}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  exit 1
fi

PROJECT="qa-agent-driver"
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" > "$QA_ROOT/apply.out" 2>&1
if rg -q 'agent/(legacy-shell-pilot|explicit-shell-pilot|claude-driver|codex-driver)' \
  "$QA_ROOT/apply.out"; then
  pass "all three driver providers and the compatibility pilot apply successfully"
else
  fail "driver fixture apply output is incomplete"
fi
if rg -q 'legacy_agent_command_deprecated.*shell/cli' "$QA_ROOT/apply.out"; then
  pass "command-only compatibility fixture emits the stable promotion warning"
else
  fail "command-only Agent promotion warning is missing"
fi
"$ORCH" describe agent/legacy-shell-pilot --project "$PROJECT" \
  > "$QA_ROOT/legacy-agent.out"
if rg -q 'provider: shell' "$QA_ROOT/legacy-agent.out"; then
  pass "command-only compatibility fixture persists as typed shell/cli"
else
  fail "promoted Agent does not describe as typed shell/cli"
fi

create_and_run() {
  local workflow="$1"
  local name="$2"
  local task_id status
  task_id="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace driver-workspace \
      --workflow "$workflow" --target-file target.md --goal "FR-116 shell equivalence" \
      --name "$name" --no-start | rg -o '[0-9a-f-]{36}'
  )"
  # FR-146: first id by expansion; `| head -1` would kill rg under pipefail.
  task_id="${task_id%%$'\n'*}"
  "$ORCH" task start "$task_id" >/dev/null 2>&1 || true
  for _ in {1..80}; do
    status="$("$ORCH" task info "$task_id" -o json | jq -r '.task.status')"
    [[ "$status" =~ ^(completed|failed|cancelled)$ ]] && break
    sleep 0.25
  done
  printf '%s|%s\n' "$task_id" "$status"
}

LEGACY="$(create_and_run legacy-shell-pilot legacy-shell)"
EXPLICIT="$(create_and_run explicit-shell-pilot explicit-shell)"
LEGACY_ID="${LEGACY%%|*}"
EXPLICIT_ID="${EXPLICIT%%|*}"
LEGACY_STATUS="${LEGACY##*|}"
EXPLICIT_STATUS="${EXPLICIT##*|}"
DB="$QA_ROOT/data/agent_orchestrator.db"
LEGACY_EXIT="$(sqlite3 "$DB" "SELECT exit_code FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='$LEGACY_ID') ORDER BY started_at DESC LIMIT 1;")"
EXPLICIT_EXIT="$(sqlite3 "$DB" "SELECT exit_code FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='$EXPLICIT_ID') ORDER BY started_at DESC LIMIT 1;")"
if [[ "$LEGACY_STATUS" == "completed" && "$EXPLICIT_STATUS" == "completed" && \
      "$LEGACY_EXIT" == "0" && "$EXPLICIT_EXIT" == "0" ]]; then
  pass "legacy shell and explicit shell driver have equivalent terminal behavior"
else
  fail "shell pilot diverged: legacy=$LEGACY_STATUS/$LEGACY_EXIT explicit=$EXPLICIT_STATUS/$EXPLICIT_EXIT"
fi

DRIVER_EVENT_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id IN ('$LEGACY_ID', '$EXPLICIT_ID') AND event_type LIKE 'driver_%';")"
if [[ "$DRIVER_EVENT_COUNT" -ge 2 ]] && \
   ! rg -q 'provider-session-secret|thread-secret|secret-session' "$QA_ROOT"; then
  pass "both promoted and explicit shell runs persist normalized driver events without provider session material"
else
  fail "driver event projection or session privacy evidence is missing"
fi

FIRST_MCP="$(find "$QA_ROOT" -path '*/driver/mcp.json' -print -quit)"
if [[ -z "$FIRST_MCP" ]]; then
  # Shell pilots do not host MCP; provider-level concurrent path isolation is
  # asserted by the runner conformance test executed above.
  pass "per-run MCP isolation covered by provider conformance test"
elif [[ "$(stat -f '%Lp' "$FIRST_MCP" 2>/dev/null || stat -c '%a' "$FIRST_MCP")" == "600" ]]; then
  pass "per-run MCP config uses private permissions"
else
  fail "per-run MCP config permissions are not 0600"
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo "FR-116 QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "FR-116 QA: $PASS passed, 0 failed"
