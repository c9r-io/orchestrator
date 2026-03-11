#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

WORKERS=4
TASKS=20
TIMEOUT_SECS=180
QA_OUTPUT_JSON=0
QA_WORKSPACE=""
QA_PROJECT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --workers) WORKERS="$2"; shift 2 ;;
    --tasks) TASKS="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    --workspace) QA_WORKSPACE="$2"; shift 2 ;;
    --project) QA_PROJECT="$2"; shift 2 ;;
    --json) QA_OUTPUT_JSON=1; shift ;;
    -h|--help)
      qa_print_usage
      echo
      echo "Extra options:"
      echo "  --workers <n>       Daemon-embedded workers (default: 4)"
      echo "  --tasks <n>         Tasks to enqueue (default: 20)"
      echo "  --timeout-secs <n>  Max wait for drain (default: 180)"
      exit 0
      ;;
    *)
      qa_error "Unknown argument: $1"
      exit 1
      ;;
  esac
done
export QA_OUTPUT_JSON QA_WORKSPACE QA_PROJECT

REPO_ROOT="$(qa_repo_root)"
BINARY="$(qa_binary_path)"
DAEMON_BINARY="$REPO_ROOT/target/release/orchestratord"
qa_require_binary
if [[ ! -x "$DAEMON_BINARY" ]]; then
  qa_error "Binary not found: $DAEMON_BINARY"
  qa_error "Build it with: cargo build --release -p orchestratord"
  exit 2
fi
cd "$REPO_ROOT"

qa_resolve_project "qa-throughput"
qa_info "Applying fixture for throughput baseline..."
qa_recreate_project "qa_only"
qa_apply_fixture_additive "fixtures/manifests/bundles/output-formats.yaml"

DAEMON_LOG="$REPO_ROOT/data/worker-throughput-daemon.log"
qa_info "Starting daemon: workers=$WORKERS"
"$DAEMON_BINARY" --foreground --workers "$WORKERS" >"$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!
trap 'kill "$DAEMON_PID" >/dev/null 2>&1 || true; wait "$DAEMON_PID" >/dev/null 2>&1 || true' EXIT

for _ in $(seq 1 30); do
  if [[ -S "$REPO_ROOT/data/orchestrator.sock" ]]; then
    break
  fi
  sleep 1
done

if [[ ! -S "$REPO_ROOT/data/orchestrator.sock" ]]; then
  qa_error "Daemon socket was not created"
  exit 12
fi

qa_info "Enqueuing tasks: $TASKS"
for i in $(seq 1 "$TASKS"); do
  "$BINARY" task create \
    --project "$QA_PROJECT" \
    --workspace "$QA_WORKSPACE" \
    --name "throughput-$i-$(date +%s)" \
    --goal "throughput baseline" \
    >/dev/null
done

start_ts="$(perl -MTime::HiRes=time -e 'printf("%.0f\n",time()*1000)')"
deadline=$(( $(date +%s) + TIMEOUT_SECS ))
max_running=0

while true; do
  pending="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM tasks WHERE status='pending' AND project_id='${QA_PROJECT}';")"
  running="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM tasks WHERE status='running' AND project_id='${QA_PROJECT}';")"
  if [[ "$running" -gt "$max_running" ]]; then
    max_running="$running"
  fi
  if [[ "$pending" -eq 0 && "$running" -eq 0 ]]; then
    break
  fi
  if [[ "$(date +%s)" -ge "$deadline" ]]; then
    qa_error "Timeout waiting for queue drain"
    exit 10
  fi
  sleep 1
done

kill "$DAEMON_PID" >/dev/null 2>&1 || true
wait "$DAEMON_PID" >/dev/null 2>&1 || true
trap - EXIT
end_ts="$(perl -MTime::HiRes=time -e 'printf("%.0f\n",time()*1000)')"
duration_ms=$(( end_ts - start_ts ))

completed="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM tasks WHERE project_id='${QA_PROJECT}' AND status='completed';")"
failed="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT COUNT(*) FROM tasks WHERE project_id='${QA_PROJECT}' AND status='failed';")"
total_done=$(( completed + failed ))

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"project":"%s","workspace":"%s","workers":%s,"tasks":%s,"duration_ms":%s,"max_running":%s,"completed":%s,"failed":%s,"total_done":%s}\n' \
    "$QA_PROJECT" "$QA_WORKSPACE" "$WORKERS" "$TASKS" "$duration_ms" "$max_running" "$completed" "$failed" "$total_done"
else
  echo "project: $QA_PROJECT"
  echo "workspace: $QA_WORKSPACE"
  echo "workers: $WORKERS"
  echo "tasks: $TASKS"
  echo "duration_ms: $duration_ms"
  echo "max_running: $max_running"
  echo "completed: $completed"
  echo "failed: $failed"
  echo "total_done: $total_done"
fi

if [[ "$total_done" -lt "$TASKS" ]]; then
  qa_error "Not all tasks reached terminal state"
  exit 11
fi
