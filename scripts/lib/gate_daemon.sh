# The shared daemon teardown contract (FR-160).
#
# 25 QA gates stopped the daemon they started with `kill "$DAEMON_PID"` then
# `wait "$DAEMON_PID"`, and in 23 of them the PID came back from a pidfile.
# `wait` only works on children of the calling shell; a daemon started inside a
# subshell (`( "$ORCHD" ... & echo $! > daemon.pid )`) is not one, so `wait`
# returned in 0s with the daemon still alive, and the cleanup's `rm -rf` ran
# against a live writer every single time. The error only surfaced when the
# daemon happened to be flushing — CI run 30795701182 went red on `parity` while
# that gate's own output said "11 passed, 0 failed". The mechanism is asserted
# re-runnably by scripts/qa/probe-daemon-wait-shapes.sh.
#
# This file replaces those 25 divergent copies with one stop that actually
# waits. It handles both shapes: for a non-child (pidfile) the `kill -0` poll is
# the only wait there is; for a real child (`$!`) the poll alone would spin on a
# zombie — `kill -0` succeeds on an exited-but-unreaped child — so liveness is
# "exists and is not state Z", and the final `wait` reaps the child case. Do not
# simplify either half away: dropping the Z test makes every `$!` site burn the
# full grace period on a corpse and then report it unkillable; dropping the
# trailing `wait` leaks a zombie per stop.
#
# Timeouts are FR-159's, copied not re-derived (DD-171): SIGTERM, 10s poll,
# named escalation to SIGKILL, 5s poll, named failure. A daemon that does not
# die is a printed fact, never a silent continue.
#
# What this deliberately does not do: reap the daemon's descendants. A daemon
# stopped is not its children stopped — process-group reclamation needs the
# gate to have recorded session PIDs as they appeared, which is the gate's
# domain knowledge (see reclaim_recorded_sessions in
# scripts/qa/test-agent-session-control-plane.sh, which stays where it is).
#
# Sourced, not executed. Installs no traps, so it composes with
# gate_runlog_arm's trap chaining in either order:
#   . "$REPO_ROOT/scripts/lib/gate_daemon.sh"
#   ...
#   DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
#   ...
#   cleanup() {
#     gate_daemon_stop "$DAEMON_PID" || true   # `|| true`: the named lines are
#     DAEMON_PID=""                            # already printed; cleanup must
#     rm -rf "$QA_ROOT"                        # not overwrite the verdict.
#   }
# Mid-script restart sites call it without `|| true` — a stuck daemon should
# fail the gate, by name — and pass the daemon's own data-dir pidfile as the
# second argument so the next start observes a released directory:
#   gate_daemon_stop "$DAEMON_PID" "$ORCHESTRATORD_DATA_DIR/daemon.pid"
#   DAEMON_PID=""

# Tenths of a second, overridable per gate. Defaults match FR-159's measured
# choices: every recorded group exited on SIGTERM alone, well inside 10s.
GATE_DAEMON_TERM_GRACE_TENTHS="${GATE_DAEMON_TERM_GRACE_TENTHS:-100}"
GATE_DAEMON_KILL_GRACE_TENTHS="${GATE_DAEMON_KILL_GRACE_TENTHS:-50}"
GATE_DAEMON_RELEASE_GRACE_TENTHS="${GATE_DAEMON_RELEASE_GRACE_TENTHS:-50}"

# The one sanctioned way to turn a pidfile into a PID. Echoes the PID; returns
# 1 with a named line when the file is missing, empty, or not a number — which
# turns "the daemon start never wrote a pidfile" into a failure at start time
# instead of a silent no-op kill at teardown.
gate_daemon_pid_from_file() {
  local pidfile="$1" pid
  if [[ ! -f "$pidfile" ]]; then
    echo "[gate-daemon] $pidfile: no pidfile; the daemon start never wrote one" >&2
    return 1
  fi
  pid="$(tr -d '[:space:]' < "$pidfile")"
  case "$pid" in
    '' | *[!0-9]*)
      echo "[gate-daemon] $pidfile: content ${pid:-<empty>} is not a pid" >&2
      return 1
      ;;
  esac
  printf '%s\n' "$pid"
}

# True while the process exists and is not a zombie. `kill -0` alone is wrong
# for direct children: an exited child stays signalable until reaped.
gate_daemon_alive() {
  local pid="$1" state
  kill -0 "$pid" 2>/dev/null || return 1
  state="$(ps -o stat= -p "$pid" 2>/dev/null)" || return 1
  case "$state" in
    *Z*) return 1 ;;
  esac
  return 0
}

# Wait until the daemon can actually serve, not merely until it answers.
#
# This replaces 24 hand-copied loops across 23 gates (FR-163). Every one of them
# spelled the same thing — `for _ in {1..N}; do "$ORCH" task list -o json
# >/dev/null 2>&1 && break; sleep 0.25; done` — and they disagreed about N:
# five different budgets, 7.5s, 10s, 15s, 20s and 25s, none of them derived
# from anything. A gate that waited 7.5s on a loaded CI runner failed for a
# reason that had nothing to do with what it was testing.
#
# Two things the copies got wrong beyond the budget:
#
#   * `task list` is a proxy. It succeeds the moment the socket accepts a
#     connection, which is before the worker pool has registered — so a gate
#     could create a task and watch nothing pick it up. `daemon status
#     --wait-ready` polls the Health RPC, which reports migrations, keyring and
#     workers separately and only says ready when all three are.
#   * `&& break` silently continues when the loop is exhausted. The gate then
#     runs its whole body against a daemon that never came up and fails
#     somewhere further down, naming the wrong thing. This returns 1 with the
#     daemon's own last subsystem report, so the failure names itself.
#
# Usage, replacing the hand-written loop after a daemon start:
#   gate_daemon_wait_ready "$ORCH" || abort_with_summary "daemon never became ready"
# The timeout is the second argument, in seconds, defaulting to the widest of
# the five budgets it replaces — no gate gets a shorter wait than it used to
# have, so this cannot introduce a flake that was not already there.
GATE_DAEMON_READY_TIMEOUT="${GATE_DAEMON_READY_TIMEOUT:-25}"

# Does this CLI know `--wait-ready`?
#
# Not every binary a gate starts is the one being built. FR-113's vertical gate
# pins PREVIOUS_REF to the 0.5.0 release cut *by design* — its subject is that
# the previous binary still serves the current schema — and that binary predates
# `--wait-ready` by ten days. Asking it for the flag produced
# `error: unexpected argument '--wait-ready' found`, which the helper reported
# as "daemon not ready within 25s". Four gates were red for a fortnight on a
# daemon that had started perfectly well.
#
# The probe is the CLI's own help output, which is where clap declares the flags
# it accepts, so this asks the binary rather than a table of which callers are
# old. An opt-in `GATE_DAEMON_LEGACY_READINESS=1` was the obvious alternative
# and is the enumeration failure §4.4 shape 2 names: it would guard exactly the
# caller we already knew about, and the next old-binary caller would rediscover
# this bug from scratch.
#
# Read into a variable, never `"$cli" ... | grep -q`. Under `set -o pipefail`
# grep leaves on the first match, the producer dies of EPIPE, and a successful
# match reports as a failed one (FR-145).
gate_daemon_supports_wait_ready() {
  local cli="$1" help
  help="$("$cli" daemon status --help 2>&1)" || true
  grep -q -- '--wait-ready' <<<"$help"
}

# The pre-FR-163 readiness poll, kept for binaries that cannot answer the Health
# RPC. Weaker on purpose and only reachable when the flag is absent.
#
# `task list` succeeds as soon as the socket accepts a connection, which is
# before the worker pool has registered — that imprecision is exactly why
# FR-163 replaced it, and it is still the best a pre-Health binary can offer.
# What is *not* carried over is the `&& break` that let an exhausted loop fall
# through silently: this returns 1 and says so, so a gate never runs its body
# against a daemon that never came up.
gate_daemon_wait_ready_legacy() {
  local cli="$1" timeout="$2" deadline

  echo "[gate-daemon] ${cli} does not support --wait-ready; falling back to a" >&2
  echo "[gate-daemon] socket probe, which cannot see worker registration" >&2

  deadline=$(( SECONDS + timeout ))
  while (( SECONDS < deadline )); do
    if "$cli" task list -o json >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done

  echo "[gate-daemon] daemon did not accept a connection within ${timeout}s" >&2
  echo "[gate-daemon] (legacy socket probe; this binary predates the Health RPC)" >&2
  return 1
}

gate_daemon_wait_ready() {
  local cli="$1" timeout="${2:-$GATE_DAEMON_READY_TIMEOUT}" output rc

  if [[ -z "$cli" ]]; then
    echo "[gate-daemon] gate_daemon_wait_ready: no CLI path given" >&2
    return 1
  fi

  if ! gate_daemon_supports_wait_ready "$cli"; then
    gate_daemon_wait_ready_legacy "$cli" "$timeout"
    return $?
  fi

  # Status is observed, never discarded: a readiness wait whose own failure is
  # invisible is the thing this function replaces (§4.4 shape 5).
  output="$("$cli" daemon status --wait-ready --timeout "$timeout" 2>&1)"
  rc=$?
  if (( rc != 0 )); then
    echo "[gate-daemon] daemon not ready within ${timeout}s:" >&2
    sed 's/^/  /' <<<"$output" >&2
    return 1
  fi
  return 0
}

# Kill a daemon outright, for gates whose subject is what an *unclean* exit
# leaves behind. SIGKILL only: no SIGTERM first, because a daemon that handles
# SIGTERM unlinks its socket and pidfile on the way out, which is precisely the
# debris such a gate needs to survive.
#
# This lives here rather than in the calling gate because the enforcement
# surface forbids signalling a daemon PID outside this library, and that rule is
# right — `wait` on a pidfile PID is a no-op, so a hand-rolled crash-stop
# reports "killed" while the process is still running and the cleanup's `rm -rf`
# races it. The correct response to needing a signal the contract lacks is to
# add it to the contract, not to rename the variable until the scan stops
# looking. Liveness is "exists and is not state Z" for the same reason
# gate_daemon_stop uses that test: `kill -0` succeeds on an unreaped child.
#
# Returns 1, having printed why, if the process is still alive afterwards.
gate_daemon_kill_hard() {
  local pid="$1" waited=0
  [[ -n "$pid" ]] || return 0

  kill -KILL "$pid" 2>/dev/null || true
  while gate_daemon_alive "$pid" && ((waited < GATE_DAEMON_KILL_GRACE_TENTHS)); do
    sleep 0.1
    waited=$((waited + 1))
  done
  # Reaps the direct-child case; an instant failing no-op for a pidfile PID.
  wait "$pid" 2>/dev/null || true

  if gate_daemon_alive "$pid"; then
    echo "[gate-daemon] pid $pid survived SIGKILL" >&2
    return 1
  fi
  return 0
}

# Stop a daemon and confirm it actually stopped. Idempotent: an empty PID is a
# no-op, so callers reset their variable (`DAEMON_PID=""`) after each call and
# conditional starts need no guard. Returns 1 (having printed why) when the
# process outlives both grace periods, or when release_pidfile never vanishes.
gate_daemon_stop() {
  local pid="$1" release_pidfile="${2:-}" waited
  [[ -n "$pid" ]] || return 0

  if gate_daemon_alive "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    waited=0
    while gate_daemon_alive "$pid" && (( waited < GATE_DAEMON_TERM_GRACE_TENTHS )); do
      sleep 0.1
      waited=$((waited + 1))
    done
    if gate_daemon_alive "$pid"; then
      echo "[gate-daemon] pid $pid did not exit within $((GATE_DAEMON_TERM_GRACE_TENTHS / 10))s of SIGTERM; escalating to SIGKILL" >&2
      kill -KILL "$pid" 2>/dev/null || true
      waited=0
      while gate_daemon_alive "$pid" && (( waited < GATE_DAEMON_KILL_GRACE_TENTHS )); do
        sleep 0.1
        waited=$((waited + 1))
      done
      if gate_daemon_alive "$pid"; then
        echo "[gate-daemon] pid $pid survived SIGKILL" >&2
        return 1
      fi
    fi
  fi

  # Reaps the direct-child case; instant, failing no-op for the pidfile case.
  wait "$pid" 2>/dev/null || true

  # Only restart sites pass this: the daemon removes its own data-dir pidfile
  # on shutdown (a different file from the harness-written one the PID was read
  # from), and the instance guard refuses a directory that still has one.
  if [[ -n "$release_pidfile" ]]; then
    waited=0
    while [[ -f "$release_pidfile" ]] && (( waited < GATE_DAEMON_RELEASE_GRACE_TENTHS )); do
      sleep 0.1
      waited=$((waited + 1))
    done
    if [[ -f "$release_pidfile" ]]; then
      echo "[gate-daemon] $release_pidfile still present $((GATE_DAEMON_RELEASE_GRACE_TENTHS / 10))s after pid $pid exited; the next start would refuse the data directory" >&2
      return 1
    fi
  fi

  return 0
}
