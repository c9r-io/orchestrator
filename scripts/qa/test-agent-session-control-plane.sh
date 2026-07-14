#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19102}"
PASS=0
FAIL=0
DAEMON_PID=""
SESSION_PROCESS_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq sqlite3 mktemp rg ps; do
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
PROJECT="qa-session-control"
DB="$QA_ROOT/data/agent_orchestrator.db"

stop_daemon() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
    DAEMON_PID=""
  fi
}

cleanup() {
  stop_daemon
  if [[ -n "$SESSION_PROCESS_PID" ]]; then
    kill "$SESSION_PROCESS_PID" 2>/dev/null || true
    wait "$SESSION_PROCESS_PID" 2>/dev/null || true
  fi
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

wait_for_daemon() {
  for _ in {1..80}; do
    "$ORCH" task list -o json >/dev/null 2>&1 && return 0
    sleep 0.25
  done
  return 1
}

start_tcp_daemon() {
  unset ORCHESTRATOR_SOCKET
  export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --bind "$BIND_ADDR" --webhook-bind none --workers 1 \
      > daemon-tcp.log 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"
  wait_for_daemon
}

start_read_only_daemon() {
  unset ORCHESTRATOR_CONTROL_PLANE_CONFIG
  export ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --uds-max-role read-only --webhook-bind none --workers 1 \
      > daemon-uds.log 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"
  wait_for_daemon
}

process_fingerprint() {
  local pid="$1"
  if [[ -r "/proc/$pid/stat" && -r /proc/sys/kernel/random/boot_id ]]; then
    local stat tail ticks boot_id
    stat="$(<"/proc/$pid/stat")"
    tail="${stat##*) }"
    ticks="$(awk '{print $20}' <<< "$tail")"
    boot_id="$(< /proc/sys/kernel/random/boot_id)"
    printf '%s:%s:%s' "$pid" "$boot_id" "$ticks"
  else
    local started
    started="$(ps -o lstart= -p "$pid" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    printf '%s:%s' "$pid" "$started"
  fi
}

mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# Deterministic session control target\n' > "$QA_ROOT/fixtures/qa/session-control.md"

if [[ "${SKIP_TARGETED_TESTS:-0}" == "1" ]] || \
   (cargo test -p agent-orchestrator populated_v28_sessions_upgrade_without_loss_or_state_ambiguity >/dev/null && \
   cargo test -p agent-orchestrator reconciliation_distinguishes_dead_process_from_live_identity_mismatch >/dev/null && \
   cargo test -p agent-orchestrator concurrent_writer_race_grants_exactly_one_client >/dev/null && \
   cargo test -p agent-orchestrator expired_writer_cleanup_never_resurrects_terminal_session >/dev/null); then
  pass "migration, reconciliation, lease cleanup, and writer-race regressions pass"
else
  fail "targeted migration or lifecycle regression failed"
fi

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"

if ! start_tcp_daemon; then
  echo "isolated TCP daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon-tcp.log" >&2
  exit 1
fi

"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/session-control-mock.yaml" >/dev/null
ENABLED_POLICY="$QA_ROOT/session-enabled.yaml"
awk '
  /^kind: RuntimePolicy$/ { print "apiVersion: orchestrator.dev/v2"; print; emit=1; next }
  emit { print }
' "$REPO_ROOT/fixtures/manifests/bundles/session-control-mock.yaml" > "$ENABLED_POLICY"
"$ORCH" apply --project _system -f "$ENABLED_POLICY" >/dev/null
CREATE_OUTPUT="$(
  cd "$QA_ROOT"
  "$ORCH" task create --project "$PROJECT" --workspace session-control-mock \
    --workflow session-control-mock --target-file fixtures/qa/session-control.md \
    --goal "hold an interactive FR-102 QA session" --no-start
)"
TASK_ID="$(printf '%s\n' "$CREATE_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)"
[[ -n "$TASK_ID" ]] || { echo "task creation returned no task id" >&2; exit 1; }
"$ORCH" task start "$TASK_ID" >/dev/null

SESSION_JSON="$QA_ROOT/session.json"
for _ in {1..80}; do
  "$ORCH" agent session list --task "$TASK_ID" -o json > "$SESSION_JSON"
  jq -e 'length == 1' "$SESSION_JSON" >/dev/null && break
  sleep 0.25
done
SESSION_ID="$(jq -r '.[0].session_id // empty' "$SESSION_JSON")"
[[ -n "$SESSION_ID" ]] || { echo "TTY session was not materialized" >&2; exit 1; }
ORIGINAL_PID="$(jq -r '.[0].pid' "$SESSION_JSON")"
ORIGINAL_FINGERPRINT="$(sqlite3 "$DB" "SELECT process_fingerprint FROM agent_sessions WHERE id='$SESSION_ID';")"

"$ORCH" agent session get "$SESSION_ID" -o json > "$QA_ROOT/public-session.json"
"$ORCH" agent session attach "$SESSION_ID" --mode reader --client-id reader-a >/dev/null
"$ORCH" agent session attach "$SESSION_ID" --mode reader --client-id reader-a >/dev/null
"$ORCH" agent session attach "$SESSION_ID" --mode reader --client-id reader-b >/dev/null
"$ORCH" agent session read "$SESSION_ID" --offset 0 --chunks-json > "$QA_ROOT/read-a.jsonl"
"$ORCH" agent session read "$SESSION_ID" --offset 0 --chunks-json > "$QA_ROOT/read-b.jsonl"
READER_A_ROWS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM session_attachments WHERE session_id='$SESSION_ID' AND client_id='reader-a' AND mode='reader' AND detached_at IS NULL;")"
if [[ "$READER_A_ROWS" == "1" ]] && \
   jq -e '.[0] | (has("input_fifo_path") or has("stdout_path") or has("process_fingerprint")) | not' "$QA_ROOT/public-session.json" >/dev/null && \
   [[ "$(jq -s '.[0].next_offset' "$QA_ROOT/read-a.jsonl")" == "$(jq -s '.[0].next_offset' "$QA_ROOT/read-b.jsonl")" ]]; then
  pass "public reads hide authority paths and reader offsets are independent and idempotent"
else
  fail "reader idempotency, offset isolation, or public-field boundary failed"
fi

set +e
"$ORCH" agent session attach "$SESSION_ID" --mode writer --client-id writer-a > "$QA_ROOT/writer-a.out" 2>&1 &
WRITER_A_PID=$!
"$ORCH" agent session attach "$SESSION_ID" --mode writer --client-id writer-b > "$QA_ROOT/writer-b.out" 2>&1 &
WRITER_B_PID=$!
wait "$WRITER_A_PID"; WRITER_A_STATUS=$?
wait "$WRITER_B_PID"; WRITER_B_STATUS=$?
set -e
if [[ "$WRITER_A_STATUS" -eq 0 && "$WRITER_B_STATUS" -ne 0 ]]; then
  WRITER="writer-a"; LOSER="writer-b"; WRITER_OUT="$QA_ROOT/writer-a.out"
elif [[ "$WRITER_B_STATUS" -eq 0 && "$WRITER_A_STATUS" -ne 0 ]]; then
  WRITER="writer-b"; LOSER="writer-a"; WRITER_OUT="$QA_ROOT/writer-b.out"
else
  WRITER=""; LOSER=""; WRITER_OUT="$QA_ROOT/writer-a.out"
fi
TOKEN="$(sed -n 's/.*fencing_token=\([0-9][0-9]*\).*/\1/p' "$WRITER_OUT" | tail -1)"
if [[ -z "$WRITER" || -z "$TOKEN" ]]; then
  echo "writer race did not produce exactly one parseable winner" >&2
  sed 's/^/  writer-a: /' "$QA_ROOT/writer-a.out" >&2
  sed 's/^/  writer-b: /' "$QA_ROOT/writer-b.out" >&2
  exit 1
fi
"$ORCH" agent session heartbeat "$SESSION_ID" --client-id "$WRITER" --fencing-token "$TOKEN" > "$QA_ROOT/heartbeat.out"
INPUT=$'FR102_ONCE\n'
"$ORCH" agent session send-input "$SESSION_ID" --client-id "$WRITER" --fencing-token "$TOKEN" \
  --idempotency-key fr102-once --text "$INPUT" > "$QA_ROOT/input-first.out"
"$ORCH" agent session send-input "$SESSION_ID" --client-id "$WRITER" --fencing-token "$TOKEN" \
  --idempotency-key fr102-once --text "$INPUT" > "$QA_ROOT/input-replay.out"
set +e
"$ORCH" agent session send-input "$SESSION_ID" --client-id "$WRITER" --fencing-token "$TOKEN" \
  --idempotency-key fr102-once --text $'FR102_CHANGED\n' > "$QA_ROOT/input-conflict.out" 2>&1
CONFLICT_STATUS=$?
set -e
STDOUT_PATH="$(sqlite3 "$DB" "SELECT stdout_path FROM agent_sessions WHERE id='$SESSION_ID';")"
for _ in {1..40}; do
  [[ -f "$STDOUT_PATH" ]] && [[ "$(rg -c '^mock:FR102_ONCE$' "$STDOUT_PATH" || true)" == "1" ]] && break
  sleep 0.25
done
if [[ -n "$WRITER" && -n "$TOKEN" && "$CONFLICT_STATUS" -ne 0 ]] && \
   [[ "$(<"$QA_ROOT/input-first.out")" == "accepted_bytes=11" ]] && \
   [[ "$(<"$QA_ROOT/input-replay.out")" == "accepted_bytes=11" ]] && \
   [[ "$(rg -c '^mock:FR102_ONCE$' "$STDOUT_PATH" || true)" == "1" ]]; then
  pass "one writer wins and identical input retry reports one atomic write"
else
  fail "writer exclusion or input idempotency contract failed"
fi

"$ORCH" agent session detach "$SESSION_ID" --mode writer --client-id "$WRITER" \
  --fencing-token "$TOKEN" --reason "rotate QA writer" >/dev/null
NEW_WRITER="$LOSER"
"$ORCH" agent session attach "$SESSION_ID" --mode writer --client-id "$NEW_WRITER" > "$QA_ROOT/writer-new.out"
NEW_TOKEN="$(sed -n 's/.*fencing_token=\([0-9][0-9]*\).*/\1/p' "$QA_ROOT/writer-new.out" | tail -1)"
set +e
"$ORCH" agent session send-input "$SESSION_ID" --client-id "$WRITER" --fencing-token "$TOKEN" \
  --idempotency-key fr102-stale --text $'STALE\n' > "$QA_ROOT/stale-input.out" 2>&1
STALE_INPUT_STATUS=$?
"$ORCH" agent session detach "$SESSION_ID" --mode writer --client-id "$WRITER" \
  --fencing-token "$TOKEN" --reason "stale detach" > "$QA_ROOT/stale-detach.out" 2>&1
STALE_DETACH_STATUS=$?
sqlite3 "$DB" "UPDATE agent_sessions SET process_fingerprint='stale-fingerprint' WHERE id='$SESSION_ID';"
"$ORCH" agent session send-input "$SESSION_ID" --client-id "$NEW_WRITER" --fencing-token "$NEW_TOKEN" \
  --idempotency-key fr102-pid-mismatch --text $'MISMATCH\n' > "$QA_ROOT/pid-input.out" 2>&1
PID_INPUT_STATUS=$?
VERSION="$("$ORCH" agent session get "$SESSION_ID" -o json | jq -r '.[0].state_version')"
"$ORCH" agent session close "$SESSION_ID" --reason "must reject PID mismatch" \
  --expected-version "$VERSION" --idempotency-key fr102-pid-close > "$QA_ROOT/pid-close.out" 2>&1
PID_CLOSE_STATUS=$?
set -e
PROCESS_LIVE=0
kill -0 "$ORIGINAL_PID" 2>/dev/null && PROCESS_LIVE=1
sqlite3 "$DB" "UPDATE agent_sessions SET process_fingerprint='$ORIGINAL_FINGERPRINT' WHERE id='$SESSION_ID';"
if [[ "$NEW_TOKEN" -gt "$TOKEN" && "$STALE_INPUT_STATUS" -ne 0 && "$STALE_DETACH_STATUS" -ne 0 && \
      "$PID_INPUT_STATUS" -ne 0 && "$PID_CLOSE_STATUS" -ne 0 && "$PROCESS_LIVE" -eq 1 ]]; then
  pass "monotonic fencing rejects stale owners and PID mismatch fails closed without signaling"
else
  fail "stale fencing or PID identity protection failed"
fi

"$ORCH" agent session detach "$SESSION_ID" --mode writer --client-id "$NEW_WRITER" \
  --fencing-token "$NEW_TOKEN" --reason "prepare feature-flag QA" >/dev/null
DISABLED_POLICY="$QA_ROOT/session-disabled.yaml"
sed 's/session_control_enabled: true/session_control_enabled: false/' \
  "$ENABLED_POLICY" > "$DISABLED_POLICY"
sleep 1
"$ORCH" apply --project _system -f "$DISABLED_POLICY" >/dev/null
sleep 0.5
set +e
"$ORCH" agent session attach "$SESSION_ID" --mode writer --client-id disabled-writer \
  > "$QA_ROOT/disabled.out" 2>&1
DISABLED_STATUS=$?
set -e
"$ORCH" agent session read "$SESSION_ID" --offset 0 --chunks-json > "$QA_ROOT/read-disabled.jsonl"
sleep 1
"$ORCH" apply --project _system -f "$ENABLED_POLICY" >/dev/null

stop_daemon
sleep 300 &
SESSION_PROCESS_PID=$!
RESTART_FIFO="$QA_ROOT/restart-input.fifo"
RESTART_TRANSCRIPT="$QA_ROOT/restart-transcript.log"
mkfifo "$RESTART_FIFO"
printf 'restart-evidence\n' > "$RESTART_TRANSCRIPT"
RESTART_FINGERPRINT="$(process_fingerprint "$SESSION_PROCESS_PID")"
sqlite3 "$DB" "UPDATE agent_sessions SET state='active',pid=$SESSION_PROCESS_PID,process_fingerprint='$RESTART_FINGERPRINT',input_fifo_path='$RESTART_FIFO',stdout_path='$RESTART_TRANSCRIPT',transcript_path='$RESTART_TRANSCRIPT',writer_client_id=NULL,writer_actor=NULL,writer_lease_expires_at=NULL,writer_last_heartbeat_at=NULL,ended_at=NULL WHERE id='$SESSION_ID';"

if ! start_read_only_daemon; then
  echo "isolated read-only UDS daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon-uds.log" >&2
  exit 1
fi
"$ORCH" agent session list --task "$TASK_ID" -o json > "$QA_ROOT/read-only-list.json"
"$ORCH" agent session get "$SESSION_ID" -o json > "$QA_ROOT/read-only-get.json"
"$ORCH" agent session read "$SESSION_ID" --offset 0 --chunks-json > "$QA_ROOT/read-only-read.jsonl"
"$ORCH" agent session attach "$SESSION_ID" --mode reader --client-id read-only-reader >/dev/null
set +e
"$ORCH" agent session attach "$SESSION_ID" --mode writer --client-id denied-writer > "$QA_ROOT/denied-writer.out" 2>&1
DENIED_WRITER_STATUS=$?
"$ORCH" agent session send-input "$SESSION_ID" --client-id denied-writer --fencing-token 1 \
  --idempotency-key denied-input --text $'DENIED\n' > "$QA_ROOT/denied-input.out" 2>&1
DENIED_INPUT_STATUS=$?
"$ORCH" agent session close "$SESSION_ID" --reason "denied close" \
  --idempotency-key denied-close > "$QA_ROOT/denied-close.out" 2>&1
DENIED_CLOSE_STATUS=$?
set -e
READ_ONLY_STATE="$(jq -r '.[0].state' "$QA_ROOT/read-only-get.json")"

stop_daemon
if ! start_tcp_daemon; then
  echo "isolated TCP daemon failed to restart" >&2
  sed 's/^/  /' "$QA_ROOT/daemon-tcp.log" >&2
  exit 1
fi
RESTART_STATE="$("$ORCH" agent session get "$SESSION_ID" -o json | jq -r '.[0].state')"
VERSION="$("$ORCH" agent session get "$SESSION_ID" -o json | jq -r '.[0].state_version')"
"$ORCH" agent session close "$SESSION_ID" --reason "complete isolated QA" \
  --expected-version "$VERSION" --idempotency-key fr102-final-close > "$QA_ROOT/final-close.out"
for _ in {1..40}; do
  PROCESS_STATE="$(ps -o stat= -p "$SESSION_PROCESS_PID" 2>/dev/null | tr -d '[:space:]' || true)"
  [[ -z "$PROCESS_STATE" || "$PROCESS_STATE" == Z* ]] && break
  sleep 0.25
done
PROCESS_CLOSED=0
PROCESS_STATE="$(ps -o stat= -p "$SESSION_PROCESS_PID" 2>/dev/null | tr -d '[:space:]' || true)"
[[ -z "$PROCESS_STATE" || "$PROCESS_STATE" == Z* ]] && PROCESS_CLOSED=1

"$ORCH" audit list --project "$PROJECT" -o json > "$QA_ROOT/audit.json"
MUTATION_LINKS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM session_control_actions WHERE actor='' OR request_id IS NULL OR request_id='';")"
if [[ "$DISABLED_STATUS" -ne 0 && "$READ_ONLY_STATE" == "detached" && "$RESTART_STATE" == "detached" && \
      "$DENIED_WRITER_STATUS" -ne 0 && "$DENIED_INPUT_STATUS" -ne 0 && "$DENIED_CLOSE_STATUS" -ne 0 && \
      "$PROCESS_CLOSED" -eq 1 && "$MUTATION_LINKS" == "0" ]] && \
   ! rg -q 'FR102_ONCE|FR102_CHANGED|STALE|MISMATCH|DENIED' \
      "$QA_ROOT/daemon-tcp.log" "$QA_ROOT/daemon-uds.log" "$QA_ROOT/audit.json"; then
  pass "feature flag, read-only RBAC, restart reconciliation, audit links, and secret boundaries hold"
else
  fail "feature flag, RBAC, restart, audit, or secret-boundary check failed"
fi

echo ""
echo "Agent session control-plane QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
