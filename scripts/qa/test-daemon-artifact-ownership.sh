#!/usr/bin/env bash
# FR-170: a daemon's socket and pidfile belong to it by identity, not by path.
#
# FR-169 made a daemon whose data directory vanished shut itself down, and
# DD-185 recorded the residue as a bounded ~15s overlap. It was not bounded.
# Teardown named its two artifacts by path, and after delete-and-recreate those
# paths are the *successor's* files: measured at b1a6a0d0, the old daemon's exit
# unlinked the new daemon's socket and pidfile, leaving it alive with seven
# database fds and a listening socket on an unlinked inode, invisible to
# `daemon status`, unreachable by `daemon stop`, never self-terminating (its own
# data directory was intact, so the vanish watcher never fired) — and with the
# pidfile gone, a third daemon started cleanly on the same path. The fix for a
# stale-evidence bug had manufactured a worse one.
#
# What each case asserts, and why none is a proxy for another:
#
#   1. The successor's socket survives, *by inode*. Presence alone is satisfied
#      by a broken state — the socket deleted and anything at all recreated at
#      that name — so the inode recorded during the overlap is the subject.
#      Paired with a real RPC, because an inode proves the file and not the
#      service (§4.4: a structural check needs a behavioural one beside it).
#   2. The daemon count stays bounded: a third daemon is refused *by name and by
#      PID*. An exit code cannot say which of a start's many failures occurred.
#   3. NEGATIVE — a normal stop still cleans up after itself. This is the path
#      that runs every day and the one an over-strict ownership test breaks.
#      Without it, a `cleanup` that removes nothing at all passes cases 1 and 2.
#   4. NEGATIVE — crash debris is still reclaimed. SIGKILL leaves a socket and
#      pidfile behind by design; the next start must remove them and bind, and
#      its socket must have a *different* inode, which distinguishes "the stale
#      socket was removed and rebound" from "the stale socket was reused".
#   5. `--foreground` and daemonize give the same answer. The guard, the bind
#      and the teardown all run post-fork, so they should — asserted rather than
#      assumed, since the fork is exactly where a PID claim can go wrong.
#   6. `mv` reaches the same end state as `rm -rf`. Every route that changes the
#      directory's (dev, ino) enters the same window, so this is one assertion
#      rather than a second mechanism.
#
# Self-referential safety: own mktemp data directories and own HOME; the
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

# A UDS path has a hard length limit (SUN_LEN, 104 bytes on macOS) and macOS
# hands out a 48-character TMPDIR, so the budget for everything below it is
# small. Names here are short for that reason, not for brevity's sake; the
# readable form of each case lives in $LABEL. Checked rather than assumed
# below — over the limit, `bind` fails and every case reports the far vaguer
# "the daemon never became ready".
TMP_BASE="${TMPDIR:-/tmp}"
TMP_BASE="${TMP_BASE%/}"
QA_ROOT="$(mktemp -d "$TMP_BASE/fr170.XXXXXX")"
QA_HOME="$(mktemp -d "$TMP_BASE/fr170h.XXXXXX")"
OLD_PID=""
NEW_PID=""
THIRD_PID=""

COMPLETED=0

cleanup() {
  # A run that ends before its summary is not a verdict. Without this, an abort
  # anywhere above prints some PASS lines, no FAIL line and no total, which a
  # reader checking only the exit code cannot distinguish from a real failure.
  if [[ "$COMPLETED" -ne 1 ]]; then
    echo >&2
    echo "FR-170 daemon artifact ownership: RUN TRUNCATED before the summary line" >&2
    echo "  (${PASS:-0} passed, ${FAIL:-0} failed had been recorded when it stopped)" >&2
  fi
  gate_daemon_stop "$THIRD_PID" >/dev/null 2>&1 || true
  gate_daemon_stop "$NEW_PID" >/dev/null 2>&1 || true
  gate_daemon_stop "$OLD_PID" >/dev/null 2>&1 || true
  OLD_PID=""; NEW_PID=""; THIRD_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT

gate_runlog_arm "scripts/qa/test-daemon-artifact-ownership.sh"

export HOME="$QA_HOME"
unset ORCHESTRATOR_SOCKET || true

# The vanish watcher polls every 5s and needs 3 consecutive confirmations
# (DATA_DIR_CHECK_PERIOD x DATA_DIR_CHECK_CONFIRMATIONS in crates/daemon/src/main.rs),
# so the old daemon exits ~15s after the directory changes identity. Derived from
# those constants rather than typed, and re-derived here as a budget with margin.
VANISH_BUDGET=40

# Echoes the inode, or nothing when the path is absent — and always returns 0.
#
# The `|| true` is load-bearing under `set -e`. Without it, a missing socket
# makes both `stat`s fail, the enclosing `INO="$(inode_of ...)"` assignment
# inherits that status, and the run dies *before* its summary line. That is not
# a red gate; it is a truncated one, and a truncated run reads exactly like a
# complete one if the reader trusts the exit code (§4.4 shape 7, §4.6 cond. 5).
# Measured: against the pre-FR-170 teardown this gate printed two PASS lines, no
# FAIL line and no summary, while the defect it exists to catch was present.
inode_of() { stat -f %i "$1" 2>/dev/null || stat -c %i "$1" 2>/dev/null || true; }
# The ownership token beside a socket, or empty when there is none to read.
owner_of() { tr -d '[:space:]' < "$1.owner" 2>/dev/null || true; }

# Open file descriptors on the runtime database, or 0. Total for inode_of's
# reason: a dead PID makes lsof exit non-zero, and that must be an answer here
# rather than the end of the run.
db_fds() {
  local pid="$1"
  [[ -n "$pid" ]] || { echo 0; return 0; }
  lsof -p "$pid" 2>/dev/null | grep -c 'agent_orchestrator\.db' || true
}

# The longest socket path this run will ask the kernel to bind. A premise that
# no longer holds is a named failure, never a puzzling one: without this, an
# over-long TMPDIR surfaces as four "the daemon never became ready" timeouts
# 25s apart and reads as a broken daemon rather than a broken harness.
SUN_LEN_LIMIT=104
LONGEST_SOCKET="$QA_ROOT/dd-mf/orchestrator.sock"
if ((${#LONGEST_SOCKET} >= SUN_LEN_LIMIT)); then
  echo "FAIL: TMPDIR is too long for a UDS path: ${#LONGEST_SOCKET} bytes >= $SUN_LEN_LIMIT" >&2
  echo "  $LONGEST_SOCKET" >&2
  echo "  re-run with a shorter TMPDIR, e.g. TMPDIR=/tmp $0" >&2
  COMPLETED=1
  echo
  echo "FR-170 daemon artifact ownership: 0 passed, 1 failed"
  exit 1
fi

# Start a daemon on $1 in the form named by $2 (foreground|daemonize), echoing
# the PID that a stop must target. In daemonize form the launched process forks
# twice and exits, so $! is not the daemon — the pidfile is the only source of
# the real PID, which is precisely why gate_daemon_pid_from_file exists.
start_daemon() {
  local data_dir="$1" form="$2" log="$3"
  if [[ "$form" == "foreground" ]]; then
    ORCHESTRATORD_DATA_DIR="$data_dir" "$ORCHD" --foreground --workers 1 \
      --webhook-bind none >"$log" 2>&1 &
    echo $!
  else
    ORCHESTRATORD_DATA_DIR="$data_dir" "$ORCHD" --workers 1 \
      --webhook-bind none >"$log" 2>&1 || true
    local waited=0
    while [[ ! -f "$data_dir/daemon.pid" ]] && ((waited < 100)); do
      sleep 0.1
      waited=$((waited + 1))
    done
    # Total by construction: a missing pidfile must become a named FAIL at the
    # call site, never a `set -e` abort that swallows the summary line.
    gate_daemon_pid_from_file "$data_dir/daemon.pid" || true
  fi
}

# Attempt a third daemon and report what happened, in bounded time.
#
# The obvious form -- OUT="$("$ORCHD" --foreground ... 2>&1 || true)" -- assumes
# the refusal. When a regression removes the guard the daemon *starts*, and in
# foreground it never returns: the gate hangs instead of going red, and a hung
# gate is worse than a failed one because CI reports a timeout with no
# diagnostic and no summary line. macOS has no timeout(1), so the bound is
# explicit here. Sets THIRD_OUT and THIRD_STARTED; reclaims the daemon if one
# came up, because a probe that leaks the process it was testing for is the
# CLAUDE.md daemon rule broken by the check written to enforce it.
probe_third_daemon() {
  local data_dir="$1" log="$2" waited=0
  THIRD_OUT=""; THIRD_STARTED=0
  ORCHESTRATORD_DATA_DIR="$data_dir" "$ORCHD" --foreground --workers 1 \
    --webhook-bind none >"$log" 2>&1 &
  THIRD_PID=$!
  while gate_daemon_alive "$THIRD_PID" && ((waited < 150)); do
    sleep 0.1
    waited=$((waited + 1))
  done
  if gate_daemon_alive "$THIRD_PID"; then
    THIRD_STARTED=1
    gate_daemon_stop "$THIRD_PID" >/dev/null 2>&1 || true
  fi
  wait "$THIRD_PID" 2>/dev/null || true
  THIRD_PID=""
  THIRD_OUT="$(cat "$log" 2>/dev/null || true)"
}

wait_ready() {
  ORCHESTRATORD_DATA_DIR="$1" gate_daemon_wait_ready "$ORCH"
}

# ── Cases 1, 5, 6: the successor survives its predecessor's exit ─────────────
#
# Run once per start form and once per route that changes the directory's
# identity, so case 5 and case 6 are the same assertions under different
# preconditions rather than weaker copies of them.
for FORM in foreground daemonize; do
  for ROUTE in rm mv; do
    LABEL="$ROUTE + $FORM"
    SLUG="${ROUTE:0:1}${FORM:0:1}"
    DD="$QA_ROOT/dd-$SLUG"
    mkdir -p "$DD"

    OLD_PID="$(start_daemon "$DD" "$FORM" "$QA_ROOT/old-$SLUG.log")"
    if ! wait_ready "$DD"; then
      fail "$LABEL: the first daemon never became ready"
      gate_daemon_stop "$OLD_PID" >/dev/null 2>&1 || true
      OLD_PID=""
      continue
    fi

    # Change the directory's identity by the route under test. Both give the
    # path a new inode; nothing else about them differs to the daemon.
    if [[ "$ROUTE" == "rm" ]]; then
      rm -rf "$DD"
    else
      mv "$DD" "$DD.moved"
    fi
    mkdir -p "$DD"

    NEW_PID="$(start_daemon "$DD" "$FORM" "$QA_ROOT/new-$SLUG.log")"
    if ! wait_ready "$DD"; then
      fail "$LABEL: the successor never became ready"
      gate_daemon_stop "$NEW_PID" >/dev/null 2>&1 || true
      gate_daemon_stop "$OLD_PID" >/dev/null 2>&1 || true
      NEW_PID=""; OLD_PID=""
      continue
    fi

    # Recorded during the overlap: this is what must still be true afterwards.
    SOCK_INO_DURING="$(inode_of "$DD/orchestrator.sock")"
    SOCK_OWNER_DURING="$(owner_of "$DD/orchestrator.sock")"

    # The predecessor is still holding the orphaned database open. Asserted
    # here rather than only after its exit, because "a dead process holds no
    # fds" is true of any dead PID and of one that never existed — the
    # non-vacuous half of FR-169's 7 -> 0 measurement is the 7.
    OLD_FDS_DURING="$(db_fds "$OLD_PID")"
    if [[ "$OLD_FDS_DURING" -gt 0 ]]; then
      pass "$LABEL: during the overlap the predecessor still holds $OLD_FDS_DURING database fds on the orphaned inode"
    else
      fail "$LABEL: expected the predecessor to still hold database fds during the overlap; counted $OLD_FDS_DURING"
    fi

    # Case 2 belongs here, while the successor is alive and the predecessor has
    # not yet exited — a third daemon must be refused, and say so by name.
    probe_third_daemon "$DD" "$QA_ROOT/third-during-$SLUG.log"
    if ((THIRD_STARTED)); then
      fail "$LABEL: a third daemon STARTED during the overlap instead of being refused"
    elif grep -qF "another orchestratord is already running (PID $NEW_PID)" <<<"$THIRD_OUT"; then
      pass "$LABEL: a third daemon is refused, naming the holder's PID $NEW_PID"
    else
      fail "$LABEL: expected refusal naming PID $NEW_PID; got: $(tail -3 <<<"$THIRD_OUT")"
    fi

    # Wait for the predecessor to notice and exit. Its own diagnostic is the
    # subject — an exit code cannot say which of its exits this was, and a
    # timeout here must fail rather than silently proceed to weaker assertions.
    WAITED=0
    while gate_daemon_alive "$OLD_PID" && ((WAITED < VANISH_BUDGET)); do
      sleep 1
      WAITED=$((WAITED + 1))
    done
    if gate_daemon_alive "$OLD_PID"; then
      fail "$LABEL: the predecessor was still alive after ${VANISH_BUDGET}s"
      gate_daemon_stop "$OLD_PID" >/dev/null 2>&1 || true
      gate_daemon_stop "$NEW_PID" >/dev/null 2>&1 || true
      OLD_PID=""; NEW_PID=""
      continue
    fi
    OLD_FDS_AFTER="$(db_fds "$OLD_PID")"
    if [[ "$OLD_FDS_AFTER" -eq 0 ]]; then
      pass "$LABEL: the predecessor's database fds fell $OLD_FDS_DURING -> 0"
    else
      fail "$LABEL: the predecessor still holds $OLD_FDS_AFTER database fds after exiting"
    fi
    OLD_PID=""

    # Where the predecessor's own explanation landed depends on the start form,
    # and that is not a harness detail — a daemonized orchestratord redirects
    # stdout/stderr to `$data_dir/daemon.log` (main.rs), so its account of
    # "my data directory vanished" is written *into the directory that
    # vanished*. After `mv` the file is still reachable at its new location;
    # after `rm -rf` it is an unlinked inode only the dying process can write
    # to, and no assertion can recover it. That is FR-170's own shape applied
    # to a diagnostic instead of a socket, and it is recorded in DD-186 rather
    # than hidden behind a skip.
    case "$FORM/$ROUTE" in
      foreground/*) PRED_LOG="$QA_ROOT/old-$SLUG.log" ;;
      daemonize/mv) PRED_LOG="$DD.moved/daemon.log" ;;
      daemonize/rm) PRED_LOG="" ;;
    esac

    if [[ -z "$PRED_LOG" ]]; then
      # The exit itself is already asserted above, by name and within budget;
      # only the *reason* is unobservable here. Assert that much explicitly, so
      # the log says which property this case covers and which it cannot.
      # `$DD/daemon.log` exists again, but it is the *successor's* — it was
      # created when the successor daemonized into the recreated directory.
      # The predecessor's account is on an unlinked inode with no name. Assert
      # exactly that: the surviving log is the successor's and carries no
      # vanish diagnostic, which is what distinguishes "the name was re-taken"
      # from "the predecessor never explained itself".
      if [[ -e "$DD/daemon.log" ]] && ! grep -qF "data directory is gone" "$DD/daemon.log"; then
        pass "$LABEL: predecessor exited in ${WAITED}s; its reason is unobservable by construction (its log was unlinked with the directory; $DD/daemon.log is the successor's)"
      else
        fail "$LABEL: expected the recreated daemon.log to be the successor's and free of the vanish diagnostic"
      fi
    elif grep -qF "data directory is gone" "$PRED_LOG"; then
      pass "$LABEL: the predecessor exited naming the data directory, after ${WAITED}s"
    else
      fail "$LABEL: the predecessor exited without the 'data directory is gone' diagnostic in $PRED_LOG"
    fi

    # Case 1. Not mere presence: a socket deleted and recreated by anything at
    # all would satisfy an -e test. Both facts are asserted because on Linux
    # neither is sufficient alone — a rebound socket usually takes the freed
    # inode number straight back (measured 50/50 for a regular file), so an
    # unchanged inode is not by itself proof that nothing was unlinked. The
    # ownership token is: it is content, minted per process, and a successor
    # that had to rebind would have written its own.
    SOCK_INO_AFTER="$(inode_of "$DD/orchestrator.sock")"
    SOCK_OWNER_AFTER="$(owner_of "$DD/orchestrator.sock")"
    if [[ -n "$SOCK_INO_AFTER" && "$SOCK_INO_AFTER" == "$SOCK_INO_DURING" \
       && -n "$SOCK_OWNER_AFTER" && "$SOCK_OWNER_AFTER" == "$SOCK_OWNER_DURING" ]]; then
      pass "$LABEL: the successor's socket survived, same inode ($SOCK_INO_AFTER) and same claim"
    else
      fail "$LABEL: socket was inode $SOCK_INO_DURING / claim ${SOCK_OWNER_DURING:-<none>} during the overlap, now inode ${SOCK_INO_AFTER:-<gone>} / claim ${SOCK_OWNER_AFTER:-<none>}"
    fi

    PIDFILE_AFTER="$(cat "$DD/daemon.pid" 2>/dev/null || true)"
    if [[ "$PIDFILE_AFTER" == "$NEW_PID" ]]; then
      pass "$LABEL: the pidfile still names the successor ($NEW_PID)"
    else
      fail "$LABEL: pidfile should name $NEW_PID, reads ${PIDFILE_AFTER:-<gone>}"
    fi

    # The behavioural half: the file existing is not the service answering.
    STATUS_OUT="$(ORCHESTRATORD_DATA_DIR="$DD" "$ORCH" daemon status 2>&1 || true)"
    if grep -qF "PID $NEW_PID" <<<"$STATUS_OUT"; then
      pass "$LABEL: daemon status reaches the successor and names it"
    else
      fail "$LABEL: daemon status did not name $NEW_PID; got: $STATUS_OUT"
    fi

    RPC_OUT="$(ORCHESTRATORD_DATA_DIR="$DD" "$ORCH" task list -o json 2>&1 || true)"
    if grep -q '^\[' <<<"$RPC_OUT" || grep -q '"tasks"' <<<"$RPC_OUT"; then
      pass "$LABEL: a real RPC through the surviving socket succeeds"
    else
      fail "$LABEL: RPC through the surviving socket failed: $(head -3 <<<"$RPC_OUT")"
    fi

    # The case that actually failed before DD-186: during the overlap the
    # pidfile still existed, so a third daemon was refused either way. It is
    # *after* the predecessor's teardown — which used to delete that pidfile —
    # that the refusal had nothing left to stand on.
    probe_third_daemon "$DD" "$QA_ROOT/third-after-$SLUG.log"
    if ((THIRD_STARTED)); then
      fail "$LABEL: after the predecessor's exit a third daemon STARTED — the pidfile that refuses it is gone"
    elif grep -qF "another orchestratord is already running (PID $NEW_PID)" <<<"$THIRD_OUT"; then
      pass "$LABEL: after the predecessor's exit, a third daemon is still refused, naming PID $NEW_PID"
    else
      fail "$LABEL: after the predecessor's exit, expected refusal naming PID $NEW_PID; got: $(tail -3 <<<"$THIRD_OUT")"
    fi

    # ── Case 3, NEGATIVE: a normal stop must still clean up after itself ─────
    gate_daemon_stop "$NEW_PID" "$DD/daemon.pid" || fail "$LABEL: the successor did not stop"
    NEW_PID=""
    if [[ ! -e "$DD/orchestrator.sock" && ! -e "$DD/daemon.pid" ]]; then
      pass "$LABEL: a clean stop removes the daemon's own socket and pidfile"
    else
      fail "$LABEL: a clean stop left socket=$([[ -e "$DD/orchestrator.sock" ]] && echo present || echo gone) pidfile=$([[ -e "$DD/daemon.pid" ]] && echo present || echo gone)"
    fi
  done
done

# ── Case 4, NEGATIVE: crash debris is still reclaimed ────────────────────────
#
# SIGKILL leaves the socket and pidfile behind by design — that debris is what
# the entry-side removal exists for, and an ownership check on the way out must
# not turn a crash into a daemon that can never restart.
DD="$QA_ROOT/dd-ck"
mkdir -p "$DD"
OLD_PID="$(start_daemon "$DD" foreground "$QA_ROOT/crash.log")"
if wait_ready "$DD"; then
  STALE_OWNER="$(owner_of "$DD/orchestrator.sock")"
  gate_daemon_kill_hard "$OLD_PID" || fail "the crashed daemon survived SIGKILL"
  OLD_PID=""

  if [[ -e "$DD/orchestrator.sock" && -e "$DD/daemon.pid" ]]; then
    pass "SIGKILL leaves the socket and pidfile behind, as the next start expects"
  else
    fail "SIGKILL left no debris; this case can no longer prove anything"
  fi

  NEW_PID="$(start_daemon "$DD" foreground "$QA_ROOT/crash-restart.log")"
  if wait_ready "$DD"; then
    # The claim, not the inode. This assertion used to read
    # `REBOUND_INO != STALE_INO`, which is a claim about the inode allocator
    # rather than about the daemon: a socket unlinked and rebound at the same
    # path takes the freed number back on Linux (measured 50 times in 50), so
    # the restart this case exists to prove would have read as a failure there.
    # The token is minted per process and cannot collide.
    REBOUND_OWNER="$(owner_of "$DD/orchestrator.sock")"
    if [[ -n "$REBOUND_OWNER" && "$REBOUND_OWNER" != "$STALE_OWNER" ]]; then
      pass "the next start reclaimed the crash debris and bound a socket it claims as its own"
    else
      fail "expected a fresh socket claim after restart; stale=${STALE_OWNER:-<none>} now=${REBOUND_OWNER:-<none>}"
    fi
  else
    fail "a daemon could not start over its own crash debris"
  fi
  gate_daemon_stop "$NEW_PID" "$DD/daemon.pid" >/dev/null 2>&1 || true
  NEW_PID=""
else
  fail "the crash-case daemon never became ready"
  gate_daemon_stop "$OLD_PID" >/dev/null 2>&1 || true
  OLD_PID=""
fi

COMPLETED=1
echo
echo "FR-170 daemon artifact ownership: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
