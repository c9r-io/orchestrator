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

qa_info "Ensuring config is applied from manifest..."
qa_apply_fixture_additive "fixtures/manifests/bundles/retry-workflow.yaml"
qa_resolve_project "qa-retry"
qa_recreate_project "qa_only_fail"

qa_info "========================================"
qa_info "TEST: Task Retry"
qa_info "========================================"
qa_info "Project: $QA_PROJECT"
qa_info "Workspace: $QA_WORKSPACE"

TASK_NAME="retry-test-$(date +%s)"
CREATE_ARGS=(
  task create
  --name "$TASK_NAME"
  --goal "Test retry"
  --project "$QA_PROJECT"
  --workspace "$QA_WORKSPACE"
  --workflow qa_only_fail
  --no-start
)

qa_info "Creating task..."
TASK_OUTPUT="$($BINARY "${CREATE_ARGS[@]}" 2>&1)"
TASK_ID="$(qa_extract_task_id "$TASK_OUTPUT")"
if [[ -z "$TASK_ID" ]]; then
  qa_error "Failed to parse task id from: $TASK_OUTPUT"
  exit 3
fi

"$BINARY" task start "$TASK_ID" >/dev/null 2>&1 || true
sleep 3

TASK_STATUS="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
FAILED_ITEM="$($BINARY task info "$TASK_ID" -o json 2>/dev/null | jq -r '.items[] | select(.status == "qa_failed" or .status == "unresolved") | .id' 2>/dev/null | head -1 || true)"

if [[ -z "$FAILED_ITEM" ]]; then
  qa_error "No unresolved item found for retry"
  exit 4
fi

BEFORE_ROW="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT status || '|' || updated_at FROM task_items WHERE id='${FAILED_ITEM}' LIMIT 1;" 2>/dev/null || true)"
qa_info "Retrying failed item: $FAILED_ITEM"
"$BINARY" task retry "$FAILED_ITEM" >/dev/null 2>&1 || true
sleep 2
AFTER_ROW="$(sqlite3 "$REPO_ROOT/data/agent_orchestrator.db" "SELECT status || '|' || updated_at FROM task_items WHERE id='${FAILED_ITEM}' LIMIT 1;" 2>/dev/null || true)"
STATUS_AFTER="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
PASS=0
if [[ -n "$AFTER_ROW" ]] && [[ "$BEFORE_ROW" != "$AFTER_ROW" ]]; then
  PASS=1
fi

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","status":"%s","failed_item":"%s","before_item":"%s","after_item":"%s","status_after_retry":"%s","pass":%s}\n' \
    "$TASK_ID" "$TASK_STATUS" "$FAILED_ITEM" "$BEFORE_ROW" "$AFTER_ROW" "$STATUS_AFTER" "$PASS"
else
  echo "Task ID: $TASK_ID"
  echo "Status: $TASK_STATUS"
  echo "Failed Item: ${FAILED_ITEM:-none}"
  echo "Item before retry: $BEFORE_ROW"
  echo "Item after retry: $AFTER_ROW"
  echo "Status after retry: $STATUS_AFTER"
  if [[ "$PASS" -eq 1 ]]; then
    echo "RESULT: PASS"
  else
    echo "RESULT: FAIL"
  fi
fi

if [[ "$PASS" -ne 1 ]]; then
  exit 5
fi
