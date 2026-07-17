#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PASS=0

run() {
  echo "  RUN: $*"
  "$@"
  PASS=$((PASS + 1))
}

for command in cargo rg bash; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"

run cargo test -p agent-orchestrator source_automation::tests --lib
run cargo test -p agent-orchestrator source_automation_attention_deduplicates_resolves_and_reopens --lib
run cargo test -p agent-orchestrator source_automation_metrics_are_authoritative_and_privacy_safe --lib
run cargo test -p agent-orchestrator migration --lib
run cargo test -p orchestratord source_router::tests --bin orchestratord
run cargo test -p orchestratord slack_api::tests --bin orchestratord
run cargo test -p orchestrator-cli source_automation_ --bin orchestrator

run bash "$SCRIPT_DIR/test-slack-reaction-task-routing.sh"

if rg -n 'permalink: include_permalink|include_permalink\.then_some' \
    crates/daemon/src/server/source.rs >/dev/null &&
  rg -n '"SourceAutomation(List|Get|Watch|Simulate|StatusGet)"' \
    crates/daemon/src/control_plane.rs >/dev/null; then
  PASS=$((PASS + 1))
else
  echo "safe projection or read-only control-plane contract missing" >&2
  exit 1
fi

if rg -n 'installation_id|message_identity|channel_id|message_ts|lease_token|credential' \
    core/src/process_metrics.rs | rg 'labels\(' >/dev/null; then
  echo "high-cardinality source automation metric label found" >&2
  exit 1
fi
PASS=$((PASS + 1))

echo "Source automation operations QA passed: $PASS gates"
