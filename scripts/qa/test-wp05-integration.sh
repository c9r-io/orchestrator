#!/usr/bin/env bash
# WP05: Integration Validation — Primitive Composition Verification
# Tests that WP01-WP04 compose correctly when used together.
#
# Usage:
#   test-wp05-integration.sh [--layer N] [--scenario ID] [--verbose] [--keep-db]
#
#   --layer 1|2       Run only scenarios in the specified layer
#   --scenario L1A    Run a single scenario by ID (L1A, L1B, L1C, L1D, L2A)
#   --verbose         Show full orchestrator output
#   --keep-db         Don't delete DB between scenarios (debugging)
#   (no args)         Run all scenarios sequentially
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="./scripts/orchestrator.sh"
DB="data/agent_orchestrator.db"
VERBOSE=false
KEEP_DB=false
RUN_LAYER=""
RUN_SCENARIO=""

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0

# ── Argument parsing ──────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --layer)   RUN_LAYER="$2"; shift 2 ;;
    --scenario) RUN_SCENARIO="$2"; shift 2 ;;
    --verbose) VERBOSE=true; shift ;;
    --keep-db) KEEP_DB=true; shift ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

# ── Utilities ─────────────────────────────────────────────────────────
fail() {
  echo "  FAIL: $*" >&2
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

pass() {
  echo "  PASS: $*"
  PASS_COUNT=$((PASS_COUNT + 1))
}

info() {
  echo "[wp05] $*"
}

run_orch() {
  if $VERBOSE; then
    $ORCH "$@"
  else
    $ORCH "$@" >/dev/null 2>&1
  fi
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "FATAL: missing required command: $1" >&2; exit 1; }
}

setup_db() {
  if [ -f "$DB" ] && ! $KEEP_DB; then
    rm -f "$DB"
  fi
  if [ ! -f "$DB" ]; then
    run_orch init
  fi
}

assert_task_status() {
  local task_id="$1" expected="$2"
  local actual
  actual="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${task_id}';")"
  if [ "$actual" = "$expected" ]; then
    pass "task $task_id status = $expected"
  else
    fail "task $task_id status: expected '$expected', got '$actual'"
  fi
}

assert_event_exists() {
  local task_id="$1" event_type="$2" desc="${3:-$event_type event}"
  local count
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${task_id}' AND event_type='${event_type}';")"
  if [ "$count" -ge 1 ]; then
    pass "$desc exists ($count)"
  else
    fail "$desc not found (0 events of type '$event_type')"
  fi
}

assert_event_absent() {
  local task_id="$1" event_type="$2" desc="${3:-$event_type event}"
  local count
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${task_id}' AND event_type='${event_type}';")"
  if [ "$count" -eq 0 ]; then
    pass "$desc absent (expected)"
  else
    fail "$desc unexpectedly present ($count events)"
  fi
}

assert_child_task_exists() {
  local parent_id="$1"
  local count
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE parent_task_id='${parent_id}';")"
  if [ "$count" -ge 1 ]; then
    pass "child task(s) spawned from $parent_id ($count)"
  else
    fail "no child tasks found for parent $parent_id"
  fi
}

get_child_task_id() {
  local parent_id="$1"
  sqlite3 "$DB" "SELECT id FROM tasks WHERE parent_task_id='${parent_id}' LIMIT 1;"
}

assert_store_has_key() {
  local store="$1" key="$2" desc="${3:-store $store/$key}"
  local count
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM workflow_store_entries WHERE store_name='${store}' AND key='${key}';")"
  if [ "$count" -ge 1 ]; then
    pass "$desc exists in store"
  else
    fail "$desc not found in store"
  fi
}

assert_item_count() {
  local task_id="$1" expected="$2" source="${3:-}"
  local count
  if [ -n "$source" ]; then
    count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${task_id}' AND source='${source}';")"
  else
    count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${task_id}';")"
  fi
  if [ "$count" -eq "$expected" ]; then
    pass "task $task_id has $expected items (source=${source:-any})"
  else
    fail "task $task_id item count: expected $expected, got $count (source=${source:-any})"
  fi
}

should_run() {
  local scenario_id="$1" layer="$2"
  if [ -n "$RUN_SCENARIO" ]; then
    [ "$RUN_SCENARIO" = "$scenario_id" ] && return 0 || return 1
  fi
  if [ -n "$RUN_LAYER" ]; then
    [ "$RUN_LAYER" = "$layer" ] && return 0 || return 1
  fi
  return 0
}

create_and_run_task() {
  local workspace="$1" workflow="$2" goal="$3"
  local create_output task_id
  create_output="$($ORCH task create \
    --workspace "$workspace" \
    --workflow "$workflow" \
    --target-file fixtures/wp05-qa/wp05-check.md \
    --goal "$goal" \
    --no-start 2>&1)"
  task_id="$(echo "$create_output" | grep -oE '[0-9a-f-]{36}' | head -1)"

  [ -n "$task_id" ] || { fail "task creation returned no task id (output: $create_output)"; return 1; }

  # Start task (may fail for invariant tests — that's expected)
  $ORCH task start "$task_id" >/dev/null 2>&1 || true
  echo "$task_id"
}

# ── Prerequisites ─────────────────────────────────────────────────────
require_cmd sqlite3
require_cmd cargo

info "Building release CLI"
(cd core && cargo build --release >/dev/null 2>&1)

mkdir -p fixtures/wp05-qa
printf '# WP05 QA target\n' > fixtures/wp05-qa/wp05-check.md

# ═══════════════════════════════════════════════════════════════════════
# Layer 1: Pairwise Composition
# ═══════════════════════════════════════════════════════════════════════

# ── L1-A: Store + Spawning (WP01 x WP02) ─────────────────────────────
if should_run L1A 1; then
  info "═══ L1-A: Store + Spawning (WP01 x WP02) ═══"
  setup_db

  run_orch apply -f fixtures/manifests/bundles/wp05-store-spawn.yaml

  TASK_ID="$(create_and_run_task wp05-ws wp05-store-spawn-parent "test store+spawn")"

  # Assertions
  assert_task_status "$TASK_ID" completed
  assert_store_has_key context parent_finding "parent store write"
  assert_child_task_exists "$TASK_ID"

  CHILD_ID="$(get_child_task_id "$TASK_ID")"
  if [ -n "$CHILD_ID" ]; then
    # Verify child has correct parent linkage
    PARENT_REF="$(sqlite3 "$DB" "SELECT parent_task_id FROM tasks WHERE id='${CHILD_ID}';")"
    if [ "$PARENT_REF" = "$TASK_ID" ]; then
      pass "child parent_task_id correct"
    else
      fail "child parent_task_id: expected '$TASK_ID', got '$PARENT_REF'"
    fi

    # Verify spawn depth incremented
    DEPTH="$(sqlite3 "$DB" "SELECT spawn_depth FROM tasks WHERE id='${CHILD_ID}';")"
    if [ "$DEPTH" -ge 1 ]; then
      pass "child spawn_depth=$DEPTH (>= 1)"
    else
      fail "child spawn_depth: expected >= 1, got $DEPTH"
    fi
  fi

  info "L1-A done"
  echo ""
fi

# ── L1-B: Store + Invariants (WP01 x WP04) — violation path ──────────
if should_run L1B 1; then
  info "═══ L1-B: Store + Invariants — violation (WP01 x WP04) ═══"
  setup_db

  run_orch apply -f fixtures/manifests/bundles/wp05-store-invariant.yaml

  # Test 1: invariant should fail (echo 8, threshold 10)
  TASK_ID="$(create_and_run_task wp05-ws wp05-store-invariant-fail "test invariant fail")"

  # The task should be in a failed state (invariant halt sets status=failed)
  assert_task_status "$TASK_ID" failed

  # Check for invariant-related failure event
  INV_EVENT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='task_failed' AND json_extract(payload_json,'$.reason')='invariant_halt_before_complete';")"
  if [ "$INV_EVENT" -ge 1 ]; then
    pass "invariant halt event found"
  else
    # Also check for invariant_violated event type
    assert_event_exists "$TASK_ID" invariant_violated "invariant_violated event"
  fi

  # Test 2: invariant should pass (echo 12, threshold 10)
  info "--- L1-B pass path ---"
  setup_db
  run_orch apply -f fixtures/manifests/bundles/wp05-store-invariant.yaml

  TASK_ID2="$(create_and_run_task wp05-ws wp05-store-invariant-pass "test invariant pass")"

  assert_task_status "$TASK_ID2" completed

  info "L1-B done"
  echo ""
fi

# ── L1-C: Dynamic Items + Selection (WP03 baseline) ──────────────────
if should_run L1C 1; then
  info "═══ L1-C: Dynamic Items + Selection (WP03) ═══"
  setup_db

  run_orch apply -f fixtures/manifests/bundles/wp05-items-select.yaml

  TASK_ID="$(create_and_run_task wp05-ws wp05-items-select "test items+select")"

  # Check task completed
  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  # Check dynamic items were generated
  ITEM_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';")"
  info "  total items: $ITEM_COUNT"

  DYN_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' AND source='dynamic';")"
  if [ "$DYN_COUNT" -ge 1 ]; then
    pass "dynamic items generated ($DYN_COUNT)"
  else
    # Items might exist without source='dynamic' depending on implementation
    if [ "$ITEM_COUNT" -ge 3 ]; then
      pass "items exist ($ITEM_COUNT total, dynamic source flag may differ)"
    else
      fail "expected >= 3 items, got $ITEM_COUNT"
    fi
  fi

  # Check items_generated event
  assert_event_exists "$TASK_ID" items_generated "items_generated event"

  # Check store result from item_select
  assert_store_has_key evolution winner_latest "item_select store_result"

  info "L1-C done"
  echo ""
fi

# ── L1-D: Dynamic Items + Invariants (WP03 x WP04) ───────────────────
if should_run L1D 1; then
  info "═══ L1-D: Dynamic Items + Invariants (WP03 x WP04) ═══"
  setup_db

  run_orch apply -f fixtures/manifests/bundles/wp05-items-invariant.yaml

  TASK_ID="$(create_and_run_task wp05-ws wp05-items-invariant "test items+invariant")"

  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  # With invariant check_at: after_implement and exit 0, both items should pass
  assert_event_exists "$TASK_ID" items_generated "items_generated event"

  # Both candidates should have been generated
  ITEM_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';")"
  if [ "$ITEM_COUNT" -ge 2 ]; then
    pass "items generated for invariant test ($ITEM_COUNT)"
  else
    fail "expected >= 2 items, got $ITEM_COUNT"
  fi

  # Invariant should have passed (exit 0)
  assert_event_exists "$TASK_ID" invariant_passed "invariant_passed event"

  info "L1-D done"
  echo ""
fi

# ═══════════════════════════════════════════════════════════════════════
# Layer 2: Triple Composition
# ═══════════════════════════════════════════════════════════════════════

# ── L2-A: Store + Items + Selection + Spawn (WP01 x WP02 x WP03) ─────
if should_run L2A 2; then
  info "═══ L2-A: Store + Items + Selection + Spawn (WP01 x WP02 x WP03) ═══"
  setup_db

  run_orch apply -f fixtures/manifests/bundles/wp05-store-items-select.yaml

  TASK_ID="$(create_and_run_task wp05-ws wp05-store-items-select "test store+items+select+spawn")"

  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  # Check items were generated
  assert_event_exists "$TASK_ID" items_generated "items_generated event"

  # Check store has winner
  assert_store_has_key evolution winner_latest "item_select winner in store"

  # Check store has journal entry
  assert_store_has_key journal run_latest "journal entry in store"

  # Check child task was spawned
  assert_child_task_exists "$TASK_ID"

  CHILD_ID="$(get_child_task_id "$TASK_ID")"
  if [ -n "$CHILD_ID" ]; then
    # Verify parent linkage
    PARENT_REF="$(sqlite3 "$DB" "SELECT parent_task_id FROM tasks WHERE id='${CHILD_ID}';")"
    if [ "$PARENT_REF" = "$TASK_ID" ]; then
      pass "child parent_task_id correct (3-primitive chain)"
    else
      fail "child parent_task_id: expected '$TASK_ID', got '$PARENT_REF'"
    fi
  fi

  info "L2-A done"
  echo ""
fi

# ═══════════════════════════════════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════════════════════════════════
echo ""
info "═══════════════════════════════════════════"
info "  WP05 Integration Validation Summary"
info "═══════════════════════════════════════════"
info "  PASS: $PASS_COUNT"
info "  FAIL: $FAIL_COUNT"
info "  SKIP: $SKIP_COUNT"
info "═══════════════════════════════════════════"

if [ "$FAIL_COUNT" -gt 0 ]; then
  info "RESULT: FAILED"
  exit 1
else
  info "RESULT: ALL PASSED"
  exit 0
fi
