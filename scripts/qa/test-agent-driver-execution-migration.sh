#!/usr/bin/env bash

set -euo pipefail

# FR-158: the freshness ledger is written by the manual-runbook gates, and two
# of them are invoked from ci-required gates. This gate refuses to run on a dirty
# worktree, so it reads the tree through the shared predicate that excludes that
# one file — otherwise it fails for the recorder's reason rather than its own.
. "20 20 12 61 79 80 81 33 98 100 204 250 395 398 399 400 701git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
EVIDENCE_DIR="${FR126_EVIDENCE_DIR:-$(mktemp -d)}"
INVENTORY="$EVIDENCE_DIR/execution-inventory.json"
PASS=0

pass() {
  echo "  PASS: $1"
  PASS=$((PASS + 1))
}

for command in cargo git jq rg ruby; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR126_ALLOW_DIRTY:-0}" != "1" && -n "$(gate_runlog_worktree_status "$(git rev-parse --show-toplevel)")" ]]; then
  echo "FR-126 QA requires a clean worktree (or FR126_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

mkdir -p "$EVIDENCE_DIR"
ruby scripts/qa/coordination-governance.rb \
  --test-fixtures \
  --require-complete \
  --output "$INVENTORY" >/dev/null

jq -e '
  (.executionInventory.agents | length) == 20 and
  ([.executionInventory.agents[] | select(.classification == "shell-script")] | length) == 3 and
  ([.executionInventory.agents[] | select(.classification == "ai-provider")] | length) == 17 and
  all(.executionInventory.agents[];
    (.workflows | length) > 0 and
    (.migrationTarget | type) == "string" and
    (.manifestFingerprint | test("^[0-9a-f]{64}$"))
  ) and
  .executionInventory.legacyCommandOnlyAgents == [] and
  .executionInventory.driverCounts == {
    "shell/cli": 3,
    "claude/cli": 17,
    "codex/cli": 0
  } and
  .executionInventory.globalStreamingExecutors == [] and
  .sourceTouches.legacyRunnerSelection == 0
' "$INVENTORY" >/dev/null
pass "production inventory has 20 individually fingerprinted typed Agents and zero legacy consumers"

if rg -n \
  'RunnerExecutorKind|ShellRunnerExecutor|StreamingAgentRunner|spawn_with_runner(_and_capture)?_session|prepare_legacy_claude_streaming_command' \
  core/src crates/*/src --glob '*.rs'; then
  echo "legacy runner selection remains in production Rust source" >&2
  exit 1
fi
pass "legacy runner types and provider-session compatibility bridge are absent from production source"

"$SCRIPT_DIR/test-agent-driver-documentation-alignment.sh" --fixture-test \
  > "$EVIDENCE_DIR/documentation-alignment.log"
pass "EN/ZH guides, architecture, authoring skill, design records, and governance layers align with typed drivers"

# One invocation per package, several filters each — after the `--`, because
# cargo itself accepts a single TESTNAME and it is libtest that ORs a filter
# list. The same nine tests run and the same single exit status certifies
# them; the one-filter-per-invocation form paid a cargo start-up and
# fingerprint pass per test in a step whose cost is recorded against the
# FR-140 budget.
cargo test -p orchestrator-config \
  shell_cli_factory_is_explicit_and_safe_by_default >/dev/null
cargo test -p orchestrator-runner -- \
  shell_driver_delivers_stdin_payload_and_closes_stdin \
  command_rules_are_only_supported_by_shell_driver >/dev/null
cargo test -p agent-orchestrator -- \
  apply_legacy_command_agent_warns_and_persists_shell_driver \
  validate_rejects_removed_streaming_executor >/dev/null
cargo test -p orchestrator-scheduler -- \
  tty_is_only_supported_by_typed_shell_cli_driver \
  failed_driver_terminal_is_a_hard_validation_failure \
  execute_cycle_graph_persists_replay_and_skips_prehook_false_nodes >/dev/null
cargo test -p orchestrator-integration-tests --test workflow_loop \
  workflow_failing_step >/dev/null
pass "promotion, stdin, command-rules, streaming rejection, TTY, failure propagation, and engine-command boundaries pass"

FR116_ALLOW_DIRTY=1 KEEP_FR116_QA="${KEEP_FR126_QA:-0}" \
  "$SCRIPT_DIR/test-agent-driver-abstraction.sh" \
  > "$EVIDENCE_DIR/agent-driver-isolated.log"
pass "isolated daemon proves promoted and explicit shell Agents converge through typed drivers"

# In FAST mode the parity run is deferred, not skipped: ci.yml's governance job
# runs ./scripts/qa/test-agent-driver-production-parity.sh as its own step
# (id: parity) in the same job, so the certifying aggregate still executes it —
# the same shape as the repository-wide gates below. A full local run still
# pays for it here and keeps its log in the evidence bundle.
if [[ "${FR126_FAST:-0}" != "1" ]]; then
  FR126_ALLOW_DIRTY="${FR126_ALLOW_DIRTY:-0}" \
    KEEP_FR126_QA="${KEEP_FR126_QA:-0}" \
    "$SCRIPT_DIR/test-agent-driver-production-parity.sh" \
    > "$EVIDENCE_DIR/production-parity.log"
  pass "all four production migration contracts pass offline parity and rollback checks"
else
  pass "offline parity and rollback checks explicitly deferred to the sibling step that runs them"
fi

if [[ "${FR126_FAST:-0}" != "1" ]]; then
  cargo fmt --all -- --check
  FR125_ALLOW_DIRTY="${FR126_ALLOW_DIRTY:-0}" \
    "$SCRIPT_DIR/test-coordination-strangler.sh" \
    > "$EVIDENCE_DIR/coordination-strangler.log"
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  ./scripts/coverage-governance.sh --fixture-test
  ./scripts/qa-doc-lint.sh
  pass "mandatory format, strangler, workspace, strict Clippy, coverage, and QA documentation gates pass"
else
  pass "fast iteration mode explicitly skipped release-only repository gates"
fi

echo "FR-126 QA: $PASS passed, 0 failed"
echo "Evidence: $EVIDENCE_DIR"
