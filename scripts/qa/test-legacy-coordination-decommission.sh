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

jq -e '
  .productionConsumers.capturesOrJsonPath == [] and
  .productionConsumers.celCoordination == [] and
  (.productionConsumers.pipelineVariables | length) == 2 and
  (.executionInventory.legacyCommandOnlyAgents | length) == 4 and
  .sourceTouches.capturesOrJsonPath <= 55
' "$INVENTORY" >/dev/null
pass "machine-readable inventory proves 0 capture/JSONPath, 0 coordination CEL, 2 generic variable, and 4 legacy Agent consumers"

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
