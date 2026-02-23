#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

WORKERS=4
TASKS=20
POLL_MS=200
TIMEOUT_SECS=180
QA_OUTPUT_JSON=0
QA_WORKSPACE=""
QA_PROJECT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --workers) WORKERS="$2"; shift 2 ;;
    --tasks) TASKS="$2"; shift 2 ;;
    --poll-ms) POLL_MS="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    --workspace) QA_WORKSPACE="$2"; shift 2 ;;
    --project) QA_PROJECT="$2"; shift 2 ;;
    --json) QA_OUTPUT_JSON=1; shift ;;
    -h|--help)
      qa_print_usage
      echo
      echo "Extra options:"
      echo "  --workers <n>       Worker consumers (default: 4)"
      echo "  --tasks <n>         Detached tasks to enqueue (default: 20)"
      echo "  --poll-ms <ms>      Worker poll interval (default: 200)"
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
qa_require_binary
cd "$REPO_ROOT"

qa_info "Applying fixture for throughput baseline..."
"$BINARY" init --force >/dev/null 2>&1 || true
"$BINARY" db reset --force --include-config >/dev/null 2>&1 || true
"$BINARY" apply -f fixtures/manifests/bundles/output-formats.yaml >/dev/null
DEFAULT_WORKFLOW="$("$BINARY" manifest export -o json 2>/dev/null | jq -r 'first(.[] | select(.kind=="Defaults") | .spec.workflow) // "qa_only"' 2>/dev/null || echo "qa_only")"
if [[ -z "$DEFAULT_WORKFLOW" || "$DEFAULT_WORKFLOW" == "null" ]]; then
  DEFAULT_WORKFLOW="qa_only"
fi
qa_resolve_project "qa-throughput"
qa_prepare_project "$DEFAULT_WORKFLOW"
qa_reset_project_data
qa_prepare_project "$DEFAULT_WORKFLOW"

qa_info "Enqueuing detached tasks: $TASKS"
for i in $(seq 1 "$TASKS"); do
  "$BINARY" task create \
    --project "$QA_PROJECT" \
    --workspace "$QA_WORKSPACE" \
    --name "throughput-$i-$(date +%s)" \
    --goal "throughput baseline" \
    --detach >/dev/null
done

WORKER_LOG="$REPO_ROOT/data/worker-throughput.log"
qa_info "Starting worker: workers=$WORKERS poll_ms=$POLL_MS"
"$BINARY" task worker start --poll-ms "$POLL_MS" --workers "$WORKERS" >"$WORKER_LOG" 2>&1 &
WORKER_PID=$!
trap '"$BINARY" task worker stop >/dev/null 2>&1 || true; kill "$WORKER_PID" >/dev/null 2>&1 || true' EXIT

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

"$BINARY" task worker stop >/dev/null 2>&1 || true
wait "$WORKER_PID" || true
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
