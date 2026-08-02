#!/usr/bin/env bash
#
# FR-159: interactive session process reclamation.
#
# Runs an isolated daemon on a non-standard port with its own data directory,
# drives a real mock session, and asserts that an unreachable session process is
# reclaimed at the OS level -- together with the negative fixtures that make
# those assertions mean something.
#
# Two shapes this script deliberately avoids:
#
#   * A `ps` that returned nothing and a machine running nothing produce the
#     same zero rows, and every count derived from them reads as clean. The
#     process table probe fails the run when it comes back empty.
#   * "The process is gone" is true before the feature exists if nothing ever
#     started it. Each reclamation assertion is paired with a run under
#     `session_reclaim_enabled: false`, where the same process must survive.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
PASS=0
FAIL=0
DAEMON_PID=""
TRACKED_PIDS=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq sqlite3 mktemp ps awk; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  (cd "$REPO_ROOT" && cargo build -p orchestratord -p orchestrator-cli >/dev/null)
fi
if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run without SKIP_BUILD or provide ORCH/ORCHD" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
PROJECT="qa-session-reclaim"
DB="$QA_ROOT/data/agent_orchestrator.db"

# Reads the process table once and refuses to proceed on an empty result.
#
# On any live machine this is never empty. Emptiness means the probe failed, and
# every orphan count below would then be fiction rather than evidence.
process_table() {
  local table
  table="$(ps -eo pid=,ppid=,command= 2>/dev/null || true)"
  if [[ -z "$table" ]]; then
    echo "process table probe returned no rows; the counts below would be fiction" >&2
    exit 1
  fi
  printf '%s\n' "$table"
}

process_alive() { kill -0 "$1" 2>/dev/null; }

track_pid() { [[ -n "$1" && "$1" != "0" && "$1" != "null" ]] && TRACKED_PIDS="$TRACKED_PIDS $1"; return 0; }

stop_daemon() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    local waited=0
    # `wait` is useless here: the daemon is a child of a subshell, not of this
    # shell, so `wait` on it returns immediately. Poll instead, then wait for
    # the PID file so the next start does not race this shutdown's cleanup.
    while process_alive "$DAEMON_PID" && (( waited < 100 )); do
      sleep 0.1; waited=$((waited + 1))
    done
    process_alive "$DAEMON_PID" && kill -KILL "$DAEMON_PID" 2>/dev/null || true
    waited=0
    while [[ -f "$QA_ROOT/data/daemon.pid" ]] && (( waited < 50 )); do
      sleep 0.1; waited=$((waited + 1))
    done
    DAEMON_PID=""
  fi
}

cleanup() {
  stop_daemon
  local pid
  for pid in $TRACKED_PIDS; do
    kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
  done
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
mkdir -p "$QA_ROOT/data" "$QA_ROOT/workspace"

# Sweep mock leftovers from earlier runs before adding to them.
#
# Without this the final count charges this run for a previous one's residue,
# and once anything is left behind every subsequent run fails on it. Restricted
# to ppid == 1: an orphan has no owner, whereas a mock still parented to a live
# daemon belongs to a concurrent run and killing it would break that run.
sweep_previous_residue() {
  local pid swept=0
  while read -r pid; do
    [[ -n "$pid" ]] || continue
    kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
    swept=$((swept + 1))
  done < <(process_table | awk '$2 == 1 && index($0, "mock:$line") > 0 && index($0, "sh -c") > 0 { print $1 }')
  (( swept > 0 )) && echo "  swept $swept orphaned mock process(es) left by an earlier run" >&2
  return 0
}
sweep_previous_residue

# Uses the UDS transport rather than TCP: the TCP listener requires an mTLS
# control-plane bootstrap that has nothing to do with what this script asserts.
start_daemon() {
  export ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --webhook-bind none --workers 1 \
      > daemon.log 2>&1 &
    echo $! > runner.pid
  )
  DAEMON_PID="$(cat "$QA_ROOT/runner.pid")"
  local attempt
  for attempt in {1..80}; do
    "$ORCH" task list -o json >/dev/null 2>&1 && return 0
    sleep 0.25
  done
  return 1
}

if ! start_daemon; then
  echo "isolated daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon.log" >&2
  exit 1
fi

apply_policy() {
  # Separate statements: in a single `local a=$1 b=$a`, bash expands `$a` before
  # the first assignment lands, which under `set -u` aborts as unbound.
  local reclaim="$1"
  local policy="$QA_ROOT/policy-$reclaim.yaml"
  cat > "$policy" <<YAML
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner:
    shell: /bin/sh
    shell_arg: -c
  resume:
    auto: false
  session_read_enabled: true
  session_control_enabled: true
  session_reclaim_enabled: $reclaim
YAML
  "$ORCH" apply --project _system -f "$policy" >/dev/null
}

# Starts a mock session and returns "<session_id> <pid>".
start_mock_session() {
  local task_output task_id session_json session_id pid attempt
  "$ORCH" apply --project "$PROJECT" \
    -f "$REPO_ROOT/fixtures/manifests/bundles/session-control-mock.yaml" >/dev/null
  mkdir -p "$QA_ROOT/docs/qa"
  echo "# reclaim fixture" > "$QA_ROOT/docs/qa/reclaim.md"
  task_output="$("$ORCH" task create --project "$PROJECT" --workspace session-control-mock \
    --workflow session-control-mock --target-file docs/qa/reclaim.md 2>&1)"
  task_id="$(grep -oE '[0-9a-f-]{36}' <<< "$task_output" || true)"
  task_id="${task_id%%$'\n'*}"
  [[ -n "$task_id" ]] || { echo "task creation returned no id: $task_output" >&2; return 1; }
  "$ORCH" task start "$task_id" >/dev/null
  session_json="$QA_ROOT/session.json"
  for attempt in {1..80}; do
    "$ORCH" agent session list --task "$task_id" -o json > "$session_json" 2>/dev/null || true
    if jq -e 'length >= 1 and (.[0].pid // 0) > 0' "$session_json" >/dev/null 2>&1; then break; fi
    sleep 0.25
  done
  session_id="$(jq -r '.[0].session_id // empty' "$session_json")"
  pid="$(jq -r '.[0].pid // 0' "$session_json")"
  [[ -n "$session_id" && "$pid" != "0" ]] || { echo "no session materialized" >&2; return 1; }
  track_pid "$pid"
  printf '%s %s\n' "$session_id" "$pid"
}

# Removes the session's input FIFO, which is the condition reconciliation treats
# as "this process can never be driven again".
break_transport() {
  local session_id="$1" fifo
  fifo="$(sqlite3 "$DB" "SELECT input_fifo_path FROM agent_sessions WHERE id='$session_id';")"
  [[ -n "$fifo" ]] || return 1
  rm -f "$fifo"
  printf '%s\n' "$fifo"
}

wait_for_exit() {
  local pid="$1" limit="${2:-40}" waited=0
  while process_alive "$pid" && (( waited < limit )); do sleep 0.5; waited=$((waited + 1)); done
  process_alive "$pid" && return 1 || return 0
}

echo "Scenario 1: an unreachable session process is reclaimed and the reclamation is recorded"
apply_policy true
read -r SESSION_ID SESSION_PID < <(start_mock_session)
SESSION_DIR="$(dirname "$(sqlite3 "$DB" "SELECT input_fifo_path FROM agent_sessions WHERE id='$SESSION_ID';")")"
if process_alive "$SESSION_PID"; then
  pass "mock session $SESSION_ID is running as pid $SESSION_PID"
else
  fail "mock session never started; every assertion below would be vacuous"
fi
break_transport "$SESSION_ID" >/dev/null
if wait_for_exit "$SESSION_PID" 40; then
  pass "session process was reclaimed after its transport disappeared"
else
  fail "session process $SESSION_PID survived reclamation"
fi
RECLAIM_EVENTS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE event_type='session_process_reclaimed' AND payload_json LIKE '%$SESSION_ID%';")"
if [[ "$RECLAIM_EVENTS" -ge 1 ]]; then
  pass "the reclamation emitted a session_process_reclaimed event"
else
  fail "no session_process_reclaimed event was recorded for $SESSION_ID"
fi
RECLAIM_OUTCOME="$(sqlite3 "$DB" "SELECT payload_json FROM events WHERE event_type='session_process_reclaimed' AND payload_json LIKE '%$SESSION_ID%' LIMIT 1;" | jq -r '.outcome // empty')"
if [[ "$RECLAIM_OUTCOME" == "reclaimed" ]]; then
  pass "the event records the outcome as reclaimed rather than refused"
else
  fail "reclamation event outcome was '$RECLAIM_OUTCOME', expected 'reclaimed'"
fi
if [[ -n "$SESSION_DIR" && ! -d "$SESSION_DIR" ]]; then
  pass "the reclaimed session's own directory was removed"
else
  fail "session directory '$SESSION_DIR' survived reclamation"
fi
if [[ -f "$DB" ]]; then
  pass "the database and the rest of the data directory are untouched"
else
  fail "reclamation removed more than the session's own directory"
fi

echo "Scenario 2 (negative): with reclamation disabled the same process survives"
apply_policy false
read -r SESSION_ID_OFF SESSION_PID_OFF < <(start_mock_session)
if process_alive "$SESSION_PID_OFF"; then
  pass "second mock session is running as pid $SESSION_PID_OFF"
else
  fail "second mock session never started"
fi
break_transport "$SESSION_ID_OFF" >/dev/null
# Give reconciliation more than two 10s cycles to act, then require survival.
sleep 25
if process_alive "$SESSION_PID_OFF"; then
  pass "session survived with session_reclaim_enabled=false, so scenario 1 is not vacuous"
else
  fail "session was reclaimed despite session_reclaim_enabled=false"
fi
STATE_OFF="$(sqlite3 "$DB" "SELECT state FROM agent_sessions WHERE id='$SESSION_ID_OFF';")"
if [[ "$STATE_OFF" == "failed" ]]; then
  pass "reconciliation still moved the row to failed; only the signal is gated"
else
  fail "expected state 'failed' with reclamation disabled, got '$STATE_OFF'"
fi

echo "Scenario 3 (negative): a mismatched fingerprint refuses and signals nothing"
apply_policy true
read -r SESSION_ID_MM SESSION_PID_MM < <(start_mock_session)
sqlite3 "$DB" "UPDATE agent_sessions SET process_fingerprint='deliberately-wrong' WHERE id='$SESSION_ID_MM';"
break_transport "$SESSION_ID_MM" >/dev/null
sleep 25
if process_alive "$SESSION_PID_MM"; then
  pass "a PID whose fingerprint does not match is never signalled"
else
  fail "a mismatched fingerprint was signalled anyway; the PID-reuse guard is not holding"
fi

echo "Scenario 4: graceful shutdown drains a healthy session (requirement 4)"
# A session whose transport is intact and which reconciliation would leave alone
# entirely. Nothing in the periodic path touches it, so if it dies here it died
# because the shutdown drain reclaimed it.
read -r SESSION_ID_DRAIN SESSION_PID_DRAIN < <(start_mock_session)
if process_alive "$SESSION_PID_DRAIN"; then
  pass "healthy session $SESSION_ID_DRAIN is running as pid $SESSION_PID_DRAIN"
else
  fail "healthy session never started; the drain assertion would be vacuous"
fi
stop_daemon
if wait_for_exit "$SESSION_PID_DRAIN" 30; then
  pass "graceful shutdown drained the healthy session's process group"
else
  fail "session process $SESSION_PID_DRAIN survived a graceful daemon shutdown"
fi

echo "Scenario 5: nothing beyond the deliberately spared fixtures is left running"
# Scenarios 2 and 3 exist to prove processes SURVIVE, so they are still alive by
# design and are reclaimed here before counting. Anything left after that was
# leaked rather than spared.
for spared in "$SESSION_PID_OFF" "$SESSION_PID_MM"; do
  kill -KILL -- "-$spared" 2>/dev/null || kill -KILL "$spared" 2>/dev/null || true
done
sleep 1
ORPHANS="$(process_table | awk '$2 == 1 && index($0, "mock:$line") > 0 && index($0, "sh -c") > 0' | wc -l | tr -d ' ')"
if [[ "$ORPHANS" == "0" ]]; then
  pass "no orphaned mock session processes remain"
else
  fail "$ORPHANS orphaned mock session process(es) remain"
fi

echo ""
echo "Session process reclamation QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
