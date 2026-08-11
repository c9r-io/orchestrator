#!/usr/bin/env bash
# FR-163 requirement 3: readiness is a signal the daemon publishes, not a proxy
# every gate re-invents.
#
# Before this, 24 hand-copied loops across 23 gates polled `task list` and
# treated the first successful call as "ready", with five different timeout
# budgets (7.5s, 10s, 15s, 20s, 25s) none of which was derived from anything.
# `task list` succeeds the moment the socket accepts a connection — which is
# before the worker pool has registered — so a gate could create a task and
# watch nothing pick it up.
#
# What this asserts, and why each case is not a proxy for the others:
#
#   1. The wait really waits. Started before the daemon exists, `--wait-ready`
#      must block and then succeed — measured by elapsed time, because a call
#      that returned instantly would also "succeed" against a daemon that
#      happened to be up already.
#   2. It fails, in bounded time, when readiness never comes, and the failure
#      names the last thing observed. A wait that hangs forever and a wait that
#      exits 0 are both worse than a red gate.
#   3. Every subsystem is named whether ready or not. A report that lists only
#      failures reads as complete when the list is empty, so "ready" would be
#      indistinguishable from "nothing was measured".
#   4. `daemon status` without the flag still answers from the PID file and
#      opens no connection — the property that lets it work on a daemon that
#      cannot serve.
#
# Deliberately NOT asserted here: the RBAC tier. `Health` must be ReadOnly or no
# gate can call it, and that is pinned by a unit test on the role table
# (`required_role_mapping_is_stable`), where the Admin-defaulting fallback is
# visible. A gate-level check would need control-plane material to prove the
# same thing less directly.
#
# Self-referential safety: own mktemp data directory and own HOME; the
# developer's daemon, database and ~/.orchestratord are never touched (QA §4.7).
set -euo pipefail

. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

QA_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/fr163-readiness.XXXXXX")"
QA_HOME="$(mktemp -d "${TMPDIR:-/tmp}/fr163-readiness-home.XXXXXX")"
DAEMON_PID=""

cleanup() {
  gate_daemon_stop "$DAEMON_PID" >/dev/null 2>&1 || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT

gate_runlog_arm "scripts/qa/test-daemon-readiness.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data-dir"
mkdir -p "$ORCHESTRATORD_DATA_DIR"
unset ORCHESTRATOR_SOCKET || true

# ── 1. The wait blocks until the daemon can serve ────────────────────────────
# The waiter starts first and the daemon follows after a delay, so a probe that
# only ever sees a ready daemon cannot pass this. The delay is well inside the
# wait's own timeout.
START=$SECONDS
(
  sleep 3
  "$ORCHD" --foreground --workers 2 >"$QA_ROOT/daemon.log" 2>&1 &
  echo $! >"$QA_ROOT/daemon.pid"
) &
SPAWNER=$!

set +e
READY_OUTPUT="$("$ORCH" daemon status --wait-ready --timeout 40 2>&1)"
READY_STATUS=$?
set -e
ELAPSED=$((SECONDS - START))
wait "$SPAWNER" 2>/dev/null || true
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

if [[ "$READY_STATUS" -ne 0 ]]; then
  fail "the wait did not reach readiness within 40s:"
  sed 's/^/    /' <<<"$READY_OUTPUT" >&2
  sed -n '1,60p' "$QA_ROOT/daemon.log" >&2
else
  pass "the wait reached readiness: $READY_OUTPUT"
fi

if (( ELAPSED >= 3 )); then
  pass "the wait actually waited (${ELAPSED}s, daemon started at 3s)"
else
  fail "the wait returned after ${ELAPSED}s, before the daemon was started — it is not waiting on anything"
fi

# ── 3. Every subsystem is named ──────────────────────────────────────────────
# Asserted on the same output, and per subsystem rather than by counting: a
# total tells you three things were printed, not which three.
for subsystem in migrations keyring workers; do
  if grep -qF "$subsystem=" <<<"$READY_OUTPUT"; then
    pass "the report names the '$subsystem' subsystem"
  else
    fail "the report never names the '$subsystem' subsystem: $READY_OUTPUT"
  fi
done

# The worker count is the fact `task list` could not see, so it is asserted
# rather than assumed: the daemon was started with 2 workers.
if grep -qE 'workers=ready \(2/2 started\)' <<<"$READY_OUTPUT"; then
  pass "readiness reports both configured workers started, which a socket probe cannot see"
else
  fail "expected 'workers=ready (2/2 started)'; got: $READY_OUTPUT"
fi

# ── 4. Plain status opens no connection ──────────────────────────────────────
# Run against a data directory with no daemon at all. If `status` had started
# connecting, this would hang or fail rather than reporting from the PID file.
EMPTY_DIR="$QA_ROOT/empty"
mkdir -p "$EMPTY_DIR"
set +e
PLAIN_OUTPUT="$(ORCHESTRATORD_DATA_DIR="$EMPTY_DIR" "$ORCH" daemon status 2>&1)"
PLAIN_STATUS=$?
set -e
if [[ "$PLAIN_STATUS" -eq 0 ]] && grep -qF "not running" <<<"$PLAIN_OUTPUT"; then
  pass "plain 'daemon status' answers from the PID file with no daemon present"
else
  fail "plain 'daemon status' should report not-running without connecting; got ($PLAIN_STATUS): $PLAIN_OUTPUT"
fi

# ── 2. The wait fails, bounded, and names what it last saw ───────────────────
# Same empty data directory: nothing to connect to, so readiness never comes.
START=$SECONDS
set +e
TIMEOUT_OUTPUT="$(ORCHESTRATORD_DATA_DIR="$EMPTY_DIR" \
  "$ORCH" daemon status --wait-ready --timeout 3 2>&1)"
TIMEOUT_STATUS=$?
set -e
TIMEOUT_ELAPSED=$((SECONDS - START))

if [[ "$TIMEOUT_STATUS" -eq 0 ]]; then
  fail "waiting for a daemon that does not exist exited 0: $TIMEOUT_OUTPUT"
else
  pass "waiting for a daemon that does not exist fails rather than succeeding"
fi

if (( TIMEOUT_ELAPSED <= 20 )); then
  pass "the failure is bounded (${TIMEOUT_ELAPSED}s for a 3s timeout)"
else
  fail "a 3s timeout took ${TIMEOUT_ELAPSED}s; the deadline is not bounding the wait"
fi

# The diagnostic, not the exit code: a bare non-zero cannot tell the reader
# whether readiness was refused or the CLI simply could not start.
if grep -qF "was not ready within" <<<"$TIMEOUT_OUTPUT" &&
  grep -qF "last status:" <<<"$TIMEOUT_OUTPUT"; then
  pass "the timeout names the deadline and the last observed status"
else
  fail "expected a 'was not ready within ...; last status: ...' diagnostic; got: $TIMEOUT_OUTPUT"
fi

gate_daemon_stop "$DAEMON_PID" "$ORCHESTRATORD_DATA_DIR/daemon.pid" || true
DAEMON_PID=""

echo
echo "FR-163 daemon readiness: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
