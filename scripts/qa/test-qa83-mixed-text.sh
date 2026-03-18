#!/usr/bin/env bash
# QA83: generate_items mixed-text extraction - Test Runner
# Executes all 5 scenarios from docs/qa/orchestrator/83-generate-items-mixed-text-extraction.md
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="./target/release/orchestrator"
DB="data/agent_orchestrator.db"

PASS=0
FAIL=0

ensure_test_file() {
  local num="$1"
  mkdir -p fixtures/qa83-test
  if [ ! -f "fixtures/qa83-test/s${num}-test.md" ]; then
    printf '# QA83 S%s Test\n' "$num" > "fixtures/qa83-test/s${num}-test.md"
  fi
}

run_scenario() {
  local num="$1"
  local desc="$2"
  local ws="$3"
  local wf="$4"
  local expected_items="$5"
  local expect_event="$6"
  local fixture="fixtures/manifests/bundles/qa83-s${num}-$(echo "$desc" | tr ' ' '-').yaml"

  case $num in
    1) fixture="fixtures/manifests/bundles/qa83-s1-mixed-text.yaml" ;;
    2) fixture="fixtures/manifests/bundles/qa83-s2-fenced-block.yaml" ;;
    3) fixture="fixtures/manifests/bundles/qa83-s3-pure-json.yaml" ;;
    4) fixture="fixtures/manifests/bundles/qa83-s4-malformed-json.yaml" ;;
    5) fixture="fixtures/manifests/bundles/qa83-s5-multi-json.yaml" ;;
  esac

  local project="qa83-s${num}"
  local target="fixtures/qa83-test/s${num}-test.md"

  echo ""
  echo "═══ Scenario $num: $desc ═══"
  echo "  Fixture: $fixture"
  echo "  Expected items: $expected_items, Expect event: $expect_event"

  # Reset project
  $ORCH delete project/$project --force 2>/dev/null || true

  # Apply fixture
  local apply_out
  apply_out=$($ORCH apply -f "$fixture" --project $project 2>&1) || {
    echo "  FAIL: apply failed: $apply_out"
    FAIL=$((FAIL+1))
    return 1
  }
  echo "  Applied OK"

  # Ensure test target file
  ensure_test_file "$num"

  # Create task
  local task_out task_id
  task_out=$($ORCH task create \
    --project $project \
    --workspace "$ws" \
    -W "$wf" \
    --target-file "$target" \
    --goal "qa83 s$num $desc" \
    --no-start 2>&1) || {
    echo "  FAIL: task creation failed: $task_out"
    FAIL=$((FAIL+1))
    return 1
  }

  task_id=$(echo "$task_out" | grep -oE '[0-9a-f-]{36}' | head -1)
  if [ -z "$task_id" ]; then
    echo "  FAIL: no task ID returned"
    FAIL=$((FAIL+1))
    return 1
  fi
  echo "  Task ID: $task_id"

  # Start task
  $ORCH task start "$task_id" >/dev/null 2>&1 || true

  # Wait for completion
  for i in $(seq 1 30); do
    local status=$($ORCH task info "$task_id" 2>&1 | grep -i "status:" | awk '{print $2}')
    if [ "$status" = "completed" ] || [ "$status" = "failed" ]; then
      echo "  Task status: $status"
      break
    fi
    sleep 1
  done

  # Check items_generated event
  local event_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${task_id}' AND event_type='items_generated';")
  echo "  items_generated events: $event_count"

  if [ "$event_count" -ge 1 ]; then
    local event_payload=$(sqlite3 "$DB" "SELECT payload_json FROM events WHERE task_id='${task_id}' AND event_type='items_generated' LIMIT 1;")
    echo "  Event payload: $event_payload"
  fi

  # Check dynamic items
  local dyn_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${task_id}' AND source='dynamic';")
  local total_count=$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${task_id}';")
  echo "  Dynamic items: $dyn_count / Total items: $total_count"

  # Verify
  if [ "$expect_event" = "yes" ]; then
    if [ "$event_count" -ge 1 ]; then
      local actual_count=$(sqlite3 "$DB" "SELECT json_extract(payload_json,'$.count') FROM events WHERE task_id='${task_id}' AND event_type='items_generated' LIMIT 1;")
      if [ "$actual_count" = "$expected_items" ]; then
        echo "  PASS: Scenario $num (count=$actual_count)"
        PASS=$((PASS+1))
      else
        echo "  FAIL: Scenario $num — expected $expected_items items, got count=$actual_count"
        FAIL=$((FAIL+1))
      fi
    else
      echo "  FAIL: Scenario $num — expected items_generated event but got none"
      FAIL=$((FAIL+1))
    fi
  elif [ "$expect_event" = "no" ]; then
    if [ "$event_count" -eq 0 ]; then
      echo "  PASS: Scenario $num (graceful fallback, no items_generated event)"
      PASS=$((PASS+1))
    else
      echo "  FAIL: Scenario $num — expected NO items_generated event, got $event_count"
      FAIL=$((FAIL+1))
    fi
  fi
}

# Run all 5 scenarios
run_scenario 1 "Mixed text" "s1-mixed-text-test" "s1-mixed-text" 2 yes
run_scenario 2 "Fenced code block" "s2-fenced-block-test" "s2-fenced-block" 3 yes
run_scenario 3 "Pure JSON baseline" "s3-pure-json-test" "s3-pure-json" 1 yes
run_scenario 4 "Malformed JSON" "s4-malformed-json-test" "s4-malformed-json" 0 no
run_scenario 5 "Multiple JSON objects" "s5-multi-json-test" "s5-multi-json" 2 yes

echo ""
echo "═══════════════════════════════════"
echo "  QA83 Summary"
echo "  PASS: $PASS / 5"
echo "  FAIL: $FAIL / 5"
echo "═══════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
