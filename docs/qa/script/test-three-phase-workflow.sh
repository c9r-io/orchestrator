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

qa_resolve_project "qa-three-phase"
qa_info "Ensuring config is applied from manifest..."
qa_recreate_project "qa_fix_retest_forced"
qa_apply_fixture_additive "fixtures/manifests/bundles/three-phase-forced.yaml"

qa_info "========================================"
qa_info "TEST: Three-Phase Workflow (QA + Fix + Retest)"
qa_info "========================================"
qa_info "Project: $QA_PROJECT"
qa_info "Workspace: $QA_WORKSPACE"

TASK_NAME="three-phase-$(date +%s)"
CREATE_ARGS=(
  task create
  --name "$TASK_NAME"
  --goal "Test three-phase workflow"
  --project "$QA_PROJECT"
  --workspace "$QA_WORKSPACE"
  --workflow qa_fix_retest_forced
  --no-start
)

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
PHASES_CSV="$(echo "$PHASES" | tr '\n' ',' | sed 's/,$//')"
PASS=0
if [[ "$PHASES_CSV" == *qa* ]] && [[ "$PHASES_CSV" == *fix* ]] && [[ "$PHASES_CSV" == *retest* ]]; then
  PASS=1
fi

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","status":"%s","progress":"%s","phases":"%s","pass":%s}\n' \
    "$TASK_ID" "$TASK_STATUS" "$PROGRESS" "$PHASES_CSV" "$PASS"
else
  echo "Task ID: $TASK_ID"
  echo "Status: $TASK_STATUS"
  echo "Progress: $PROGRESS"
  echo "Phases: ${PHASES_CSV:-none}"
  if [[ "$PASS" -eq 1 ]]; then
    echo "RESULT: PASS"
  else
    echo "RESULT: FAIL"
  fi
fi

if [[ "$PASS" -ne 1 ]]; then
  exit 5
fi
