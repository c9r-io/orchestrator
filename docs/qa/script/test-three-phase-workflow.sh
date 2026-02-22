#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

if ! qa_parse_common_args "$@"; then
  qa_print_usage
  exit 1
fi

REPO_ROOT="$(qa_repo_root)"
BINARY="$(qa_binary_path)"
qa_require_binary
cd "$REPO_ROOT"

qa_info "Ensuring config is bootstrapped..."
"$BINARY" init --force 2>/dev/null || true
"$BINARY" config bootstrap --from fixtures/output-formats.yaml --force 2>/dev/null || { qa_error "Failed to bootstrap config"; exit 2; }

qa_info "========================================"
qa_info "TEST: Three-Phase Workflow (QA + Fix + Retest)"
qa_info "========================================"

TASK_NAME="three-phase-$(date +%s)"
CREATE_ARGS=(task create --name "$TASK_NAME" --goal "Test three-phase workflow" --no-start)
if [[ -n "${QA_WORKSPACE:-}" ]]; then
  CREATE_ARGS+=(--workspace "$QA_WORKSPACE")
fi

TASK_OUTPUT="$($BINARY "${CREATE_ARGS[@]}" 2>&1)"
TASK_ID="$(qa_extract_task_id "$TASK_OUTPUT")"
if [[ -z "$TASK_ID" ]]; then
  qa_error "Failed to parse task id from: $TASK_OUTPUT"
  exit 3
fi

"$BINARY" task start "$TASK_ID" >/dev/null 2>&1 || true
sleep 5

TASK_STATUS="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
PROGRESS="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Progress: [0-9/]+' | awk '{print $2}')"
PHASES="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" \
  "SELECT DISTINCT phase FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '$TASK_ID')" 2>/dev/null || true)"

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  phases_one_line="$(echo "$PHASES" | tr '\n' ',' | sed 's/,$//')"
  printf '{"task_id":"%s","status":"%s","progress":"%s","phases":"%s"}\n' \
    "$TASK_ID" "$TASK_STATUS" "$PROGRESS" "$phases_one_line"
else
  echo "Task ID: $TASK_ID"
  echo "Status: $TASK_STATUS"
  echo "Progress: $PROGRESS"
  echo "Phases: ${PHASES:-none}"
  echo "RESULT: COMPLETED"
fi
