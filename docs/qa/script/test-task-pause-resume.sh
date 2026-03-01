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
qa_apply_fixture_additive "fixtures/manifests/bundles/pause-resume-workflow.yaml"
qa_resolve_project "qa-pause-resume"
qa_recreate_project "qa_sleep"

qa_info "========================================"
qa_info "TEST: Task Pause and Resume"
qa_info "========================================"
qa_info "Project: $QA_PROJECT"
qa_info "Workspace: $QA_WORKSPACE"

TASK_NAME="pause-test-$(date +%s)"
CREATE_ARGS=(
  task create
  --name "$TASK_NAME"
  --goal "Test pause"
  --project "$QA_PROJECT"
  --workspace "$QA_WORKSPACE"
  --workflow qa_sleep
  --no-start
)

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

qa_info "Waiting for task to enter running state (up to 20s)..."
STATUS_BEFORE=""
for _ in $(seq 1 20); do
  STATUS_BEFORE="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}' || true)"
  if [[ "$STATUS_BEFORE" == "running" ]]; then
    break
  fi
  sleep 1
done

qa_info "Status before pause: $STATUS_BEFORE"
if [[ "$STATUS_BEFORE" != "running" ]]; then
  qa_error "Task did not reach running state before pause"
  exit 4
fi

qa_info "Pausing task..."
PAUSE_OUT="$($BINARY task pause "$TASK_ID" 2>&1 || true)"
qa_info "Pause output: $PAUSE_OUT"

sleep 1
STATUS_AFTER="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')"
qa_info "Status after pause: $STATUS_AFTER"

qa_info "Resuming task..."
"$BINARY" task resume "$TASK_ID" >/dev/null 2>&1 || true

qa_info "Waiting for terminal state after resume (up to 30s)..."
FINAL_STATUS=""
for _ in $(seq 1 30); do
  FINAL_STATUS="$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}' || true)"
  if [[ "$FINAL_STATUS" == "completed" ]] || [[ "$FINAL_STATUS" == "failed" ]]; then
    break
  fi
  sleep 1
done
PASS=0
if [[ "$STATUS_AFTER" == "paused" ]] && ([[ "$FINAL_STATUS" == "completed" ]] || [[ "$FINAL_STATUS" == "failed" ]]); then
  PASS=1
fi

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","status_before":"%s","status_after_pause":"%s","final_status":"%s","pass":%s}\n' \
    "$TASK_ID" "$STATUS_BEFORE" "$STATUS_AFTER" "$FINAL_STATUS" "$PASS"
else
  echo "Task ID: $TASK_ID"
  echo "Status before: $STATUS_BEFORE"
  echo "Status after pause: $STATUS_AFTER"
  echo "Final status: $FINAL_STATUS"
  if [[ "$PASS" -eq 1 ]]; then
    echo "RESULT: PASS"
  else
    echo "RESULT: FAIL"
  fi
fi

if [[ "$PASS" -ne 1 ]]; then
  exit 5
fi
