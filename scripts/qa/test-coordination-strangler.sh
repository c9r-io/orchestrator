#!/usr/bin/env bash

set -euo pipefail

# FR-158: the freshness ledger is written by the manual-runbook gates, and two
# of them are invoked from ci-required gates. This gate refuses to run on a dirty
# worktree, so it reads the tree through the shared predicate that excludes that
# one file — otherwise it fails for the recorder's reason rather than its own.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

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

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"
# shellcheck source=../lib/gate_daemon.sh
. "$REPO_ROOT/scripts/lib/gate_daemon.sh"

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  if [[ "$FAIL" -gt 0 || "${KEEP_FR124_QA:-0}" == "1" ]]; then
    echo "FR-124 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    # A cleanup failure must not overwrite the gate's verdict, and must not be
    # silent either. Observed in run 30795701182: the assertions had already
    # reported "11 passed, 0 failed" when this `rm` raced a session child still
    # writing into $QA_ROOT/data, and `Directory not empty` turned the step red
    # with nothing in the log connecting it to what the gate tests.
    # `gate_daemon_stop` above confirms the daemon exited, not the process
    # group beneath it — FR-159's subject, surfacing here in the harness
    # rather than the product.
    #
    # So: settle and retry once, then say plainly what leaked. The gate's
    # subject is coordination strangler parity; a temp directory that outlives
    # it is a fact worth printing, not a verdict.
    if ! rm -rf "$QA_ROOT" "$QA_HOME" 2>/dev/null; then
      sleep 1
      rm -rf "$QA_ROOT" "$QA_HOME" 2>/dev/null ||
        echo "warning: $QA_ROOT or $QA_HOME survived cleanup; a child process is still writing there" >&2
    fi
  fi
  return 0
}
trap cleanup EXIT

for command in cargo git jq mktemp rg ruby sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR125_ALLOW_DIRTY:-${FR124_ALLOW_DIRTY:-0}}" != "1" &&
      -n "$(gate_runlog_worktree_status "$(git rev-parse --show-toplevel)")" ]]; then
  echo "coordination QA requires a clean worktree (or FR125_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

ruby scripts/qa/coordination-governance.rb --test-fixtures --require-complete >/dev/null
pass "inventory, ratchet, rejection, governance, and consumer fixtures pass"

cargo build -p orchestratord -p orchestrator-cli >/dev/null
cargo build -p orchestrator-runner --bin orch-mcp-tools >/dev/null
cargo test -p orchestrator-scheduler authenticated_host_executes_real_coordination_tools \
  >/dev/null
pass "typed coordination host and all real tool contracts pass offline"

TOOL_FIXTURE="$QA_ROOT/coordination-tools-only.yaml"
# Derived from $FIXTURE by keeping only the `-tools` Workflows. The selection
# names a suffix in a document this gate does not own: rename those workflows and
# the filter keeps every one of them out, leaving an empty file that everything
# downstream reads as "no contracts to check" — zero and N are the same exit
# code. fixture_produce refuses an empty result for that reason (FR-143).
if ! fixture_produce "coordination tool fixture" "$TOOL_FIXTURE" ruby -ryaml -e '
  source, output = ARGV
  documents = YAML.load_stream(File.read(source)).compact.select do |document|
    document["kind"] != "Workflow" ||
      document.dig("metadata", "name").to_s.end_with?("-tools")
  end
  File.open(output, "w") do |file|
    documents.each_with_index do |document, index|
      file.write("---\n") unless index.zero?
      file.write(YAML.dump(document).sub(/\A---\s*\n/, ""))
    end
  end
' "$FIXTURE" "$TOOL_FIXTURE"; then
  # Setup, not a case: every contract assertion below reads this file, so there
  # is no scope to skip. It stops here rather than continuing over an empty
  # fixture — but it stops by printing the summary line a reader stops at, which
  # is the whole difference from the abort this replaced. A run that ended early
  # in silence is indistinguishable from one that finished.
  echo "coordination strangler QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi

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
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
gate_daemon_wait_ready "$ORCH" || true
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,260p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  exit 1
fi

if "$ORCH" manifest validate -f "$FIXTURE" >"$QA_ROOT/legacy-validate.out" 2>&1; then
  fail "legacy capture/JSONPath parity fixture unexpectedly validates"
elif rg -q '\[legacy_(coordination|json_path)_removed\]' \
  "$QA_ROOT/legacy-validate.out"; then
  pass "legacy fixture is retained as rollback evidence and rejected by production validation"
else
  cat "$QA_ROOT/legacy-validate.out" >&2
  fail "legacy fixture failed without the stable retirement diagnostic"
fi

PROJECT="qa-coordination-strangler"
(
  cd "$QA_ROOT/workspace"
  "$ORCH" apply --project "$PROJECT" -f "$TOOL_FIXTURE" > "$QA_ROOT/apply.out"
)
if [[ "$(rg -c '^workflow/' "$QA_ROOT/apply.out")" -eq 7 ]]; then
  pass "all seven post-retirement tool workflows apply"
else
  cat "$QA_ROOT/apply.out" >&2
  fail "tool matrix did not apply all seven workflows"
fi

create_and_wait() {
  local workflow="$1"
  local task_id status
  # See FR-146: `| head -1` under pipefail kills `rg` and ends the gate. `rg -o` already
  # reads to EOF, so the first id comes off the captured text with no pipe at all.
  local created_ids
  created_ids="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace strangler-parity \
      --workflow "$workflow" --target-file docs/qa/pilot.md \
      --goal "FR-124 independent parity" --name "$workflow" --no-start |
      rg -o '[0-9a-f-]{36}'
  )"
  task_id="${created_ids%%$'\n'*}"
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
# A `case` lookup rather than `declare -A`: bash 3.2 has no associative arrays,
# and this repository runs its shell gates on macOS runners where 3.2 is the
# only bash present. Same mapping, same call sites.
production_workflow() {
  case "$1" in
    command) echo "command_rules" ;;
    qa) echo "qa_loop" ;;
    plan) echo "plan_execute" ;;
    fullqa) echo "full-qa" ;;
    bootstrap) echo "self-bootstrap" ;;
    promotion) echo "promotion" ;;
    evolution) echo "self-evolution" ;;
    *) echo "unknown case: $1" >&2; return 1 ;;
  esac
}
EVIDENCE='[]'
for name in ${CASES[@]+"${CASES[@]}"}; do
  workflow="$(production_workflow "$name")"
  tools="$(create_and_wait "parity-${name}-tools")"
  tools_id="${tools%%|*}"
  tools_status="${tools##*|}"
  if [[ "$tools_status" == "completed" ]]; then
    pass "$workflow post-retirement tool workflow completed"
  else
    fail "$workflow tool workflow ended as $tools_status"
  fi
  tool_events="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$tools_id' AND event_type IN ('driver_tool_use','driver_tool_result','coordination_tool_started','coordination_tool_completed');")"
  if [[ "$name" == "command" || "$tool_events" -ge 4 ]]; then
    pass "$workflow has typed event evidence"
  else
    fail "$workflow lacks complete typed event evidence"
  fi
  EVIDENCE="$(jq -c \
    --arg workflow "$workflow" \
    --arg tool_task "$tools_id" \
    --arg terminal "$tools_status" \
    --argjson tool_events "$tool_events" \
    '. + [{workflow:$workflow,tool_task:$tool_task,terminal:$terminal,typed_event_count:$tool_events}]' \
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
  echo "coordination strangler QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "coordination strangler QA: $PASS passed, 0 failed"
cat "$QA_ROOT/coordination-strangler-evidence.json"
