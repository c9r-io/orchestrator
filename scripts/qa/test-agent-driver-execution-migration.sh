#!/usr/bin/env bash

set -euo pipefail

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
if [[ "${FR126_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
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
  .executionInventory.legacyCommandOnlyAgents == [] and
  .executionInventory.driverCounts == {
    "shell/cli": 3,
    "claude/cli": 17,
    "codex/cli": 0
  } and
  .executionInventory.globalStreamingExecutors == [] and
  .sourceTouches.legacyRunnerSelection == 0
' "$INVENTORY" >/dev/null
pass "production inventory has 0 command-only Agents, 0 global streaming executors, and 0 legacy runner selection symbols"

if rg -n \
  'RunnerExecutorKind|ShellRunnerExecutor|StreamingAgentRunner|spawn_with_runner(_and_capture)?_session|prepare_legacy_claude_streaming_command' \
  core/src crates/*/src --glob '*.rs'; then
  echo "legacy runner selection remains in production Rust source" >&2
  exit 1
fi
pass "legacy runner types and provider-session compatibility bridge are absent from production source"

cargo test -p orchestrator-config \
  shell_cli_factory_is_explicit_and_safe_by_default >/dev/null
cargo test -p orchestrator-runner \
  shell_driver_delivers_stdin_payload_and_closes_stdin >/dev/null
cargo test -p orchestrator-runner \
  command_rules_are_only_supported_by_shell_driver >/dev/null
cargo test -p agent-orchestrator \
  apply_legacy_command_agent_warns_and_persists_shell_driver >/dev/null
cargo test -p agent-orchestrator \
  validate_rejects_removed_streaming_executor >/dev/null
cargo test -p orchestrator-scheduler \
  tty_is_only_supported_by_typed_shell_cli_driver >/dev/null
cargo test -p orchestrator-scheduler \
  failed_driver_terminal_is_a_hard_validation_failure >/dev/null
cargo test -p orchestrator-integration-tests --test workflow_loop \
  workflow_failing_step >/dev/null
cargo test -p orchestrator-scheduler \
  execute_cycle_graph_persists_replay_and_skips_prehook_false_nodes >/dev/null
pass "promotion, stdin, command-rules, streaming rejection, TTY, failure propagation, and engine-command boundaries pass"

FR116_ALLOW_DIRTY=1 KEEP_FR116_QA="${KEEP_FR126_QA:-0}" \
  "$SCRIPT_DIR/test-agent-driver-abstraction.sh" \
  > "$EVIDENCE_DIR/agent-driver-isolated.log"
pass "isolated daemon proves promoted and explicit shell Agents converge through typed drivers"

if [[ "${FR126_FULL:-0}" == "1" ]]; then
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  pass "optional full workspace and strict Clippy gates pass"
fi

echo "FR-126 QA: $PASS passed, 0 failed"
echo "Evidence: $EVIDENCE_DIR"
