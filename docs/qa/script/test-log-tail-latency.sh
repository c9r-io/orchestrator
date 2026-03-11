#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

APPEND_LINES=80000
SAMPLES=3
TAIL=1

QA_OUTPUT_JSON=0
QA_WORKSPACE=""
QA_PROJECT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --append-lines) APPEND_LINES="$2"; shift 2 ;;
    --samples) SAMPLES="$2"; shift 2 ;;
    --tail) TAIL="$2"; shift 2 ;;
    --workspace) QA_WORKSPACE="$2"; shift 2 ;;
    --project) QA_PROJECT="$2"; shift 2 ;;
    --json) QA_OUTPUT_JSON=1; shift ;;
    -h|--help)
      qa_print_usage
      echo
      echo "Extra options:"
      echo "  --append-lines <n>  Lines appended to a run log (default: 80000)"
      echo "  --samples <n>       Latency samples count (default: 3)"
      echo "  --tail <n>          task logs --tail value (default: 1)"
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

ms_now() {
  perl -MTime::HiRes=time -e 'printf("%.0f\n",time()*1000)'
}

qa_resolve_project "qa-logtail"
qa_info "Applying fixture for log tail latency baseline..."
qa_recreate_project "qa_only"
qa_apply_fixture_additive "fixtures/manifests/bundles/output-formats.yaml"

TASK_ID="$("$BINARY" task create \
  --project "$QA_PROJECT" \
  --workspace "$QA_WORKSPACE" \
  --name "log-tail-latency-$(date +%s)" \
  --goal "tail latency baseline" \
  --no-start | grep -oE '[0-9a-f-]{36}' | head -1)"
if [[ -z "$TASK_ID" ]]; then
  qa_error "Failed to create task"
  exit 2
fi
"$BINARY" task start "$TASK_ID" >/dev/null 2>&1 || true

# Wait for command_runs entry to appear
for _ in $(seq 1 10); do
  RUN_STDOUT="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT stdout_path FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 1;" 2>/dev/null)"
  if [[ -n "$RUN_STDOUT" && -f "$RUN_STDOUT" ]]; then
    break
  fi
  sleep 1
done

if [[ -z "$RUN_STDOUT" || ! -f "$RUN_STDOUT" ]]; then
  qa_error "Failed to resolve run stdout path after wait"
  exit 3
fi

qa_info "Appending $APPEND_LINES lines to $RUN_STDOUT"
seq 1 "$APPEND_LINES" | sed 's/^/tail-latency-/' >> "$RUN_STDOUT"

sum=0
min_ms=999999999
max_ms=0
for _ in $(seq 1 "$SAMPLES"); do
  s="$(ms_now)"
  "$BINARY" task logs "$TASK_ID" --tail "$TAIL" >/dev/null
  e="$(ms_now)"
  d=$((e - s))
  sum=$((sum + d))
  if [[ "$d" -lt "$min_ms" ]]; then min_ms="$d"; fi
  if [[ "$d" -gt "$max_ms" ]]; then max_ms="$d"; fi
done
avg_ms=$((sum / SAMPLES))

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","append_lines":%s,"samples":%s,"tail":%s,"avg_ms":%s,"min_ms":%s,"max_ms":%s}\n' \
    "$TASK_ID" "$APPEND_LINES" "$SAMPLES" "$TAIL" "$avg_ms" "$min_ms" "$max_ms"
else
  echo "task_id: $TASK_ID"
  echo "append_lines: $APPEND_LINES"
  echo "samples: $SAMPLES"
  echo "tail: $TAIL"
  echo "avg_ms: $avg_ms"
  echo "min_ms: $min_ms"
  echo "max_ms: $max_ms"
fi
