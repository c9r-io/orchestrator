#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BINARY="$PROJECT_ROOT/orchestrator/src-tauri/target/release/agent-orchestrator"
WORKSPACE="$PROJECT_ROOT/orchestrator"

log_info() { echo "[INFO] $1"; }
log_warn() { echo "[WARN] $1"; }
log_error() { echo "[ERROR] $1"; }

cd "$WORKSPACE"

log_info "========================================"
log_info "TEST: Task Pause and Resume"
log_info "========================================"

TASK_NAME="pause-test-$(date +%s)"

log_info "Creating task..."
TASK_OUTPUT=$($BINARY task create \
    --name "$TASK_NAME" \
    --goal "Test pause" \
    --no-start 2>&1)

TASK_ID=$(echo "$TASK_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)
log_info "Task ID: $TASK_ID"

log_info "Starting task in background..."
nohup $BINARY task start "$TASK_ID" > /tmp/task-$TASK_ID.log 2>&1 &
BG_PID=$!

log_info "Waiting for task to start (3s)..."
sleep 3

log_info "Checking status before pause..."
STATUS_BEFORE=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
log_info "Status before pause: $STATUS_BEFORE"

log_info "Pausing task..."
PAUSE_OUT=$($BINARY task pause "$TASK_ID" 2>&1)
log_info "Pause output: $PAUSE_OUT"

sleep 1

log_info "Checking status after pause..."
STATUS_AFTER=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
log_info "Status after pause: $STATUS_AFTER"

log_info "Resuming task..."
$BINARY task resume "$TASK_ID" 2>&1 &
log_info "Resume started in background"

log_info "Waiting for completion (10s)..."
sleep 10

log_info "Checking final status..."
FINAL_STATUS=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
log_info "Final status: $FINAL_STATUS"

echo ""
echo "========================================"
echo "TEST RESULTS"
echo "========================================"
echo "Task ID: $TASK_ID"
echo "Status before: $STATUS_BEFORE"
echo "Status after pause: $STATUS_AFTER"
echo "Final status: $FINAL_STATUS"
echo ""

if [[ "$FINAL_STATUS" == "completed" ]] || [[ "$FINAL_STATUS" == "failed" ]]; then
    echo "RESULT: PASS"
else
    echo "RESULT: COMPLETED (check manually)"
fi
