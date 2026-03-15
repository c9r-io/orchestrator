#!/usr/bin/env bash
# Launch a full-QA regression task and monitor it with periodic status updates.
# Usage: ./scripts/run-full-qa.sh
#
# Prerequisites:
#   - orchestratord daemon running
#   - Binary built: cargo build --release -p orchestratord -p orchestrator-cli
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

export ORCHESTRATOR_SOCKET=data/orchestrator.sock
CLI=./target/release/orchestrator

# ── Verify daemon is running ──
if ! pgrep -f orchestratord >/dev/null 2>&1; then
  echo "ERROR: orchestratord not running. Start it first."
  exit 1
fi

# ── Verify CLI connectivity ──
if ! "$CLI" task list >/dev/null 2>&1; then
  echo "ERROR: CLI cannot connect to daemon via $ORCHESTRATOR_SOCKET"
  exit 1
fi

# ── Load resources ──
echo "==> Loading workflow resources..."
"$CLI" init
"$CLI" apply -f docs/workflow/claude-secret.yaml   --project self-bootstrap
"$CLI" apply -f docs/workflow/minimax-secret.yaml   --project self-bootstrap
"$CLI" apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
"$CLI" apply -f docs/workflow/self-bootstrap.yaml   --project self-bootstrap
"$CLI" apply -f docs/workflow/full-qa.yaml          --project self-bootstrap

# ── Create task ──
echo "==> Creating full-qa-regression task..."
TASK_ID=$("$CLI" task create \
  -n "full-qa-regression" \
  -w full-qa -W full-qa \
  --project self-bootstrap \
  -g "对 docs/qa/ 下全部 QA 文档执行场景级回归测试，对失败项创建 ticket 并尝试修复，最终确保所有场景通过或明确记录未通过原因" \
  2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)

if [ -z "$TASK_ID" ]; then
  echo "ERROR: Failed to create task"
  exit 1
fi
echo "Task created: $TASK_ID"
echo ""

# ── Monitor loop ──
INTERVAL=${1:-30}  # poll every N seconds (default 30)
echo "==> Monitoring (interval=${INTERVAL}s). Press Ctrl-C to stop."
echo ""

while true; do
  TIMESTAMP=$(date '+%H:%M:%S')

  # Task summary
  INFO=$("$CLI" task info "$TASK_ID" 2>&1)
  STATUS=$(echo "$INFO" | grep "Status:" | awk '{print $2}')
  PROGRESS=$(echo "$INFO" | grep "Progress:" | awk '{print $2}')
  FAILED=$(echo "$INFO" | grep "Failed:" | awk '{print $2}')

  # Count actual completed runs (exit != -1)
  COMPLETED=$(echo "$INFO" | grep "exit=" | grep -v "exit=-1" | wc -l | tr -d ' ')
  RUNNING=$(echo "$INFO" | grep "exit=-1" | wc -l | tr -d ' ')
  FAILED_RUNS=$(echo "$INFO" | grep "exit=" | grep -v "exit=-1\|exit=0" | wc -l | tr -d ' ')

  # Ticket pollution check
  DOCS_TICKETS=$(find docs/ticket -name 'auto_*' 2>/dev/null | wc -l | tr -d ' ')
  FIX_TICKETS=$(find fixtures/ticket -name 'auto_*' 2>/dev/null | wc -l | tr -d ' ')

  printf "[%s] status=%s  completed=%s  running=%s  failed=%s  docs/ticket/auto=%s  fixtures/ticket/auto=%s\n" \
    "$TIMESTAMP" "$STATUS" "$COMPLETED" "$RUNNING" "$FAILED_RUNS" "$DOCS_TICKETS" "$FIX_TICKETS"

  if [ "$DOCS_TICKETS" -gt 0 ]; then
    echo "  ⚠ ALERT: auto_* tickets found in docs/ticket/ — fixture isolation may be broken!"
    find docs/ticket -name 'auto_*' -exec echo "    {}" \;
  fi

  # Exit if task is done
  if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ]; then
    echo ""
    echo "==> Task finished: $STATUS"
    echo "==> Final summary:"
    "$CLI" task info "$TASK_ID" | head -8
    echo ""
    echo "==> Trace:"
    "$CLI" task trace "$TASK_ID" 2>&1 | tail -20
    break
  fi

  sleep "$INTERVAL"
done
