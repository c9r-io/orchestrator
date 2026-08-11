#!/usr/bin/env bash
# WP05: Primitive Composition — QA Test Script
# Tests that WP01-WP04 compose correctly when used together.
# QA doc: docs/qa/orchestrator/51-primitive-composition.md
#
# Isolation: a throwaway ORCHESTRATORD_DATA_DIR and a daemon this script starts
# and reaps; each scenario additionally uses --project wp05-<ID>. The
# developer's runtime root at ~/.orchestratord is never opened.
#
# FR-149: L1-C, L1-D and L2-A were removed. They drove `generate_items` and
# `item_select`, which DD-137 (1b0937ca, 2026-07-25) retired, and their bundles
# were rejected at `apply`. What survives is what still exists: Store x Spawning
# and Store x Invariants, both on self-contained `command:` steps, so no
# provider is reachable.
#
# Usage:
#   test-wp05-integration.sh [--layer N] [--scenario ID] [--verbose]
#
#   --layer 1         Run only scenarios in the specified layer (only 1 remains)
#   --scenario L1A    Run a single scenario by ID (L1A, L1B)
#   --verbose         Show full orchestrator output
#   (no args)         Run all scenarios sequentially
#
# A selection that matches no scenario is a failure, not a silent exit 0
# (§4.4 shape 5: zero iterations and N passing iterations must not look alike).
#
# FR-149, second finding: this gate had not reached L1-A since 2026-03-26.
# `1be4666d` split the CLI from the daemon, after which every `orchestrator`
# invocation is a control-plane client call — and this script started no daemon,
# so `ensure_db` died on `daemon socket not found` before the first scenario.
# The three rotted bundles at the old :250/282/312 were real but were never the
# reason it failed; nothing ever got that far. FR-148 and DD-158 recorded the
# cause as DD-137 (07-25) by reading the source rather than running it, which
# understated the outage by four months.
#
# Two more things were wrong with the harness and are fixed here:
#   * `(cd core && cargo build --release)` builds the `agent-orchestrator`
#     library package. The `orchestrator` binary comes from `crates/cli` and
#     `orchestratord` from `crates/daemon`, so the gate was running whatever
#     stale artifact happened to be in target/ — measured at eight days old.
#   * `DB=data/agent_orchestrator.db` is a repository-local path the product
#     stopped using. The runtime root is `ORCHESTRATORD_DATA_DIR`.
#
# Isolation follows test-agent-driver-production-parity.sh: a throwaway
# ORCHESTRATORD_DATA_DIR and a daemon this script starts and reaps. The
# developer's runtime database at ~/.orchestratord is never opened. No provider
# is reachable either — both bundles drive self-contained `command:` steps.
#
# No `--bind`, deliberately. With a TCP bind the daemon serves TCP and the UDS
# socket is never created, so the CLI — which reaches the isolated instance by
# finding `$ORCHESTRATORD_DATA_DIR/orchestrator.sock` — has nothing to dial. The
# parity gate passes `--bind` because it also supplies a control-plane config
# and connects over TLS; this gate needs no network at all, so it stays on UDS.
set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="$REPO_ROOT/target/debug/orchestrator"
ORCHD="$REPO_ROOT/target/debug/orchestratord"
QA_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/wp05-qa.XXXXXX")"
DB="$QA_ROOT/data/agent_orchestrator.db"
DAEMON_PID=""
VERBOSE=false
RUN_LAYER=""
RUN_SCENARIO=""

PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
SELECTED_COUNT=0

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  if [[ "$FAIL_COUNT" -gt 0 || "${KEEP_WP05_QA:-0}" == "1" ]]; then
    echo "[wp05] retained at QA_ROOT=$QA_ROOT" >&2
  else
    rm -rf "$QA_ROOT"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-wp05-integration.sh"

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
    # Captured, not discarded: a failing CLI call under set -e used to end the
    # gate between an info banner and its first assertion with nothing in the
    # log — the FR-156 fixture rejection was only visible under --verbose
    # (ticket 20260810-wp05-fixture-legacy-store-put). Failure prints, then
    # propagates.
    local out rc=0
    out="$($ORCH "$@" 2>&1)" || rc=$?
    if [[ "$rc" -ne 0 ]]; then
      printf '%s\n' "$out" | sed 's/^/  [orch] /' >&2
      return "$rc"
    fi
  fi
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "FATAL: missing required command: $1" >&2; exit 1; }
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

# Every `should_run` that says yes is counted. A `--layer`/`--scenario`
# selection that matches nothing would otherwise reach the summary with
# 0 pass / 0 fail and exit 0 — a green run that asserted nothing. The counter
# is what SELECTED_COUNT below turns into a failure.
should_run() {
  local scenario_id="$1" layer="$2"
  if [ -n "$RUN_SCENARIO" ]; then
    [ "$RUN_SCENARIO" = "$scenario_id" ] || return 1
  elif [ -n "$RUN_LAYER" ]; then
    [ "$RUN_LAYER" = "$layer" ] || return 1
  fi
  SELECTED_COUNT=$((SELECTED_COUNT + 1))
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
  # FR-146: `| head -1` under pipefail kills grep and ends the gate with no summary line.
  local ids
  ids="$(grep -oE '[0-9a-f-]{36}' <<< "$create_output" || true)"
  task_id="${ids%%$'\n'*}"

  [ -n "$task_id" ] || { fail "task creation returned no task id (output: $create_output)"; return 1; }

  # Start task (may fail for invariant tests — that's expected)
  $ORCH task start "$task_id" >/dev/null 2>&1 || true

  for _ in {1..30}; do
    TASK_INFO="$($ORCH task info "$task_id" || true)"
    if grep -qiE 'status:[[:space:]]*(completed|failed)' <<< "$TASK_INFO"; then
      break
    fi
    sleep 1
  done

  echo "$task_id"
}

# ── Prerequisites ─────────────────────────────────────────────────────
require_cmd sqlite3
require_cmd cargo

# Both binaries, by package. Building `core` produces neither of them.
info "Building orchestrator and orchestratord"
cargo build -p orchestratord -p orchestrator-cli >/dev/null

[ -x "$ORCH" ] || { echo "FATAL: $ORCH was not produced by the build" >&2; exit 1; }
[ -x "$ORCHD" ] || { echo "FATAL: $ORCHD was not produced by the build" >&2; exit 1; }

# The workspace the tasks run against, and the target file the fixtures name.
# Under QA_ROOT, so the repository is never written.
mkdir -p "$QA_ROOT/workspace/fixtures/wp05-qa" "$QA_ROOT/workspace/fixtures/ticket" "$QA_ROOT/data"
printf '# WP05 QA target\n' > "$QA_ROOT/workspace/fixtures/wp05-qa/wp05-check.md"

# ORCHESTRATORD_DATA_DIR alone is what selects the isolated socket: the client's
# priority list takes an explicit control-plane config *before* the local socket
# file, so exporting one — even a path that does not exist — routes the CLI down
# the TLS branch and away from the daemon this script just started.
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
# The wp05 fixtures write their store keys from inside the step (FR-156 form:
# `orchestrator store put ...`), so the step shell must resolve `orchestrator`.
export PATH="$REPO_ROOT/target/debug:$PATH"
unset ORCHESTRATOR_SOCKET
unset ORCHESTRATOR_CONTROL_PLANE_CONFIG

# Assert the isolation is in effect rather than assuming the exports took. If
# the daemon were to open the developer's runtime root instead, every assertion
# below would still pass and the damage would be invisible from the outside.
case "$ORCHESTRATORD_DATA_DIR" in
  "$QA_ROOT"/*) ;;
  *) echo "FATAL: data dir isolation is not in effect: $ORCHESTRATORD_DATA_DIR" >&2; exit 1 ;;
esac

info "Starting an isolated daemon over UDS (data dir: $ORCHESTRATORD_DATA_DIR)"
(
  cd "$QA_ROOT/workspace"
  "$ORCHD" --foreground --workers 1 --webhook-bind none \
    --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
gate_daemon_wait_ready "$ORCH" || true
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  # The client's error, not only the daemon's log. The daemon can be listening
  # happily while the CLI is dialling somewhere else entirely, and the daemon
  # log says nothing about that — which is exactly how this failed once.
  echo "--- client error ---" >&2
  "$ORCH" task list -o json >&2 2>&1 || true
  echo "--- daemon log ---" >&2
  sed -n '1,200p' "$QA_ROOT/daemon.log" >&2
  echo "FATAL: the isolated daemon did not become ready" >&2
  exit 1
fi

# The daemon writes the database on first use; a missing file here means the
# isolation pointed somewhere unexpected, not that the run may continue.
[ -f "$DB" ] || { echo "FATAL: no database at $DB after the daemon became ready" >&2; exit 1; }
info "daemon ready (pid $DAEMON_PID)"

# ═══════════════════════════════════════════════════════════════════════
# Layer 1: Pairwise Composition
# ═══════════════════════════════════════════════════════════════════════

# ── L1-A: Store + Spawning (WP01 x WP02) ─────────────────────────────
if should_run L1A 1; then
  info "═══ L1-A: Store + Spawning (WP01 x WP02) ═══"

  run_orch apply -f "$REPO_ROOT"/fixtures/manifests/bundles/wp05-store-spawn.yaml --project wp05-L1A

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

  run_orch apply -f "$REPO_ROOT"/fixtures/manifests/bundles/wp05-store-invariant.yaml --project wp05-L1B

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

# ── A selection that ran nothing is a failure ─────────────────────────
# `--layer 2` and `--scenario L1C` were valid before FR-149 and are not now.
# Without this, either one reaches the summary with 0 pass / 0 fail and exits
# 0 — indistinguishable from a clean full run.
# `|| true`, because `fail` returns 1 and this is the last command of the `if`,
# so under `set -e` the compound's status ends the script — skipping the very
# summary line whose absence this check exists to make impossible. Caught by
# running it: the first version exited 1 with no summary, which is
# indistinguishable from the truncated runs §4.6 condition 5 is about.
if [ "$SELECTED_COUNT" -eq 0 ]; then
  fail "no scenario matched the selection (--layer '${RUN_LAYER:-}' --scenario '${RUN_SCENARIO:-}'); known scenarios: L1A (layer 1), L1B (layer 1)" || true
fi

# ═══════════════════════════════════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════════════════════════════════
echo ""
info "═══════════════════════════════════════════"
info "  WP05 Primitive Composition QA Summary"
info "═══════════════════════════════════════════"
info "  SELECTED: $SELECTED_COUNT"
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
