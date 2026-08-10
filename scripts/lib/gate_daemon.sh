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
