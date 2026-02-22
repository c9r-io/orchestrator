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

qa_info "========================================"
qa_info "TEST: Task Retry"
qa_info "========================================"

TASK_NAME="retry-test-$(date +%s)"
CREATE_ARGS=(task create --name "$TASK_NAME" --goal "Test retry" --no-start)
if [[ -n "${QA_WORKSPACE:-}" ]]; then
  CREATE_ARGS+=(--workspace "$QA_WORKSPACE")
fi

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
FAILED_ITEMS="$($BINARY task info "$TASK_ID" -o json 2>/dev/null | jq -r '.items[] | select(.status == "unresolved") | .id' 2>/dev/null || true)"

if [[ -n "$FAILED_ITEMS" ]]; then
  qa_info "Retrying failed item: $FAILED_ITEMS"
  "$BINARY" task retry "$FAILED_ITEMS" >/dev/null 2>&1 || true
  sleep 2
fi

STATUS_AFTER="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","status":"%s","failed_item":"%s","status_after_retry":"%s"}\n' \
    "$TASK_ID" "$TASK_STATUS" "${FAILED_ITEMS:-}" "$STATUS_AFTER"
else
  echo "Task ID: $TASK_ID"
  echo "Status: $TASK_STATUS"
  echo "Failed Items: ${FAILED_ITEMS:-none}"
  echo "Status after retry: $STATUS_AFTER"
  echo "RESULT: COMPLETED"
fi
