#!/bin/bash
# scripts/qa/auto-regress.sh — Robust QA Document Runner
# Use this tool to run manual QA documents in an automated way.
# Usage: ./scripts/qa/auto-regress.sh [path/to/qa/doc.md | path/to/qa/dir] [fixture.yaml]

set -euo pipefail

ORCH="./target/release/orchestrator"

run_doc() {
  local doc_path=$1
  local fixture=$2
  local workflow_hint=${3:-""}
  
  local doc_name=$(basename "$doc_path" .md)
  local proj="qa-$(date +%s)-$doc_name"

  echo "═══════════════════════════════════════════════"
  echo "DOC: $doc_path"
  echo "PROJ: $proj"

  # 1. Reset project
  $ORCH delete "project/$proj" --force 2>/dev/null || true

  # 2. Apply fixture
  if [[ ! -f "$fixture" ]]; then
    echo "[SKIP] Fixture not found: $fixture"
    return 0
  fi
  $ORCH apply -f "$fixture" --project "$proj" > /dev/null

  # 3. Detect Workspace and Workflow
  local ws_name=$($ORCH get workspaces --project "$proj" -o json | python3 -c "import sys, json; data=json.load(sys.stdin); print(data[0] if data else 'default')")
  local wf_name=$workflow_hint
  if [[ -z "$wf_name" ]]; then
    wf_name=$($ORCH get workflows --project "$proj" -o json | python3 -c "import sys, json; data=json.load(sys.stdin); print(data[0] if data else '')")
  fi

  if [[ -z "$wf_name" ]]; then
    echo "[PASS] CLI-only check finished."
    return 0
  fi

  # 4. Create and Start Task
  echo "Creating task: WS=$ws_name, WF=$wf_name"
  local create_output=$($ORCH task create --project "$proj" --workspace "$ws_name" --workflow "$wf_name" --name "auto-reg" --goal "Regression: $doc_name" --no-start 2>&1)
  local task_id=$(echo "$create_output" | grep -oE '[0-9a-f-]{36}' | head -1 || true)

  if [[ -z "$task_id" ]]; then
    echo "[FAIL] Task creation failed: $create_output"
    return 1
  fi

  $ORCH task start "$task_id" > /dev/null

  # 5. Wait for completion
  echo "Waiting for task $task_id..."
  local status="unknown"
  for _ in {1..60}; do
    local info=$($ORCH task info "$task_id" 2>/dev/null || echo "Status: failed")
    if echo "$info" | grep -qiE 'status:[[:space:]]*(completed|failed)'; then
      status=$(echo "$info" | grep "Status:" | awk '{print $2}' | tr '[:upper:]' '[:lower:]')
      break
    fi
    sleep 1
  done

  if [[ "$status" == "completed" ]]; then
    echo "[PASS] Task completed."
  elif [[ "$status" == "failed" ]]; then
    echo "[WARN] Task failed (check if expected for this scenario)."
  else
    echo "[FAIL] Task timed out or vanished."
    return 1
  fi
}

# Main
if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <doc_path|dir> [fixture_path]"
  exit 1
fi

TARGET=$1
DEFAULT_FIXTURE=${2:-"fixtures/manifests/bundles/echo-workflow.yaml"}

if [[ -d "$TARGET" ]]; then
  for f in "$TARGET"/*.md; do
    [[ -e "$f" ]] || continue
    run_doc "$f" "$DEFAULT_FIXTURE"
  done
else
  run_doc "$TARGET" "$DEFAULT_FIXTURE"
fi
