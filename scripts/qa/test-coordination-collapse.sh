#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19318}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/coordination-collapse-pilot.yaml"
# The legacy half FR-118 measured against. Never applied as setup — `behavior.captures`
# was removed by design (DD-137), so `apply` rejects it, and case 4 asserts exactly that.
BASELINE="$REPO_ROOT/fixtures/manifests/bundles/coordination-legacy-baseline.yaml"
PILOT_GOAL="FR-118 coordination parity"
FAKE_CLAUDE="$REPO_ROOT/scripts/qa/fixtures/fake-claude-coordination.sh"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
DAEMON_PID=""
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# Setup that fails must still print the summary line. This gate spent four days ending at the
# fixture apply with no summary at all, and a truncated run reads exactly like a complete one —
# the shape SKILL.md §4.4 shape 7 names. `set -e` is what made it silent, so any setup step whose
# failure ends the run goes through here instead of being allowed to kill the script.
abort_with_summary() {
  fail "$1"
  echo "FR-118 QA: $PASS passed, $FAIL failed" >&2
  exit 1
}

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  if [[ "$FAIL" -gt 0 || "${KEEP_FR118_QA:-0}" == "1" ]]; then
    echo "FR-118 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-coordination-collapse.sh"

for command in cargo jq mktemp rg sqlite3 awk; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR118_ALLOW_DIRTY:-0}" != "1" && -n "$(gate_runlog_worktree_status "$REPO_ROOT")" ]]; then
  echo "FR-118 QA requires a clean worktree (or FR118_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

cargo build -p orchestratord -p orchestrator-cli >/dev/null
cargo build -p orchestrator-runner --bin orch-mcp-tools >/dev/null
cargo test -p orchestrator-runner --test mcp_shim >/dev/null
cargo test -p orchestrator-scheduler authenticated_host_executes_real_coordination_tools \
  >/dev/null
pass "typed host, real tool execution, and stdio callback forwarding pass offline"

TOOL_BLOCK="$(awk '/BEGIN TOOL COORDINATION/{active=1; next} /END TOOL COORDINATION/{active=0} active' "$FIXTURE")"
if rg -q 'prehook:|captures:|json_path:|post_actions:|from_var:|pipeline_vars?' \
  <<<"$TOOL_BLOCK"; then
  fail "tool pilot still contains transitional coordination mechanisms"
else
  pass "tool pilot contains no CEL, capture, JSONPath, post-action, or pipeline-var wiring"
fi

LEGACY_BLOCK="$(awk '/BEGIN LEGACY COORDINATION/{active=1; next} /END LEGACY COORDINATION/{active=0} active' "$BASELINE")"
LEGACY_EFFECTIVE_LINES="$(rg -v '^\s*(#|$)' <<<"$LEGACY_BLOCK" | wc -l | tr -d ' ')"
TOOL_EFFECTIVE_LINES="$(rg -v '^\s*(#|$)' <<<"$TOOL_BLOCK" | wc -l | tr -d ' ')"
LEGACY_COORDINATION_LINES="$(rg -c 'prehook:|engine: cel|when:|captures:|var:|source:|json_path:|post_actions:|type: scan_tickets|on_success:|action: set_status|status:' <<<"$LEGACY_BLOCK")"
TOOL_COORDINATION_LINES="$(rg -c 'prehook:|engine: cel|when:|captures:|var:|source:|json_path:|post_actions:|from_var:' <<<"$TOOL_BLOCK" || true)"
TOOL_COORDINATION_LINES="${TOOL_COORDINATION_LINES:-0}"
REDUCTION_PERCENT=$(( (LEGACY_COORDINATION_LINES - TOOL_COORDINATION_LINES) * 100 / LEGACY_COORDINATION_LINES ))
if [[ "$REDUCTION_PERCENT" -ge 80 ]]; then
  pass "handwritten coordination lines fall by ${REDUCTION_PERCENT}%"
else
  fail "coordination reduction is ${REDUCTION_PERCENT}%, below the 80% target"
fi

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/workspace/docs/qa" "$QA_ROOT/workspace/docs/ticket" \
  "$QA_ROOT/workspace/src" "$QA_ROOT/bin"
cp "$FAKE_CLAUDE" "$QA_ROOT/bin/fake-claude-coordination"
chmod 700 "$QA_ROOT/bin/fake-claude-coordination"
export PATH="$QA_ROOT/bin:$PATH"
printf '# Coordination pilot\n' > "$QA_ROOT/workspace/docs/qa/pilot.md"
printf '%s\n' '[package]' "name = \"coordination-pilot\"" 'version = "0.1.0"' \
  'edition = "2024"' > "$QA_ROOT/workspace/Cargo.toml"
printf '%s\n' '#[test]' 'fn pilot_passes() { assert_eq!(2 + 2, 4); }' \
  > "$QA_ROOT/workspace/src/lib.rs"

(
  cd "$QA_ROOT/workspace"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 --webhook-bind none \
    --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,260p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  exit 1
fi

PROJECT="qa-coordination-collapse"
APPLY_STATUS=0
(
  cd "$QA_ROOT/workspace"
  "$ORCH" apply --project "$PROJECT" -f "$FIXTURE" > "$QA_ROOT/apply.out" 2>&1
) || APPLY_STATUS=$?
if [[ "$APPLY_STATUS" -ne 0 ]]; then
  abort_with_summary "the pilot fixture did not apply (status=$APPLY_STATUS): $(head -1 "$QA_ROOT/apply.out")"
fi
if rg -q 'workflow/coordination-tools' "$QA_ROOT/apply.out"; then
  pass "the tool pilot applies in one isolated project"
else
  fail "pilot apply output is incomplete"
fi

# The retired baseline must still be rejected, and must be rejected *for the reason it was
# retired*. Asserting only that the apply failed would not separate this from the neighbouring
# diagnostic: capability validation runs before the captures check, so a baseline missing its
# agent fails with "no agent supports capability" and a bare exit code cannot tell them apart.
#
# This is what keeps the baseline file honest. It is text this gate measures; if it ever became
# appliable again, the measurement would be describing something the daemon accepts.
BASELINE_STATUS=0
(
  cd "$QA_ROOT/workspace"
  "$ORCH" apply --project "$PROJECT" -f "$BASELINE" > "$QA_ROOT/baseline-apply.out" 2>&1
) || BASELINE_STATUS=$?
if [[ "$BASELINE_STATUS" -ne 0 ]] &&
   rg -q '\[legacy_coordination_removed\].*coordination-legacy' "$QA_ROOT/baseline-apply.out"; then
  pass "the retired legacy baseline is still rejected, and names why"
else
  fail "the legacy baseline was not rejected with [legacy_coordination_removed] (status=$BASELINE_STATUS): $(head -1 "$QA_ROOT/baseline-apply.out")"
fi

create_and_wait() {
  local workflow="$1"
  local name="$2"
  local task_id status
  task_id="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace coordination-pilot \
      --workflow "$workflow" --target-file docs/qa/pilot.md \
      --goal "$PILOT_GOAL" --name "$name" --no-start | \
      rg -o '[0-9a-f-]{36}'
  )"
  # FR-146: `rg -o` reads to EOF, so the first id is taken by expansion rather than by a
  # `| head -1` that would kill rg and end the gate before its summary line.
  task_id="${task_id%%$'\n'*}"
  "$ORCH" task start "$task_id" >/dev/null
  status="pending"
  for _ in {1..160}; do
    status="$("$ORCH" task info "$task_id" -o json | jq -r '.task.status')"
    [[ "$status" =~ ^(completed|failed|cancelled)$ ]] && break
    sleep 0.25
  done
  printf '%s|%s\n' "$task_id" "$status"
}

# Only the tool pilot runs. The legacy pilot cannot: the daemon rejects the workflow, which is
# the decommission working. The parity comparison retired with the mechanism it compared against
# — what survives it is the reduction measurement above, which is a property of the two YAML
# blocks and needs no runtime, and the rejection assertion, which is a property of the daemon.
TOOLS="$(create_and_wait coordination-tools coordination-tools)"
TOOLS_ID="${TOOLS%%|*}"
TOOLS_STATUS="${TOOLS##*|}"
DB="$QA_ROOT/data/agent_orchestrator.db"
TOOLS_ITEM_STATUS="$(sqlite3 "$DB" "SELECT status FROM task_items WHERE task_id='$TOOLS_ID' ORDER BY order_no LIMIT 1;")"
if [[ "$TOOLS_STATUS" == "completed" && "$TOOLS_ITEM_STATUS" == "qa_passed" ]]; then
  pass "the tool pilot converges to completed/qa_passed"
else
  sed -n '1,300p' "$QA_ROOT/daemon.log" >&2
  fail "tool pilot diverged: tools=$TOOLS_STATUS/$TOOLS_ITEM_STATUS"
fi

DRIVER_USE_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TOOLS_ID' AND event_type='driver_tool_use';")"
DRIVER_RESULT_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TOOLS_ID' AND event_type='driver_tool_result';")"
HOST_START_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TOOLS_ID' AND event_type='coordination_tool_started';")"
HOST_END_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TOOLS_ID' AND event_type='coordination_tool_completed';")"
if [[ "$DRIVER_USE_COUNT" == "3" && "$DRIVER_RESULT_COUNT" == "3" && \
      "$HOST_START_COUNT" == "3" && "$HOST_END_COUNT" == "3" ]]; then
  pass "all pilot tool uses, results, and daemon receipts enter the event table"
else
  fail "event parity mismatch: use=$DRIVER_USE_COUNT result=$DRIVER_RESULT_COUNT host=$HOST_START_COUNT/$HOST_END_COUNT"
fi

for tool in run_tests scan_tickets mark_item; do
  if sqlite3 "$DB" "SELECT payload_json FROM events WHERE task_id='$TOOLS_ID' AND event_type='driver_tool_use';" | \
    rg -q "mcp__orch__${tool}"; then
    pass "event stream contains $tool"
  else
    fail "event stream is missing $tool"
  fi
done

MCP_CONFIG="$(find "$QA_ROOT" -path '*/driver/mcp.json' -print -quit)"
if [[ -n "$MCP_CONFIG" && \
      "$(stat -f '%Lp' "$MCP_CONFIG" 2>/dev/null || stat -c '%a' "$MCP_CONFIG")" == "600" ]]; then
  pass "run-scoped MCP config is private"
else
  fail "run-scoped MCP config is missing or not mode 0600"
fi
CALLBACK_TOKEN="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_TOKEN' "$MCP_CONFIG")"
# The dump is captured, not piped. `rg -q` leaves on the first match, and a whole
# database dump has no bound on its size: under `set -o pipefail` sqlite3's EPIPE
# would become the condition's status, and `!` would turn a *detected* leak into
# "absent from database" (FR-145). An empty dump is its own failure, because a
# condition that read nothing and one that read everything and found nothing
# carry the same exit code.
DB_DUMP="$(sqlite3 "$DB" '.dump')" || DB_DUMP=""
if [[ -z "$DB_DUMP" ]]; then
  fail "sqlite3 produced no dump of $DB, so the token-leak assertion examined nothing"
elif [[ -n "$CALLBACK_TOKEN" ]] && ! rg -q --fixed-strings "$CALLBACK_TOKEN" <<< "$DB_DUMP" && \
     ! rg -q --fixed-strings "$CALLBACK_TOKEN" "$QA_ROOT/daemon.log"; then
  pass "per-run callback token is absent from database and daemon logs"
else
  fail "callback token leaked beyond the private MCP config"
fi

TASK_PIPELINE="$(sqlite3 "$DB" "SELECT COALESCE(pipeline_vars_json,'{}') FROM tasks WHERE id='$TOOLS_ID';")"
ITEM_PIPELINE="$(sqlite3 "$DB" "SELECT COALESCE(dynamic_vars_json,'{}') FROM task_items WHERE task_id='$TOOLS_ID' ORDER BY order_no LIMIT 1;")"
# This used to require the item's generic var map to hold exactly
# ["goal","last_sandbox_denied","sandbox_denied_count","last_sandbox_denial_reason"]. The same
# commit that retired `behavior.captures` (1b0937ca) moved those four into a typed carrier —
# `PipelineVariables::normalize_preserved_channels` migrates them out of the generic map, and the
# goal lives on `tasks.goal`. Measured after the change: both JSON columns are NULL and the run is
# otherwise identical, so the invariant now holds in its strongest form — there is no generic
# store to leak into.
#
# `-z` on the key lists would pass on a run where nothing happened, so it is paired with the goal:
# a task carrying the intent this gate supplied is a task that actually ran. The expected goal is
# the variable the task was created with, not a second copy of the string.
TASK_GOAL="$(sqlite3 "$DB" "SELECT COALESCE(goal,'') FROM tasks WHERE id='$TOOLS_ID';")"
TASK_VAR_KEYS="$(jq -r '(.vars // {}) | keys | join(",")' <<<"$TASK_PIPELINE")"
ITEM_VAR_KEYS="$(jq -r '(.vars // {}) | keys | join(",")' <<<"$ITEM_PIPELINE")"
if [[ "$TASK_GOAL" == "$PILOT_GOAL" && -z "$TASK_VAR_KEYS" && -z "$ITEM_VAR_KEYS" ]]; then
  pass "user intent is carried on the typed channel and the generic var store holds nothing"
else
  fail "residual channel check: goal='$TASK_GOAL' task_vars=[$TASK_VAR_KEYS] item_vars=[$ITEM_VAR_KEYS]"
fi

jq -n \
  --argjson legacy_effective_lines "$LEGACY_EFFECTIVE_LINES" \
  --argjson tool_effective_lines "$TOOL_EFFECTIVE_LINES" \
  --argjson legacy_coordination_lines "$LEGACY_COORDINATION_LINES" \
  --argjson tool_coordination_lines "$TOOL_COORDINATION_LINES" \
  --argjson reduction_percent "$REDUCTION_PERCENT" \
  --arg task_var_keys "$TASK_VAR_KEYS" \
  --arg item_var_keys "$ITEM_VAR_KEYS" \
  '{legacy:{effective_yaml_lines:$legacy_effective_lines,coordination_lines:$legacy_coordination_lines},tool:{effective_yaml_lines:$tool_effective_lines,coordination_lines:$tool_coordination_lines},coordination_reduction_percent:$reduction_percent,residual_typed_channels:[{key:"goal",carrier:"tasks.goal + PreservedExecutionChannels",source:"task_create",consumer:"prompt_context"},{key:"last_sandbox_denied",carrier:"PreservedExecutionChannels",source:"runner_safety",consumer:"subsequent_step_safety_context"},{key:"sandbox_denied_count",carrier:"PreservedExecutionChannels",source:"runner_safety",consumer:"subsequent_step_safety_context"},{key:"last_sandbox_denial_reason",carrier:"PreservedExecutionChannels",source:"runner_safety",consumer:"subsequent_step_safety_context"}],generic_pipeline_var_keys:{task:$task_var_keys,item:$item_var_keys}}' \
  > "$QA_ROOT/coordination-collapse-metrics.json"

if [[ "$FAIL" -ne 0 ]]; then
  echo "FR-118 QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "FR-118 QA: $PASS passed, 0 failed"
cat "$QA_ROOT/coordination-collapse-metrics.json"
