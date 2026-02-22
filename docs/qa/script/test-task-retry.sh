#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BINARY="$PROJECT_ROOT/core/target/release/agent-orchestrator"
WORKSPACE="$PROJECT_ROOT/orchestrator"

log_info() { echo "[INFO] $1"; }
log_warn() { echo "[WARN] $1"; }
log_error() { echo "[ERROR] $1"; }

cd "$WORKSPACE"

log_info "========================================"
log_info "TEST: Task Retry"
log_info "========================================"

TASK_NAME="retry-test-$(date +%s)"

log_info "Creating task..."
TASK_OUTPUT=$($BINARY task create \
    --name "$TASK_NAME" \
    --goal "Test retry" \
    --no-start 2>&1)

TASK_ID=$(echo "$TASK_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)
log_info "Task ID: $TASK_ID"

log_info "Starting task..."
$BINARY task start "$TASK_ID" 2>/dev/null || true

sleep 3

log_info "Checking task status..."
TASK_STATUS=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
log_info "Task status: $TASK_STATUS"

log_info "Getting failed items..."
FAILED_ITEMS=$($BINARY task info "$TASK_ID" -o json 2>/dev/null | jq -r '.items[] | select(.status == "unresolved") | .id' 2>/dev/null || echo "")

if [[ -z "$FAILED_ITEMS" ]]; then
    log_warn "No failed items found"
    ITEMS_STATUS=$($BINARY task info "$TASK_ID" -o json 2>/dev/null | jq -r '.items[].status' 2>/dev/null || echo "unknown")
    log_info "Item statuses: $ITEMS_STATUS"
else
    log_info "Found failed item: $FAILED_ITEMS"
    
    log_info "Retrying failed item..."
    $BINARY task retry "$FAILED_ITEMS" 2>&1 || log_warn "Retry may have failed"
    
    sleep 2
    
    log_info "Checking status after retry..."
    STATUS_AFTER=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
    log_info "Status after retry: $STATUS_AFTER"
fi

echo ""
echo "========================================"
echo "TEST RESULTS"
echo "========================================"
echo "Task ID: $TASK_ID"
echo "Status: $TASK_STATUS"
echo "Failed Items: ${FAILED_ITEMS:-none}"
echo "RESULT: COMPLETED"
