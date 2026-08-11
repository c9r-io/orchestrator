#!/usr/bin/env bash
# FR-165 / the --wait-ready ticket: gate_daemon_wait_ready must work against a
# binary that predates the Health RPC.
#
# Why this exists as its own gate. `c1060338` centralised 24 hand-written
# readiness polls onto `orchestrator daemon status --wait-ready`, which was
# right. One of the 24 callers does not start the binary being built:
# test-slack-skill-automation-vertical.sh pins PREVIOUS_REF to the 0.5.0 cut
# *because* its subject is that the previous release still serves the current
# schema. That binary has no such flag, so the helper reported
# `error: unexpected argument` as "daemon not ready within 25s", and four gates
# were red for a fortnight on daemons that had started fine.
#
# The repair degrades to a socket probe when the flag is absent. Nothing that
# runs against the current binary can observe that branch — it would pass while
# the fallback was broken, which is how the original defect survived a
# 23-gate migration. So the CLI here is a stub whose capabilities the fixture
# chooses, and every case asserts the *diagnostic*, not just the exit code:
# "it failed" cannot distinguish the branch that failed from any other.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# shellcheck source=/dev/null
. "$REPO_ROOT/scripts/lib/gate_daemon.sh"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr165-readiness.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# A stub CLI. $1 decides whether `daemon status --help` advertises --wait-ready;
# $2 decides whether `task list` ever succeeds. Writing a stub rather than
# doctoring a real binary keeps the case hermetic and fast, and lets the
# "modern" and "legacy" binaries differ in exactly one declared capability.
make_cli() {
  local path="$1" supports="$2" task_list="$3"
  cat > "$path" <<EOF
#!/usr/bin/env bash
set -uo pipefail
if [[ "\${1:-}" == "daemon" && "\${2:-}" == "status" ]]; then
  for arg in "\$@"; do
    if [[ "\$arg" == "--help" ]]; then
      echo "Usage: orchestrator daemon status [OPTIONS]"
      echo "Options:"
      $( [[ "$supports" == "yes" ]] && echo 'echo "      --wait-ready  Wait for readiness"' )
      echo "      --timeout <SECS>"
      exit 0
    fi
    if [[ "\$arg" == "--wait-ready" ]]; then
      if [[ "$supports" == "yes" ]]; then
        echo "serving"; exit 0
      fi
      echo "error: unexpected argument '--wait-ready' found" >&2
      echo "Usage: orchestrator daemon status [OPTIONS]" >&2
      exit 2
    fi
  done
  exit 0
fi
if [[ "\${1:-}" == "task" && "\${2:-}" == "list" ]]; then
  [[ "$task_list" == "yes" ]] && exit 0
  exit 1
fi
exit 0
EOF
  chmod +x "$path"
}

# `set -e` is off inside a condition, so status and output are captured
# explicitly rather than inferred.
run_wait() {
  local cli="$1" timeout="$2" out status
  out="$(gate_daemon_wait_ready "$cli" "$timeout" 2>&1)" && status=0 || status=$?
  LAST_OUTPUT="$out"
  return "$status"
}

MODERN="$WORK/orchestrator-modern"
LEGACY="$WORK/orchestrator-legacy"
LEGACY_DEAD="$WORK/orchestrator-legacy-dead"
make_cli "$MODERN"      yes yes
make_cli "$LEGACY"      no  yes
make_cli "$LEGACY_DEAD" no  no

# ── 1. capability probe reads the binary, not a list of callers ───────────────
if gate_daemon_supports_wait_ready "$MODERN"; then
  pass "a CLI whose help advertises --wait-ready is detected as supporting it"
else
  fail "the modern stub was not detected as supporting --wait-ready"
fi

if gate_daemon_supports_wait_ready "$LEGACY"; then
  fail "a CLI whose help omits --wait-ready was detected as supporting it"
else
  pass "a CLI whose help omits --wait-ready is detected as not supporting it"
fi

# ── 2. the modern path is still taken when the flag exists ───────────────────
# Without this the fallback could swallow every caller and all 24 gates would
# silently lose the worker-registration guarantee FR-163 bought.
if run_wait "$MODERN" 5; then
  if grep -q 'falling back' <<< "$LAST_OUTPUT"; then
    fail "a CLI supporting --wait-ready was sent down the legacy path anyway"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  else
    pass "a CLI supporting --wait-ready uses the Health RPC path, not the fallback"
  fi
else
  fail "the modern path failed against a stub that reports serving"
  echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
fi

# ── 3. the regression itself ─────────────────────────────────────────────────
# This is the case that was failing in production: a ready daemon behind a CLI
# with no --wait-ready. It must succeed, and it must say it degraded.
if run_wait "$LEGACY" 5; then
  if grep -q 'does not support --wait-ready' <<< "$LAST_OUTPUT"; then
    pass "a ready daemon behind a pre-Health CLI is ready, and the log says the probe degraded"
  else
    fail "it succeeded but never announced the weaker guarantee"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
else
  fail "a ready daemon behind a pre-Health CLI was reported not ready — the original defect"
  echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
fi

# ── 4. the fallback still fails closed ───────────────────────────────────────
# The pre-FR-163 loops ended in `&& break`, so an exhausted wait fell through
# and the gate ran its whole body against a daemon that never came up, failing
# somewhere further down and naming the wrong thing. The fallback must not
# reintroduce that.
start=$SECONDS
if run_wait "$LEGACY_DEAD" 2; then
  fail "a daemon that never accepts a connection was reported ready"
else
  elapsed=$(( SECONDS - start ))
  if ! grep -q 'did not accept a connection' <<< "$LAST_OUTPUT"; then
    fail "it failed, but not through the legacy exhaustion branch"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  elif (( elapsed < 2 )); then
    fail "returned after ${elapsed}s against a 2s budget — it did not wait"
  else
    pass "an unreachable daemon fails closed after its budget, naming the legacy branch"
  fi
fi

# ── 5. an empty CLI argument is still rejected ───────────────────────────────
# The probe runs before the existing guard, so a caller passing "" must not
# reach the fallback and spend the whole timeout on it.
if run_wait "" 2; then
  fail "an empty CLI path was accepted"
else
  if grep -q 'no CLI path given' <<< "$LAST_OUTPUT"; then
    pass "an empty CLI path is rejected by name, not by timing out in the fallback"
  else
    fail "rejected, but not for the empty path"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
fi

echo ""
echo "FR-165 gate_daemon readiness fallback: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
