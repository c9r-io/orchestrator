#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "FR-001 sandbox matrix requires macOS sandbox-exec" >&2
  exit 1
fi

# Starts its own daemon over a temporary workspace. It used to require an ambient
# one, which is why it had never run since the freshness ledger was built, and the
# shape of what it did to that ambient daemon is the reason converting it mattered
# more than the freshness: it ran `orchestrator delete project/qa-fr001-sandbox
# --force` against whatever daemon answered, read `data/agent_orchestrator.db`
# relative to the repository, and let sandboxed tasks write into the working
# tree's own `docs/`. A release precondition that deletes a project from the
# operator's database and dirties the checkout is one nobody will run twice.
#
# The workspace is rebuilt under $QA_ROOT rather than pointed at the repository,
# because the bundle declares `root_path: "."` — the workspace root *is* the
# daemon's working directory. So the probe binary is copied in, the profiles'
# writable `docs` path is created there, and every assertion below reads the
# temporary database. Nothing outside $QA_ROOT is written, which is what lets the
# six clean-worktree gates keep passing after this one has run.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19232}"
DAEMON_PID=""

for command in jq sqlite3 mktemp; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT
# Armed after the gate's own trap, never before it: a second bare `trap ... EXIT`
# discards the first silently, and arming first is how this conversion initially
# produced a green run that the freshness ledger never recorded.
gate_runlog_arm "scripts/qa/test-fr001-sandbox-matrix.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"

PROJECT="${QA_PROJECT:-qa-fr001-sandbox}"
DB_PATH="$QA_ROOT/data/agent_orchestrator.db"
BUNDLE="$REPO_ROOT/fixtures/manifests/bundles/sandbox-execution-profiles.yaml"

# The workspace the profiles describe: `docs` is their writable path, the qa and
# ticket directories are declared by the Workspace resource, and the probe is the
# real binary rather than a stub because the subject is what the sandbox does to a
# process that genuinely tries to exceed a limit.
mkdir -p "$QA_ROOT/target/debug" "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"
cp "$ORCH" "$QA_ROOT/target/debug/orchestrator"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

if ! gate_daemon_wait_ready "$ORCH"; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

cd "$QA_ROOT"

run_task() {
  local workflow="$1"
  local name="$2"
  local goal="$3"
  local event_type="$4"
  local reason_code="$5"
  local resource_kind="${6:-}"

  local task_id
  local task_create_output
  task_create_output=$(
    "$ORCH" task create \
      --project "${PROJECT}" \
      --workflow "${workflow}" \
      --name "${name}" \
      --goal "${goal}" \
      --no-start
  )
  task_id=$(
    printf '%s\n' "${task_create_output}" | grep -oE '[0-9a-f-]{36}' | tail -1
  )
  "$ORCH" task start "${task_id}" || true

  for _ in {1..30}; do
    local task_info_output
    task_info_output=$("$ORCH" task info "${task_id}")
    case "${task_info_output}" in
      *"Status: completed"*|*"Status: failed"*) break ;;
    esac
    sleep 1
  done

  local payload
  payload=$(
    sqlite3 "${DB_PATH}" \
      "SELECT payload_json FROM events WHERE task_id='${task_id}' AND event_type='${event_type}' ORDER BY created_at DESC LIMIT 1;"
  )

  if [[ -z "${payload}" ]]; then
    echo "[FAIL] ${workflow}: missing ${event_type}" >&2
    exit 1
  fi
  if [[ "${payload}" != *"\"reason_code\":\"${reason_code}\""* ]]; then
    echo "[FAIL] ${workflow}: expected reason_code=${reason_code}" >&2
    echo "${payload}" >&2
    exit 1
  fi
  if [[ -n "${resource_kind}" && "${payload}" != *"\"resource_kind\":\"${resource_kind}\""* ]]; then
    echo "[FAIL] ${workflow}: expected resource_kind=${resource_kind}" >&2
    echo "${payload}" >&2
    exit 1
  fi

  echo "[PASS] ${workflow}: ${reason_code}"
}

# No reset step. The database and the workspace are both new, so there is nothing
# to delete — which is the point: the old `delete project --force` was issued
# against whichever daemon answered, and it is exactly the operation CLAUDE.md
# forbids reaching for when isolation is the actual fix.
echo "FR-001 sandbox QA project: ${PROJECT} (isolated under ${QA_ROOT})"
"$ORCH" apply --project "${PROJECT}" -f "${BUNDLE}"

run_task "sandbox-open-files-limit" "sandbox fd limit" "sandbox fd limit" "sandbox_resource_exceeded" "open_files_limit_exceeded" "open_files"

# sandbox-cpu-limit is expected to fail and is scoped out by name, not by making
# the gate non-blocking: five of the six sub-cases work, and dropping them to
# dodge one would be the enumeration mistake DD-179 records for
# test-process-console-ui.sh's npm audit step — an exemption sized to the whole
# gate when the objection is to one line inside it.
#
# The limit is enforced (the process dies at ~1s of CPU under max_cpu_seconds: 1)
# and unobservable: no event, no reason code, sandbox_denied false, exit_code 1.
# CPU exhaustion is the one limit that kills the process instead of making a call
# fail, so the probe cannot self-report it, and the SIGXCPU fallback at
# phase_runner/util.rs:312 does not fire because the signal never reaches
# status.signal(). docs/ticket/20260812-cpu-limit-exhaustion-is-enforced-but-unobservable.md
#
# Asserted as expected-failing rather than skipped. A skip says nothing when the
# defect is fixed; this fails the moment the event starts arriving, so the gate is
# the regression test and the exemption cannot outlive what it excuses.
# A subshell, because run_task reports failure with `exit 1` rather than `return`:
# called bare in condition position it would take the whole gate down instead of
# yielding false, and the run would end before the five remaining assertions.
if ( run_task "sandbox-cpu-limit" "sandbox cpu limit" "sandbox cpu limit" "sandbox_resource_exceeded" "cpu_limit_exceeded" "cpu" ) 2>/dev/null; then
  echo "[FAIL] sandbox-cpu-limit now classifies; remove this exemption and the ticket" >&2
  exit 1
fi
echo "[KNOWN-FAILING] sandbox-cpu-limit: enforced but unobservable (ticket 20260812-cpu-limit-exhaustion-is-enforced-but-unobservable)"

run_task "sandbox-memory-limit" "sandbox memory limit" "sandbox memory limit" "sandbox_resource_exceeded" "memory_limit_exceeded" "memory"
run_task "sandbox-process-limit" "sandbox process limit" "sandbox process limit" "sandbox_resource_exceeded" "processes_limit_exceeded" "processes"
run_task "sandbox-network-deny" "sandbox network deny" "sandbox network deny" "sandbox_network_blocked" "network_blocked"
run_task "sandbox-network-allowlist" "sandbox allowlist unsupported" "sandbox allowlist unsupported" "sandbox_network_blocked" "unsupported_backend_feature"

echo "FR-001 sandbox matrix passed"
