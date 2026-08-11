#!/usr/bin/env bash
# FR-163 requirement 2: a socket outlives the daemon that made it, and CLI
# transport discovery must not mistake the corpse for a daemon.
#
# The daemon unlinks its socket when it binds and again on a clean shutdown, so
# the trap needs a crash: `kill -9` leaves the inode behind. Discovery step 3
# used to probe `socket.exists()`, which that inode satisfies, so the CLI
# committed to UDS, spent its three retries and reported "Is the daemon
# running?" — while a TLS control plane that would have answered sat at step 4.
#
# Two scenarios, because the fix has two halves and each can regress alone:
#   A. a control-plane config is present  -> discovery must reach TLS
#   B. no control-plane config            -> the diagnostic must name the
#                                            stale socket, not a missing daemon
#
# Both assert on the *diagnostic*, never on the exit code. Under the unfixed
# code scenario B also exits non-zero — an exit-status assertion passes on the
# defect it was written to catch, which is §4.4 shape 7 in the fr-governance
# skill. The strings asserted here are the ones connect.rs declares as
# STALE_SOCKET_DIAGNOSTIC and its missing-socket counterpart; if those move,
# this gate must move with them and will say so by failing.
#
# Self-referential safety: every daemon started here uses its own mktemp data
# directory and a non-standard TCP port, and the developer's own daemon,
# database and ~/.orchestratord are never touched (QA §4.7).
set -euo pipefail

. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/release/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/release/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19163}"

# Declared by crates/orchestrator-client/src/connect.rs. Kept as variables so a
# rename shows up here as one edit rather than four scattered literals.
STALE_DIAGNOSTIC="socket exists but nothing is listening"
ABSENT_DIAGNOSTIC="daemon socket not found"

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "release binaries not found; run: cargo build --release -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

QA_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/fr163-stale-socket.XXXXXX")"
QA_HOME="$(mktemp -d "${TMPDIR:-/tmp}/fr163-home.XXXXXX")"
DAEMON_PID=""

cleanup() {
  gate_daemon_stop "$DAEMON_PID" >/dev/null 2>&1 || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT

gate_runlog_arm "scripts/qa/test-stale-socket-discovery.sh"

# HOME is redirected because a TCP daemon writes the local user's client bundle
# under $HOME, not under the data directory — running this gate against the real
# home would leave control-plane credentials in the developer's account (QA
# §4.7). Both sides honour this: the daemon reads $HOME directly and the client's
# dirs::home_dir() prefers it too, which is what makes scenario A's
# auto-discovery assertion meaningful rather than accidental.
export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data-dir"
mkdir -p "$ORCHESTRATORD_DATA_DIR"
# The CLI must resolve the socket the same way the daemon places it. Setting
# ORCHESTRATOR_SOCKET here would test a different discovery branch (step 1,
# which is deliberately not connect-probed) and hide the one under test.
unset ORCHESTRATOR_SOCKET || true

SOCKET_PATH="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"

# ── Establish the stale inode ────────────────────────────────────────────────
start_daemon_and_kill_hard() {
  (
    "$ORCHD" --foreground --workers 1 >"$QA_ROOT/daemon.log" 2>&1 &
    echo $! >"$QA_ROOT/daemon.pid"
  )
  DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

  local waited=0
  while [[ ! -S "$SOCKET_PATH" ]] && ((waited < 100)); do
    sleep 0.25
    waited=$((waited + 1))
  done
  if [[ ! -S "$SOCKET_PATH" ]]; then
    sed 's/^/  /' "$QA_ROOT/daemon.log" >&2
    echo "FAIL: daemon never bound $SOCKET_PATH; the premise of every case below is absent" >&2
    echo
    echo "FR-163 stale-socket discovery: $PASS passed, $((FAIL + 1)) failed"
    exit 1
  fi

  # SIGKILL, not SIGTERM: a clean shutdown unlinks the socket and there would be
  # nothing to test. gate_daemon_stop is deliberately not used here — it exists
  # to stop daemons gracefully, and this case needs the ungraceful exit.
  kill -KILL "$DAEMON_PID" 2>/dev/null || true
  local dead=0
  while gate_daemon_alive "$DAEMON_PID" && ((dead < 50)); do
    sleep 0.1
    dead=$((dead + 1))
  done
  wait "$DAEMON_PID" 2>/dev/null || true
  DAEMON_PID=""
}

start_daemon_and_kill_hard

# A premise that no longer holds is a failed assertion, never a skip (§4.4
# shape 7): if the socket did not survive, everything below would pass
# vacuously on a build that never fixed anything.
if [[ -S "$SOCKET_PATH" ]]; then
  pass "premise: the socket inode survived SIGKILL at $SOCKET_PATH"
else
  fail "premise: no socket at $SOCKET_PATH after SIGKILL — the trap under test cannot occur"
  echo
  echo "FR-163 stale-socket discovery: $PASS passed, $FAIL failed"
  exit 1
fi

# ── Scenario B: no control-plane config → the diagnostic must name the socket ─
# Run before scenario A because A creates the control-plane material that B
# must not see.
set +e
B_OUTPUT="$("$ORCH" task list 2>&1)"
B_STATUS=$?
set -e

if [[ "$B_STATUS" -eq 0 ]]; then
  fail "scenario B: 'task list' succeeded against a dead socket"
elif grep -qF "$STALE_DIAGNOSTIC" <<<"$B_OUTPUT"; then
  pass "scenario B: the failure names the stale socket, not a missing daemon"
else
  fail "scenario B: expected \"$STALE_DIAGNOSTIC\"; got:"
  sed 's/^/    /' <<<"$B_OUTPUT" >&2
fi

# The two failures must not read alike — that they differ is the whole repair.
if grep -qF "$ABSENT_DIAGNOSTIC" <<<"$B_OUTPUT"; then
  fail "scenario B: reported \"$ABSENT_DIAGNOSTIC\" for a socket that is present"
else
  pass "scenario B: a present-but-dead socket is not reported as an absent one"
fi

# The absent-socket wording still has to exist, or the check above passes for
# the wrong reason — a build that says the same thing in both cases.
mv "$SOCKET_PATH" "$QA_ROOT/socket.moved"
set +e
ABSENT_OUTPUT="$("$ORCH" task list 2>&1)"
set -e
mv "$QA_ROOT/socket.moved" "$SOCKET_PATH"

if grep -qF "$ABSENT_DIAGNOSTIC" <<<"$ABSENT_OUTPUT"; then
  pass "control: an absent socket still reports \"$ABSENT_DIAGNOSTIC\""
else
  fail "control: expected \"$ABSENT_DIAGNOSTIC\" for an absent socket; got:"
  sed 's/^/    /' <<<"$ABSENT_OUTPUT" >&2
fi

# ── Scenario A: a control-plane config exists → discovery must reach TLS ─────
# A second daemon on TCP writes real control-plane material into the same data
# directory, then dies the same hard death. What is left is a stale socket and a
# usable TLS config — the state in which the pre-FR-163 CLI stopped at step 3.
(
  "$ORCHD" --foreground --workers 1 --bind "$BIND_ADDR" \
    >"$QA_ROOT/daemon-tcp.log" 2>&1 &
  echo $! >"$QA_ROOT/daemon-tcp.pid"
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon-tcp.pid")"

# The client bundle the daemon generates for the local user. Under $HOME rather
# than the data directory, and named `.orchestrator` — the client is not daemon
# state. This is the path FR-163 aligned: auto-discovery used to look for
# `.orchestratord/control-plane/config.yaml`, which nothing ever wrote.
CP_CONFIG="$HOME/.orchestrator/control-plane/config.yaml"
waited=0
while [[ ! -f "$CP_CONFIG" ]] && ((waited < 100)); do
  sleep 0.25
  waited=$((waited + 1))
done

if [[ ! -f "$CP_CONFIG" ]]; then
  sed 's/^/  /' "$QA_ROOT/daemon-tcp.log" >&2
  fail "scenario A premise: the TCP daemon never wrote $CP_CONFIG"
else
  pass "scenario A premise: control-plane config exists at $CP_CONFIG"

  # Restore the stale socket, which binding on TCP left untouched, and confirm
  # both halves of the state are present before asserting on it.
  [[ -S "$SOCKET_PATH" ]] || : >"$SOCKET_PATH"

  set +e
  A_OUTPUT="$("$ORCH" task list -o json 2>&1)"
  A_STATUS=$?
  set -e

  # Reaching TLS is the assertion. Whether the RPC then succeeds depends on
  # RBAC material this gate does not provision, so a TLS-shaped failure counts
  # as reaching step 4 — what must not appear is the UDS dead end.
  if grep -qF "$STALE_DIAGNOSTIC" <<<"$A_OUTPUT"; then
    fail "scenario A: discovery stopped at the stale socket instead of falling through to TLS:"
    sed 's/^/    /' <<<"$A_OUTPUT" >&2
  elif grep -qF "$ABSENT_DIAGNOSTIC" <<<"$A_OUTPUT"; then
    fail "scenario A: discovery reported a missing socket instead of falling through to TLS:"
    sed 's/^/    /' <<<"$A_OUTPUT" >&2
  elif [[ "$A_STATUS" -eq 0 ]] || grep -qEi 'tls|certificate|transport error|rbac|permission' <<<"$A_OUTPUT"; then
    pass "scenario A: discovery fell through the stale socket and reached the TLS control plane"
  else
    fail "scenario A: could not tell which transport was used; got:"
    sed 's/^/    /' <<<"$A_OUTPUT" >&2
  fi
fi

gate_daemon_stop "$DAEMON_PID" "$ORCHESTRATORD_DATA_DIR/daemon.pid" || true
DAEMON_PID=""

echo
echo "FR-163 stale-socket discovery: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
