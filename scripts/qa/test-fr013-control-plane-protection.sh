#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/release/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/release/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:51054}"
BURST_CALLS="${BURST_CALLS:-12}"
STREAM_CALLS="${STREAM_CALLS:-4}"
MIXED_CALLS="${MIXED_CALLS:-6}"

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

require_cmd sqlite3
require_cmd mktemp

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "release binaries not found; run: cargo build --release -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
WATCH_PIDS=""
DAEMON_PID=""

cleanup() {
  if [[ -n "$WATCH_PIDS" ]]; then
    for pid in $WATCH_PIDS; do
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    done
  fi
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT

export HOME="$QA_HOME"
mkdir -p "$QA_ROOT/data/control-plane" "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
cat > "$QA_ROOT/fixtures/qa/watch-hold.md" <<'MD'
# Watch Hold
MD

cat > "$QA_ROOT/data/control-plane/protection.yaml" <<'YAML'
defaults:
  read: { rate_per_sec: 2, burst: 2, max_in_flight: 4 }
  write: { rate_per_sec: 3, burst: 3, max_in_flight: 2 }
  stream: { rate_per_sec: 2, burst: 2, max_active_streams: 1 }
  admin: { rate_per_sec: 1, burst: 1, max_in_flight: 1 }
global:
  read: { rate_per_sec: 4, burst: 4, max_in_flight: 8 }
  write: { rate_per_sec: 6, burst: 6, max_in_flight: 4 }
  stream: { rate_per_sec: 4, burst: 4, max_active_streams: 1 }
  admin: { rate_per_sec: 2, burst: 2, max_in_flight: 2 }
overrides:
  TaskList:
    class: read
    subject: { rate_per_sec: 1, burst: 1, max_in_flight: 2 }
  TaskWatch:
    class: stream
    subject: { max_active_streams: 1 }
    global: { max_active_streams: 1 }
  Apply:
    class: write
    subject: { rate_per_sec: 1, burst: 1, max_in_flight: 1 }
YAML

echo "[fr013] starting secure daemon in $QA_ROOT"
(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"
sleep 3

echo "[fr013] applying fixture project"
"$ORCH" apply \
  --project qa-fr013-load \
  -f "$REPO_ROOT/fixtures/manifests/bundles/pause-resume-workflow.yaml" >/dev/null
sleep 2

TASK_CREATE_OUTPUT="$("$ORCH" task create \
  --project qa-fr013-load \
  --workflow qa_sleep \
  --name "watch-hold" \
  --goal "hold watch stream" \
  --no-start)"
TASK_ID="$(printf '%s\n' "$TASK_CREATE_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)"

if [[ -z "$TASK_ID" ]]; then
  echo "[fr013] failed to create task" >&2
  exit 1
fi

run_parallel() {
  local count="$1"
  shift
  local pids=()
  for _ in $(seq 1 "$count"); do
    ("$@" || true) &
    pids+=("$!")
  done
  for pid in "${pids[@]}"; do
    wait "$pid" || true
  done
}

echo "[fr013] read flood"
run_parallel "$BURST_CALLS" "$ORCH" task list -o json >/dev/null 2>&1 || true

echo "[fr013] stream flood"
"$ORCH" task watch "$TASK_ID" --interval 1 > "$QA_ROOT/first-watch.log" 2>&1 &
WATCH_PIDS="$!"
sleep 2
run_parallel "$STREAM_CALLS" "$ORCH" task watch "$TASK_ID" --interval 1 >/dev/null 2>&1 || true

echo "[fr013] mixed read/write flood"
run_parallel "$MIXED_CALLS" "$ORCH" task list -o json >/dev/null 2>&1 || true
run_parallel "$MIXED_CALLS" "$ORCH" apply \
  --project qa-fr013-load \
  -f "$REPO_ROOT/fixtures/manifests/bundles/pause-resume-workflow.yaml" >/dev/null 2>&1 || true

sleep 1

echo "[fr013] validating daemon health"
"$ORCH" debug >/dev/null

READ_REJECTIONS="$(sqlite3 "$QA_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM control_plane_audit WHERE rpc='TaskList' AND decision='rejected' AND reason_code='rate_limited';")"
STREAM_REJECTIONS="$(sqlite3 "$QA_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM control_plane_audit WHERE rpc='TaskWatch' AND decision='rejected' AND reason_code='stream_limit_exceeded';")"
WRITE_REJECTIONS="$(sqlite3 "$QA_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM control_plane_audit WHERE rpc='Apply' AND decision='rejected' AND reason_code IN ('rate_limited','concurrency_limited');")"

if [[ "$READ_REJECTIONS" -lt 1 ]]; then
  echo "[fr013] expected TaskList rate-limited audit rows" >&2
  exit 1
fi
if [[ "$STREAM_REJECTIONS" -lt 1 ]]; then
  echo "[fr013] expected TaskWatch stream-limit audit rows" >&2
  exit 1
fi
if [[ "$WRITE_REJECTIONS" -lt 1 ]]; then
  echo "[fr013] expected Apply protection audit rows" >&2
  exit 1
fi

echo "[fr013] recent control_plane_audit rows"
sqlite3 "$QA_ROOT/data/agent_orchestrator.db" \
  "SELECT rpc, traffic_class, limit_scope, decision, reason_code FROM control_plane_audit ORDER BY id DESC LIMIT 12;"

echo "[fr013] protection pressure checks passed"
