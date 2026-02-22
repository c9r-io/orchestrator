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
log_info "TEST: Three-Phase Workflow (QA + Fix + Retest)"
log_info "========================================"

TASK_NAME="three-phase-$(date +%s)"

log_info "Creating task..."
TASK_OUTPUT=$($BINARY task create \
    --name "$TASK_NAME" \
    --goal "Test three-phase workflow" \
    --no-start 2>&1)

TASK_ID=$(echo "$TASK_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)
log_info "Task ID: $TASK_ID"

log_info "Starting task execution..."
$BINARY task start "$TASK_ID" 2>/dev/null || true

log_info "Waiting for execution (5s)..."
sleep 5

log_info "Checking task status..."
TASK_STATUS=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Status: [a-z]+' | awk '{print $2}')
PROGRESS=$($BINARY task info "$TASK_ID" 2>/dev/null | grep -oE 'Progress: [0-9/]+' | awk '{print $2}')
log_info "Task status: $TASK_STATUS"
log_info "Progress: $PROGRESS"

log_info "Checking phases executed..."
PHASES=$(sqlite3 "$WORKSPACE/data/agent_orchestrator.db" \
    "SELECT DISTINCT phase FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '$TASK_ID')" 2>/dev/null || echo "none")
log_info "Phases: ${PHASES:-none}"

echo ""
echo "========================================"
echo "TEST RESULTS"
echo "========================================"
echo "Task ID: $TASK_ID"
echo "Status: $TASK_STATUS"
echo "Progress: $PROGRESS"
echo "Phases: ${PHASES:-none}"
echo ""
echo "RESULT: COMPLETED"
