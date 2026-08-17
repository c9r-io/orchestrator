#!/usr/bin/env bash

set -euo pipefail

# FR-158: the freshness ledger is written by the manual-runbook gates, and two
# of them are invoked from ci-required gates. This gate refuses to run on a dirty
# worktree, so it reads the tree through the shared predicate that excludes that
# one file — otherwise it fails for the recorder's reason rather than its own.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19326}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/agent-driver-production-parity.yaml"
BASELINE="$REPO_ROOT/fixtures/driver/legacy-agent-execution-baseline.json"
FAKE_CLAUDE="$REPO_ROOT/scripts/qa/fixtures/fake-claude-agent-driver-migration.sh"
LEDGER="$REPO_ROOT/config/governance/coordination-collapse-ledger.json"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
ORIGINAL_CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
ORIGINAL_RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
DAEMON_PID=""
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"
# shellcheck source=../lib/gate_daemon.sh
. "$REPO_ROOT/scripts/lib/gate_daemon.sh"

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  # gate_scratch_has_evidence: a retained root that holds nothing is
  # indistinguishable from a gate that ran and produced no findings. Either root
  # holding something retains both, because the line below names both.
  if [[ "$FAIL" -gt 0 || "${KEEP_FR126_QA:-0}" == "1" ]] &&
    { gate_scratch_has_evidence "$QA_ROOT" || gate_scratch_has_evidence "$QA_HOME"; }; then
    echo "FR-126 production parity retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    # A cleanup failure must not overwrite the gate's verdict, and must not be
    # silent either. Observed in run 30795701182: the assertions had already
    # reported "11 passed, 0 failed" when this `rm` raced a session child still
    # writing into $QA_ROOT/data, and `Directory not empty` turned the step red
    # with nothing in the log connecting it to what the gate tests.
    # `gate_daemon_stop` above confirms the daemon exited, not the process
    # group beneath it — FR-159's subject, surfacing here in the harness
    # rather than the product.
    #
    # So: settle and retry once, then say plainly what leaked. The gate's
    # subject is driver parity; a temp directory that outlives it is a fact
    # worth printing, not a verdict.
    if ! rm -rf "$QA_ROOT" "$QA_HOME" 2>/dev/null; then
      sleep 1
      rm -rf "$QA_ROOT" "$QA_HOME" 2>/dev/null ||
        echo "warning: $QA_ROOT or $QA_HOME survived cleanup; a child process is still writing there" >&2
    fi
  fi
  return 0
}
trap cleanup EXIT

for command in cargo git jq mktemp rg ruby shasum sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR126_ALLOW_DIRTY:-0}" != "1" && -n "$(gate_runlog_worktree_status "$(git rev-parse --show-toplevel)")" ]]; then
  echo "FR-126 production parity requires a clean worktree (or FR126_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

# The four aborts below are premise checks: this block asserts that the mock
# fixture is still bound to the production Agents it claims to mirror. Each one
# names a production object by name, which is exactly the enumerated target
# FR-143 is about — `docs/workflow/hello-world.yaml`, `echo-agent`, `streamer`.
# When one of those moves, the abort fires, and until FR-143 that took the whole
# run down with it: the assertion below never printed, no count moved, and the
# summary line a reader stops at never appeared.
#
# Wrapped, the aborts keep their words and become the diagnosis. The pass they
# guard is skipped rather than reported, which is the difference between a
# fixture that says its premise is gone and one that says nothing at all.
if fixture_premise "production fixture bindings" ruby -ryaml -rjson -rdigest -e '
  fixture_path, baseline_path = ARGV
  fixture = YAML.load_stream(File.read(fixture_path)).compact
  baseline = JSON.parse(File.read(baseline_path)).fetch("contracts")
  agents = fixture.select { |document| document["kind"] == "Agent" }
    .to_h { |document| [document.dig("metadata", "name"), document] }
  mappings = {
    "hello-world" => ["docs/workflow/hello-world.yaml", "echo-agent", "hello"],
    "scheduled-scan" => ["docs/workflow/scheduled-scan.yaml", "scan-agent", "scheduled"],
    "fr-watch" => ["docs/workflow/fr-watch.yaml", "fr-governance-agent", "fr-watch"]
  }
  mappings.each do |contract, (path, production_name, fixture_prefix)|
    production = YAML.load_stream(File.read(path)).compact.find do |document|
      document["kind"] == "Agent" && document.dig("metadata", "name") == production_name
    end
    abort("missing production Agent #{production_name}") unless production
    typed = agents.fetch("#{fixture_prefix}-typed")
    command = production.dig("spec", "command")
    abort("#{contract} fixture command drift") unless typed.dig("spec", "command") == command
    abort("#{contract} target driver drift") unless
      production.dig("spec", "driver", "provider") == "shell" &&
      typed.dig("spec", "driver", "provider") == "shell" &&
      baseline.dig(contract, "targetDriver") == "shell/cli"
  end
  production_streamer = YAML.load_stream(
    File.read("docs/workflow/streaming-mark-done-convergence.yaml")
  ).compact.find do |document|
    document["kind"] == "Agent" && document.dig("metadata", "name") == "streamer"
  end
  fixture_streamer = agents.fetch("streaming-typed")
  %w[provider transport options].each do |field|
    abort("streaming fixture #{field} drift") unless
      fixture_streamer.dig("spec", "driver", field) ==
        production_streamer.dig("spec", "driver", field)
  end
  puts "production fixture bindings: ok"
' "$FIXTURE" "$BASELINE"; then
  pass "mock-only fixture commands and drivers are bound to all four production migration objects"
fi

SOURCE_COMMIT="$(jq -r '.sourceCommit' "$BASELINE")"
if git cat-file -e "$SOURCE_COMMIT^{commit}" &&
   git merge-base --is-ancestor "$SOURCE_COMMIT" HEAD; then
  pass "recorded legacy contract is anchored to an ancestor commit"
else
  fail "recorded legacy contract source commit is unavailable or unrelated"
fi

OPEN_COMMIT="$(jq -r '.retirement.shellRunnerExecutor.compatibilityWindow.openedByCommit' "$LEDGER")"
CLOSE_COMMIT="$(jq -r '.retirement.shellRunnerExecutor.compatibilityWindow.closedByCommit' "$LEDGER")"
if git merge-base --is-ancestor "$OPEN_COMMIT" "$CLOSE_COMMIT" &&
   git merge-base --is-ancestor "$CLOSE_COMMIT" HEAD; then
  pass "legacy runtime compatibility window has an ordered, reachable commit interval"
else
  fail "compatibility window commits are missing or unordered"
fi

# Asserted against the recorded rollback anchor, not the moving worktree. The
# worktree form decayed the first time a legitimate edit landed inside the
# patch's scope — a rustfmt reflow (71e3dfe5) moved validate.rs's context lines
# and a 3-way merge conflicts too, so "reverse-applies to HEAD" is a claim no
# amount of governance can keep true. Retirement closed with FR-126; what the
# evidence has to keep proving is that the recorded rollback point exists in
# reachable history and the patch really reverse-applies to it. The anchor and
# the date its worktree form stopped holding are written in the ledger beside
# the window it belongs to.
ROLLBACK_ANCHOR="$(jq -r '.retirement.shellRunnerExecutor.compatibilityWindow.reverseApplicableAt' "$LEDGER")"
ROLLBACK_INDEX="$QA_ROOT/rollback-anchor.index"
if [[ -n "$ROLLBACK_ANCHOR" && "$ROLLBACK_ANCHOR" != "null" ]] &&
   git cat-file -e "$ROLLBACK_ANCHOR^{commit}" &&
   git merge-base --is-ancestor "$ROLLBACK_ANCHOR" HEAD &&
   GIT_INDEX_FILE="$ROLLBACK_INDEX" git read-tree "$ROLLBACK_ANCHOR" &&
   git diff "$CLOSE_COMMIT^" "$CLOSE_COMMIT" -- \
     core/src/resource/runtime_policy.rs \
     crates/orchestrator-config/src/config/runner.rs \
     crates/orchestrator-runner/src \
     crates/orchestrator-scheduler/src/scheduler/phase_runner |
     GIT_INDEX_FILE="$ROLLBACK_INDEX" git apply --cached -R --check; then
  pass "runner-removal source patch reverse-applies at the recorded rollback anchor"
else
  fail "runner-removal source patch no longer has executable rollback evidence at its recorded anchor"
fi

cargo build -p orchestratord -p orchestrator-cli >/dev/null
cargo build -p orchestrator-runner --bin orch-mcp-tools >/dev/null

export HOME="$QA_HOME"
export CARGO_HOME="$ORIGINAL_CARGO_HOME"
export RUSTUP_HOME="$ORIGINAL_RUSTUP_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
export FR126_FAKE_TRACE="$QA_ROOT/workspace/.fr126-fake-trace"
mkdir -p "$QA_ROOT/workspace/docs/qa" "$QA_ROOT/workspace/docs/ticket" "$QA_ROOT/bin"
printf '# FR-126 production parity\n' > "$QA_ROOT/workspace/docs/qa/pilot.md"
cp "$FAKE_CLAUDE" "$QA_ROOT/bin/claude"
chmod 700 "$QA_ROOT/bin/claude"
export PATH="$QA_ROOT/bin:$PATH"

# The shadow is the only barrier between this run and a real provider CLI: the
# bundle it applies declares provider: claude with no fake binary pin. Assert it
# is actually in effect rather than assuming the line above took — a commented
# out or reordered export leaves this gate spending real credentials, and the
# surface gate cannot see that from the outside. FR-134 requirement 2.
. "$REPO_ROOT/scripts/lib/provider_isolation.sh"
assert_provider_shadow "$QA_ROOT/bin" claude
# The entry-level check resolves in this script's non-login shell; the runner
# resolves under the shell/arg the fixture's RuntimePolicy declares (-c). Both
# must land in the shadow — the streaming step once reached a real claude
# through the -lc/path_helper gap while the line above passed (FR-161). Keep
# the shell/arg here in sync with the fixture's RuntimePolicy.
assert_provider_resolution /bin/bash -c "$QA_ROOT/bin" claude

(
  cd "$QA_ROOT/workspace"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 --webhook-bind none \
    --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
if ! gate_daemon_wait_ready "$ORCH"; then
  sed -n '1,260p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  exit 1
fi

PROJECT="qa-agent-driver-production-parity"
(
  cd "$QA_ROOT/workspace"
  "$ORCH" apply --project "$PROJECT" -f "$FIXTURE" > "$QA_ROOT/apply.out" 2>&1
)
# Before FR-173 this counted three [legacy_agent_command_deprecated] warnings.
# The promotion is gone, so the assertion is inverted — and inverting it alone
# would be satisfied by an apply that failed outright and therefore warned about
# nothing. The three Agents must be present with a shell driver afterwards.
if rg -q 'legacy_agent_command_deprecated' "$QA_ROOT/apply.out"; then
  cat "$QA_ROOT/apply.out" >&2
  fail "a retired promotion warning is still emitted"
else
  applied_typed=0
  for fixture_agent in hello-typed scheduled-typed fr-watch-typed; do
    if (cd "$QA_ROOT/workspace" &&
        "$ORCH" get agent "$fixture_agent" --project "$PROJECT" -o json) |
        jq -e '.spec.driver.provider == "shell"' >/dev/null; then
      applied_typed=$((applied_typed + 1))
    fi
  done
  if [[ "$applied_typed" -eq 3 ]]; then
    pass "the three production shell contracts apply with explicit drivers and no promotion"
  else
    cat "$QA_ROOT/apply.out" >&2
    fail "expected 3 typed shell Agents after apply, found $applied_typed"
  fi
fi

create_and_wait() {
  local workflow="$1"
  local task_id status
  # `rg -o` reads to end of input, so nothing upstream is left writing; the first id is
  # then taken by parameter expansion rather than by `| head -1`, which under
  # `set -o pipefail` kills the producer and takes the whole gate with it (FR-146).
  local created_ids
  created_ids="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace driver-migration-parity \
      --workflow "$workflow" --target-file docs/qa/pilot.md \
      --goal "FR-126 production migration parity" --name "$workflow" --no-start |
      rg -o '[0-9a-f-]{36}'
  )"
  task_id="${created_ids%%$'\n'*}"
  "$ORCH" task start "$task_id" >/dev/null
  status="pending"
  for _ in {1..240}; do
    status="$("$ORCH" task info "$task_id" -o json | jq -r '.task.status')"
    [[ "$status" =~ ^(completed|failed|cancelled)$ ]] && break
    sleep 0.25
  done
  printf '%s|%s\n' "$task_id" "$status"
}

DB="$QA_ROOT/data/agent_orchestrator.db"
EVIDENCE='[]'
# Each contract used to run twice — once through a command-only Agent and once
# through a typed one — and the pass required the two to agree *and* to match the
# recorded pre-migration baseline. FR-173 retired the command-only form, which
# left the two declarations differing only in capability name, so the first
# conjunct compared a run against a rename of itself. The baseline comparison is
# the one that was always carrying the evidence (§4.3), and it is unchanged: the
# hash on the right-hand side still comes from a run recorded before the
# migration, not from anything this gate produces.
for contract in hello scheduled fr-watch; do
  typed="$(create_and_wait "parity-$contract-typed")"
  typed_id="${typed%%|*}"
  typed_status="${typed##*|}"
  typed_exit="$(sqlite3 "$DB" "SELECT exit_code FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='$typed_id') ORDER BY started_at DESC LIMIT 1;")"
  typed_stdout="$(sqlite3 "$DB" "SELECT stdout_path FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='$typed_id') ORDER BY started_at DESC LIMIT 1;")"
  typed_hash="$(shasum -a 256 "$typed_stdout" | awk '{print $1}')"
  baseline_key="$contract"
  [[ "$contract" == "hello" ]] && baseline_key="hello-world"
  [[ "$contract" == "scheduled" ]] && baseline_key="scheduled-scan"
  baseline_hash="$(jq -r --arg key "$baseline_key" '.contracts[$key].stdoutSha256' "$BASELINE")"
  # An absent baseline entry yields the string "null", which would compare equal
  # to a second "null" and pass on two things that were never measured.
  if [[ -z "$baseline_hash" || "$baseline_hash" == "null" ]]; then
    fail "$baseline_key has no recorded pre-migration stdout hash to compare against"
    continue
  fi
  typed_events="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$typed_id' AND event_type LIKE 'driver_%';")"
  if [[ "$typed_status" == "completed" && "$typed_exit" == "0" &&
        "$typed_hash" == "$baseline_hash" && "$typed_events" -gt 0 ]]; then
    pass "$baseline_key preserves terminal, exit, exact output, and normalized events"
  else
    fail "$baseline_key diverged from its recorded pre-migration contract"
  fi
  EVIDENCE="$(jq -c \
    --arg contract "$baseline_key" \
    --arg typed_task "$typed_id" \
    --arg terminal "$typed_status" \
    --arg stdout_sha256 "$typed_hash" \
    --arg baseline_sha256 "$baseline_hash" \
    --argjson typed_events "$typed_events" \
    '. + [{contract:$contract,typed_task:$typed_task,terminal:$terminal,stdout_sha256:$stdout_sha256,baseline_sha256:$baseline_sha256,typed_driver_events:$typed_events}]' \
    <<<"$EVIDENCE")"
done

streaming="$(create_and_wait parity-streaming-typed)"
streaming_id="${streaming%%|*}"
streaming_status="${streaming##*|}"
streaming_exit="$(sqlite3 "$DB" "SELECT exit_code FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='$streaming_id') ORDER BY started_at DESC LIMIT 1;")"
streaming_cycles="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$streaming_id' AND event_type='cycle_started';")"
tool_uses="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$streaming_id' AND event_type='driver_tool_use' AND payload_json LIKE '%mark_done%';")"
tool_results="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$streaming_id' AND event_type='driver_tool_result';")"
if [[ "$streaming_status" == "completed" && "$streaming_exit" == "0" &&
      "$streaming_cycles" -eq "$(jq -r '.contracts["streaming-mark-done"].maxCycles' "$BASELINE")" &&
      "$tool_uses" -gt 0 && "$tool_results" -gt 0 ]]; then
  pass "streaming-mark-done typed Claude matches recorded terminal, tool, event, and convergence contract"
else
  fail "streaming-mark-done typed Claude diverged from the recorded legacy contract"
fi

# The dump is captured rather than piped. `rg -q` leaves on the first match, and
# a whole-database dump is the one producer in this repository with no bound on
# its size: under `set -o pipefail` sqlite3's EPIPE becomes the condition's
# status, `!` inverts it, and a detected leak reports as "stays out" — at exactly
# the moment it must not (FR-145).
#
# An empty dump is its own failure, not a quiet pass. A condition that reads
# nothing and a condition that read everything and found nothing are the same
# exit code, and only one of them is evidence (§4.4 shape 5).
DB_DUMP="$(sqlite3 "$DB" .dump)" || DB_DUMP=""
if [[ -z "$DB_DUMP" ]]; then
  fail "sqlite3 produced no dump of $DB, so the leak assertion examined nothing"
elif ! rg -a -q '00000000-0000-4000-8000-000000000126' "$QA_ROOT/data" &&
     ! rg -q '00000000-0000-4000-8000-000000000126' <<< "$DB_DUMP"; then
  pass "provider session material stays out of persisted database evidence"
else
  fail "provider session material leaked into persisted database evidence"
fi

EVIDENCE="$(jq -c \
  --arg contract "streaming-mark-done" \
  --arg typed_task "$streaming_id" \
  --arg terminal "$streaming_status" \
  --argjson cycles "$streaming_cycles" \
  --argjson tool_uses "$tool_uses" \
  --argjson tool_results "$tool_results" \
  '. + [{contract:$contract,typed_task:$typed_task,terminal:$terminal,cycles:$cycles,driver_tool_uses:$tool_uses,driver_tool_results:$tool_results,session_material_persisted:false}]' \
  <<<"$EVIDENCE")"
printf '%s\n' "$EVIDENCE" | jq '.' > "$QA_ROOT/production-parity-evidence.json"

cargo test -p orchestrator-runner \
  cli_drivers_preserve_guaranteed_cancel_and_sandbox >/dev/null
cargo test -p orchestrator-runner \
  test_spawn_with_runner_and_capture_redacts_persisted_output >/dev/null
cargo test -p orchestrator-scheduler \
  detect_sandbox_violation_detects_operation_not_permitted >/dev/null
pass "shared cancellation, sandbox classification, and redaction substrate remains covered"

if [[ "$FAIL" -ne 0 ]]; then
  sed -n '1,360p' "$QA_ROOT/daemon.log" >&2
  echo "FR-126 production parity: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "FR-126 production parity: $PASS passed, 0 failed"
cat "$QA_ROOT/production-parity-evidence.json"
