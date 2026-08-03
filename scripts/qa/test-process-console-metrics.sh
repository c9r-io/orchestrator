#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
gate_runlog_arm "scripts/qa/test-process-console-metrics.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PASS=0

run() {
  echo "  RUN: $*"
  "$@"
  PASS=$((PASS + 1))
}

for command in cargo npm rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"

run cargo test -p agent-orchestrator process_metrics::tests --lib -- --nocapture
run cargo test -p orchestrator-config process_metrics_can_be_disabled_independently -- --nocapture
run cargo test -p orchestrator-integration-tests --test process_metrics -- --nocapture
run cargo test --release -p agent-orchestrator large_fixture_query_meets_process_metrics_budget --lib -- --ignored --nocapture
run cargo test --release -p orchestrator-scheduler large_timeline_meets_projection_budget --lib -- --ignored --nocapture

if rg -n 'console\.(info|log)|target_id|task_id|session_id|request_id' gui/src/lib/telemetry.ts; then
  echo "forbidden content or high-cardinality telemetry field found" >&2
  exit 1
fi
PASS=$((PASS + 1))

run cargo test -p agent-orchestrator qa_doctor::tests --lib -- --nocapture
run cargo test -p agent-orchestrator metrics::tests --lib -- --nocapture

run bash -c 'cd gui && npm test -- --run'
run bash -c 'cd gui && npm run build'
run bash -c 'cd gui && npm run test:e2e'

echo "Process Console metrics QA passed: $PASS gates"
