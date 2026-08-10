#!/usr/bin/env bash
# The FR-160 reproduction probe: `wait` on a pidfile PID is a no-op.
#
# Both shapes are asserted side by side, because "waited" and "never waited"
# are indistinguishable in a log that only shows one of them:
#   A (pidfile): the daemon is a subshell's child. `wait` returns immediately
#      and the daemon is still alive when the next line runs — the state every
#      pre-FR-160 cleanup did its `rm -rf` in.
#   B ($!): the daemon is our child. `wait` blocks until it honours SIGTERM.
# Then the shared library is held to its contract: gate_daemon_stop must
# actually stop a live non-child daemon, which is the fact the 23 migrated
# pidfile sites depend on.
#
# A person runs this at a terminal (developer-tool in qa-gate-surface.json);
# it asserts bash semantics and the library's behaviour, not a repository
# invariant. It reclaims every process it starts — including on abort — per
# FR-160's governance precondition 3: a probe that demonstrates a leak must
# not itself leak.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
. "$REPO_ROOT/scripts/lib/gate_daemon.sh"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr160-probe.XXXXXX")"
PROBE_PID_A=""
PROBE_PID_B=""
PROBE_PID_C=""

cleanup() {
  # Belt for the abort case; the main flow already stopped everything.
  local p
  for p in "$PROBE_PID_A" "$PROBE_PID_B" "$PROBE_PID_C"; do
    gate_daemon_stop "$p" >/dev/null 2>&1 || true
  done
  rm -rf "$WORK"
}
trap cleanup EXIT

# A daemon that needs 2s to honour SIGTERM. The inner loop sleeps 1 so the
# handler starts within a second of the signal; total time-to-exit after TERM
# is between 2 and 3 seconds — long enough to observe "wait returned but the
# process is alive", short enough to assert on with $SECONDS granularity.
#
# $1 is a readiness file, written after the trap is installed. Killing the
# async child before it has exec'd sh and installed its handler reproduces
# neither shape — measured here: the pre-exec child is still a forked bash
# holding a copy of this script's EXIT trap, and the stray TERM made it run
# cleanup, deleting $WORK mid-probe. The handshake closes that window, the
# same way the real gates only signal a daemon they have already talked to.
FAKE_DAEMON='trap "sleep 2; exit 0" TERM; : > "$1"; while :; do sleep 1; done'

# Poll for a readiness file for up to 5s; fail the probe if it never appears.
await_ready() {
  local ready="$1" waited=0
  while [[ ! -f "$ready" ]] && (( waited < 50 )); do
    sleep 0.1
    waited=$((waited + 1))
  done
  if [[ ! -f "$ready" ]]; then
    fail "fake daemon never became ready ($ready)"
    return 1
  fi
}

echo "== shape A: PID from a pidfile (the shape 23 gates had) =="
(
  cd "$WORK"
  sh -c "$FAKE_DAEMON" probe-daemon ready-a >/dev/null 2>&1 &
  echo $! > probe-a.pid
)
PROBE_PID_A="$(gate_daemon_pid_from_file "$WORK/probe-a.pid")"
await_ready "$WORK/ready-a"
kill "$PROBE_PID_A" 2>/dev/null
BEFORE=$SECONDS
wait "$PROBE_PID_A" 2>/dev/null || true
ELAPSED=$((SECONDS - BEFORE))
# <=1 rather than ==0: $SECONDS ticks on wall-clock boundaries, so an instant
# call can straddle one. The real return is milliseconds; shape B's is >=2s.
if [[ "$ELAPSED" -le 1 ]]; then
  pass "shape A: wait returned immediately (${ELAPSED}s) — it never waited"
else
  fail "shape A: wait blocked ${ELAPSED}s; expected an immediate return"
fi
if kill -0 "$PROBE_PID_A" 2>/dev/null; then
  pass "shape A: daemon $PROBE_PID_A is still alive after wait returned"
else
  fail "shape A: daemon $PROBE_PID_A already gone; the race this FR closes is not being demonstrated"
fi

echo "== shape B: PID from \$! (the shape 2 gates had) =="
sh -c "$FAKE_DAEMON" probe-daemon "$WORK/ready-b" >/dev/null 2>&1 &
PROBE_PID_B=$!
await_ready "$WORK/ready-b"
kill "$PROBE_PID_B" 2>/dev/null
BEFORE=$SECONDS
wait "$PROBE_PID_B" 2>/dev/null || true
ELAPSED=$((SECONDS - BEFORE))
if [[ "$ELAPSED" -ge 2 ]]; then
  pass "shape B: wait blocked ${ELAPSED}s until the daemon honoured SIGTERM"
else
  fail "shape B: wait returned in ${ELAPSED}s; expected it to block ~2s"
fi
if kill -0 "$PROBE_PID_B" 2>/dev/null; then
  fail "shape B: daemon $PROBE_PID_B still signalable; wait did not reap it"
else
  pass "shape B: daemon $PROBE_PID_B exited and was reaped"
fi
PROBE_PID_B=""

echo "== library: gate_daemon_stop stops a live non-child daemon =="
(
  cd "$WORK"
  sh -c "$FAKE_DAEMON" probe-daemon ready-c >/dev/null 2>&1 &
  echo $! > probe-c.pid
)
PROBE_PID_C="$(gate_daemon_pid_from_file "$WORK/probe-c.pid")"
await_ready "$WORK/ready-c"
if gate_daemon_stop "$PROBE_PID_C"; then
  pass "library: gate_daemon_stop returned 0 for a daemon that dies on SIGTERM"
else
  fail "library: gate_daemon_stop reported failure against a well-behaved daemon"
fi
if kill -0 "$PROBE_PID_C" 2>/dev/null; then
  fail "library: daemon $PROBE_PID_C survived gate_daemon_stop"
else
  pass "library: daemon $PROBE_PID_C is gone after gate_daemon_stop"
fi
PROBE_PID_C=""

# Reclaim shape A's survivor through the same contract (precondition 3), and
# assert the reclamation rather than assume it.
if gate_daemon_stop "$PROBE_PID_A" && ! kill -0 "$PROBE_PID_A" 2>/dev/null; then
  pass "reclamation: shape A daemon $PROBE_PID_A stopped through the library"
else
  fail "reclamation: shape A daemon $PROBE_PID_A was not reclaimed"
fi
PROBE_PID_A=""

echo "== library: gate_daemon_pid_from_file names its failure =="
if PID_OUT="$(gate_daemon_pid_from_file "$WORK/absent.pid" 2>/dev/null)"; then
  fail "pid_from_file: returned 0 (\"$PID_OUT\") for a pidfile that does not exist"
else
  pass "pid_from_file: missing pidfile is a named failure, not a silent empty PID"
fi

echo
echo "FR-160 wait-shapes probe: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
