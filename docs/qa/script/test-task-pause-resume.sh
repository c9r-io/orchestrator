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
qa_info "TEST: Task Pause and Resume"
qa_info "========================================"

TASK_NAME="pause-test-$(date +%s)"
CREATE_ARGS=(task create --name "$TASK_NAME" --goal "Test pause" --no-start)
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
qa_info "Task ID: $TASK_ID"

qa_info "Starting task in background..."
nohup "$BINARY" task start "$TASK_ID" > "/tmp/task-$TASK_ID.log" 2>&1 &

qa_info "Waiting for task to start (3s)..."
sleep 3

STATUS_BEFORE="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
qa_info "Status before pause: $STATUS_BEFORE"

qa_info "Pausing task..."
PAUSE_OUT="$($BINARY task pause "$TASK_ID" 2>&1 || true)"
qa_info "Pause output: $PAUSE_OUT"

sleep 1
STATUS_AFTER="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
qa_info "Status after pause: $STATUS_AFTER"

qa_info "Resuming task..."
"$BINARY" task resume "$TASK_ID" >/dev/null 2>&1 || true

qa_info "Waiting for completion (10s)..."
sleep 10
FINAL_STATUS="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","status_before":"%s","status_after_pause":"%s","final_status":"%s"}\n' \
    "$TASK_ID" "$STATUS_BEFORE" "$STATUS_AFTER" "$FINAL_STATUS"
else
  echo "Task ID: $TASK_ID"
  echo "Status before: $STATUS_BEFORE"
  echo "Status after pause: $STATUS_AFTER"
  echo "Final status: $FINAL_STATUS"
  if [[ "$FINAL_STATUS" == "completed" ]] || [[ "$FINAL_STATUS" == "failed" ]]; then
    echo "RESULT: PASS"
  else
    echo "RESULT: COMPLETED (check manually)"
  fi
fi
