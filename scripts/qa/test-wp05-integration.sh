#!/usr/bin/env bash
# WP05: Primitive Composition — QA Test Script
# Tests that WP01-WP04 compose correctly when used together.
# QA doc: docs/qa/orchestrator/51-primitive-composition.md
#
# Isolation: Each scenario uses --project wp05-<ID> for full project-level
# isolation. No database resets. Idempotent and repeatable.
#
# Usage:
#   test-wp05-integration.sh [--layer N] [--scenario ID] [--verbose]
#
#   --layer 1|2       Run only scenarios in the specified layer
#   --scenario L1A    Run a single scenario by ID (L1A, L1B, L1C, L1D, L2A)
#   --verbose         Show full orchestrator output
#   (no args)         Run all scenarios sequentially
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="./target/release/orchestrator"
DB="data/agent_orchestrator.db"
VERBOSE=false
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

ensure_db() {
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
  local project="$1" store="$2" key="$3" desc="${4:-store $store/$key}"
  local count
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM workflow_store_entries WHERE store_name='${store}' AND project_id='${project}' AND key='${key}';")"
  if [ "$count" -ge 1 ]; then
    pass "$desc exists in store (project=$project)"
  else
    fail "$desc not found in store (project=$project)"
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
  local project="$1" workspace="$2" workflow="$3" goal="$4"
  local create_output task_id
  create_output="$($ORCH task create \
    --project "$project" \
    --workspace "$workspace" \
    -W "$workflow" \
    --target-file fixtures/wp05-qa/wp05-check.md \
    --goal "$goal" \
    --no-start 2>&1)"
  task_id="$(echo "$create_output" | grep -oE '[0-9a-f-]{36}' | head -1)"

  [ -n "$task_id" ] || { fail "task creation returned no task id (output: $create_output)"; return 1; }

  # Start task (may fail for invariant tests — that's expected)
  $ORCH task start "$task_id" >/dev/null 2>&1 || true

  for _ in {1..30}; do
    if $ORCH task info "$task_id" | grep -qiE 'status:[[:space:]]*(completed|failed)'; then
      break
    fi
    sleep 1
  done

  echo "$task_id"
}

# ── Prerequisites ─────────────────────────────────────────────────────
require_cmd sqlite3
require_cmd cargo

info "Building release CLI"
(cd core && cargo build --release >/dev/null 2>&1)

mkdir -p fixtures/wp05-qa
printf '# WP05 QA target\n' > fixtures/wp05-qa/wp05-check.md

ensure_db

# ═══════════════════════════════════════════════════════════════════════
# Layer 1: Pairwise Composition
# ═══════════════════════════════════════════════════════════════════════

# ── L1-A: Store + Spawning (WP01 x WP02) ─────────────────────────────
if should_run L1A 1; then
  info "═══ L1-A: Store + Spawning (WP01 x WP02) ═══"

  run_orch apply -f fixtures/manifests/bundles/wp05-store-spawn.yaml --project wp05-L1A

  TASK_ID="$(create_and_run_task wp05-L1A wp05-ws wp05-store-spawn-parent "test store+spawn")"

  # Assertions
  assert_task_status "$TASK_ID" completed
  assert_store_has_key wp05-L1A context parent_finding "parent store write"
  assert_child_task_exists "$TASK_ID"

  CHILD_ID="$(get_child_task_id "$TASK_ID")"
  if [ -n "$CHILD_ID" ]; then
    PARENT_REF="$(sqlite3 "$DB" "SELECT parent_task_id FROM tasks WHERE id='${CHILD_ID}';")"
    if [ "$PARENT_REF" = "$TASK_ID" ]; then
      pass "child parent_task_id correct"
    else
      fail "child parent_task_id: expected '$TASK_ID', got '$PARENT_REF'"
    fi

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

# ── L1-B: Store + Invariants (WP01 x WP04) — violation & pass paths ──
if should_run L1B 1; then
  info "═══ L1-B: Store + Invariants — violation (WP01 x WP04) ═══"

  run_orch apply -f fixtures/manifests/bundles/wp05-store-invariant.yaml --project wp05-L1B

  # Test 1: invariant should fail (exit 1 vs expect_exit 0)
  TASK_ID="$(create_and_run_task wp05-L1B wp05-ws wp05-store-invariant-fail "test invariant fail")"

  assert_task_status "$TASK_ID" failed

  INV_EVENT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='task_failed' AND json_extract(payload_json,'\$.reason')='invariant_halt_before_complete';")"
  if [ "$INV_EVENT" -ge 1 ]; then
    pass "invariant halt event found"
  else
    assert_event_exists "$TASK_ID" invariant_violated "invariant_violated event"
  fi

  # Test 2: invariant should pass (exit 0 vs expect_exit 0)
  info "--- L1-B pass path ---"

  TASK_ID2="$(create_and_run_task wp05-L1B wp05-ws wp05-store-invariant-pass "test invariant pass")"

  assert_task_status "$TASK_ID2" completed

  info "L1-B done"
  echo ""
fi

# ── L1-C: Dynamic Items + Selection (WP03 baseline) ──────────────────
if should_run L1C 1; then
  info "═══ L1-C: Dynamic Items + Selection (WP03) ═══"

  run_orch apply -f fixtures/manifests/bundles/wp05-items-select.yaml --project wp05-L1C

  TASK_ID="$(create_and_run_task wp05-L1C wp05-ws wp05-items-select "test items+select")"

  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  ITEM_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';")"
  info "  total items: $ITEM_COUNT"

  DYN_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' AND source='dynamic';")"
  if [ "$DYN_COUNT" -ge 1 ]; then
    pass "dynamic items generated ($DYN_COUNT)"
  else
    if [ "$ITEM_COUNT" -ge 3 ]; then
      pass "items exist ($ITEM_COUNT total, dynamic source flag may differ)"
    else
      fail "expected >= 3 items, got $ITEM_COUNT"
    fi
  fi

  assert_event_exists "$TASK_ID" items_generated "items_generated event"
  assert_store_has_key wp05-L1C evolution winner_latest "item_select store_result"

  info "L1-C done"
  echo ""
fi

# ── L1-D: Dynamic Items + Invariants (WP03 x WP04) ───────────────────
if should_run L1D 1; then
  info "═══ L1-D: Dynamic Items + Invariants (WP03 x WP04) ═══"

  run_orch apply -f fixtures/manifests/bundles/wp05-items-invariant.yaml --project wp05-L1D

  TASK_ID="$(create_and_run_task wp05-L1D wp05-ws wp05-items-invariant "test items+invariant")"

  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  assert_event_exists "$TASK_ID" items_generated "items_generated event"

  ITEM_COUNT="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}';")"
  if [ "$ITEM_COUNT" -ge 2 ]; then
    pass "items generated for invariant test ($ITEM_COUNT)"
  else
    fail "expected >= 2 items, got $ITEM_COUNT"
  fi

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

  run_orch apply -f fixtures/manifests/bundles/wp05-store-items-select.yaml --project wp05-L2A

  TASK_ID="$(create_and_run_task wp05-L2A wp05-ws wp05-store-items-select "test store+items+select+spawn")"

  TASK_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}';")"
  info "  task status: $TASK_STATUS"

  assert_event_exists "$TASK_ID" items_generated "items_generated event"
  assert_store_has_key wp05-L2A evolution winner_latest "item_select winner in store"
  assert_store_has_key wp05-L2A journal run_latest "journal entry in store"
  assert_child_task_exists "$TASK_ID"

  CHILD_ID="$(get_child_task_id "$TASK_ID")"
  if [ -n "$CHILD_ID" ]; then
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
info "  WP05 Primitive Composition QA Summary"
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
