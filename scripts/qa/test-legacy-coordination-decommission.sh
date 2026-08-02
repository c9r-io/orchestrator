#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
EVIDENCE_DIR="${FR125_EVIDENCE_DIR:-$(mktemp -d)}"
INVENTORY="$EVIDENCE_DIR/consumer-inventory.json"
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
if [[ "${FR125_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "FR-125 QA requires a clean worktree (or FR125_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

mkdir -p "$EVIDENCE_DIR"
ruby scripts/qa/coordination-governance.rb \
  --test-fixtures \
  --require-complete \
  --output "$INVENTORY" >/dev/null

# The counts here are exact, and every one of them has moved at least once.
# The legacy Agent count was 4 when FR-125 froze this ratchet; FR-126 drove it to
# 0 and FR-127 found the stale assertion while wiring this gate into CI, because
# until then nothing executed it. The generic-variable count was 2 until FR-156
# retired the manifest authoring surface and drove it to 0.
#
# `jq -e` under `set -e` was how both of those were reported: a bare non-zero
# exit, no output at all, and a log indistinguishable from a run that never
# started. The status is observed here instead, and the diagnostic names the
# expectation that no longer holds -- an exit code cannot say which conjunct
# failed, and this one has five.
# The sourceTouches ceiling fell 53 -> 23 when the coordinate stopped counting
# `output_json_path`, the session/step artifact path, which an unanchored
# `json_path` had been matching -- 32 of the 55 it reported. The ratchet is
# tighter, not looser: everything removed was outside what it claims to count.
INVENTORY_OK=0
jq -e '
  .productionConsumers.capturesOrJsonPath == [] and
  .productionConsumers.celCoordination == [] and
  .productionConsumers.pipelineVariables == [] and
  (.executionInventory.legacyCommandOnlyAgents | length) == 0 and
  .sourceTouches.capturesOrJsonPath <= 23
' "$INVENTORY" >/dev/null 2>&1 || INVENTORY_OK=$?
if [[ "$INVENTORY_OK" -ne 0 ]]; then
  echo "consumer inventory no longer matches the frozen expectation:" >&2
  jq -c '{
    capturesOrJsonPath: (.productionConsumers.capturesOrJsonPath | length),
    celCoordination: (.productionConsumers.celCoordination | length),
    pipelineVariables: (.productionConsumers.pipelineVariables | length),
    legacyCommandOnlyAgents: (.executionInventory.legacyCommandOnlyAgents | length),
    sourceTouchesCapturesOrJsonPath: .sourceTouches.capturesOrJsonPath
  }' "$INVENTORY" >&2
  echo "expected all consumer lists empty, 0 legacy Agents, and capturesOrJsonPath <= 23" >&2
  exit 1
fi
pass "machine-readable inventory proves 0 capture/JSONPath, 0 coordination CEL, 0 generic variable, and 0 legacy Agent consumers"

if rg -n \
  'apply_captures|pending_generate_items|extract_json_array' \
  crates/orchestrator-scheduler/src/scheduler/item_executor \
  crates/orchestrator-scheduler/src/scheduler/loop_engine \
  --glob '*.rs' --glob '!tests.rs'; then
  echo "production scheduler still contains a legacy extraction/consumption path" >&2
  exit 1
fi
pass "production scheduler contains no capture/JSONPath extraction or deferred consumption path"

cargo test -p orchestrator-config \
  legacy_preserved_keys_migrate_out_of_generic_vars >/dev/null
cargo test -p orchestrator-config \
  explicit_preserved_goal_wins_and_legacy_duplicate_is_removed >/dev/null
cargo test -p orchestrator-scheduler \
  load_task_runtime_context_normalizes_fields >/dev/null
cargo test -p agent-orchestrator \
  validate_workflow_config_rejects_json_path_on_exit_code_capture >/dev/null
pass "narrow carrier migration, persistence load, and stable manifest rejection tests pass"

./scripts/coverage-governance.sh --fixture-test >/dev/null
pass "boundary coverage governance negative fixtures pass"

FR125_ALLOW_DIRTY=1 "$SCRIPT_DIR/test-coordination-strangler.sh" \
  >"$EVIDENCE_DIR/coordination-strangler.log"
pass "all seven post-retirement tool workflows and survival boundaries pass"

if [[ "${FR125_FULL:-0}" == "1" ]]; then
  cargo test --workspace --exclude orchestrator-gui
  cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings
  pass "optional full workspace and strict Clippy gates pass"
fi

echo "FR-125 QA: $PASS passed, 0 failed"
echo "Evidence: $EVIDENCE_DIR"
