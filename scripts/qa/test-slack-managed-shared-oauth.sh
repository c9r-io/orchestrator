#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOG_ROOT="$(mktemp -d)"
STARTED_AT="$(date +%s)"
COMPLETED=()
FAILED=0

cleanup() {
  # gate_scratch_has_evidence: $LOG_ROOT is allocated above the command preamble
  # and above the clean-worktree check, so an exit at either sets FAILED=1 over a
  # directory nothing has written to. Announcing that reads exactly like a real
  # failure with logs in it.
  if [[ "$FAILED" == "1" || "${KEEP_FR114_QA:-0}" == "1" ]] && gate_scratch_has_evidence "$LOG_ROOT"; then
    echo "FR-114 QA logs retained at: $LOG_ROOT" >&2
  else
    rm -rf "$LOG_ROOT"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-slack-managed-shared-oauth.sh"

for command in bash cargo git jq mktemp npm rg tee; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    FAILED=1
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR114_ALLOW_DIRTY:-0}" != "1" && -n "$(gate_runlog_worktree_status "$REPO_ROOT")" ]]; then
  echo "FR-114 managed Slack QA requires a clean worktree" >&2
  git status --short >&2
  FAILED=1
  exit 1
fi

run_gate() {
  local name="$1"
  local owner="$2"
  shift 2
  local started elapsed log_file
  started="$(date +%s)"
  log_file="$LOG_ROOT/${name}.log"
  echo ""
  echo "==> [$name] owner=$owner"
  echo "    command: $*"
  if "$@" 2>&1 | tee "$log_file"; then
    elapsed=$(( $(date +%s) - started ))
    COMPLETED+=("PASS $name owner=$owner duration=${elapsed}s")
  else
    elapsed=$(( $(date +%s) - started ))
    COMPLETED+=("FAIL $name owner=$owner duration=${elapsed}s log=$log_file")
    FAILED=1
    printf '  %s\n' ${COMPLETED[@]+"${COMPLETED[@]}"} >&2
    return 1
  fi
}

run_gate gateway-contract FR-114 \
  cargo test -p orchestrator-slack-gateway --all-targets
run_gate source-connection-state FR-114 \
  cargo test -p agent-orchestrator source_connection --lib
run_gate daemon-managed-source FR-114 \
  cargo test -p orchestratord managed_source
run_gate daemon-source-connection FR-114 \
  cargo test -p orchestratord source_connection
run_gate strict-managed-clippy FR-114 \
  cargo clippy -p orchestrator-slack-gateway -p agent-orchestrator -p orchestratord \
    --all-targets --all-features -- -D warnings
run_gate frontend-connections-unit FR-114 bash -c \
  'cd gui && npm test -- --run src/pages/source-connections/SourceConnections.test.tsx'
run_gate frontend-connections-e2e FR-114 bash -c \
  'cd gui && npm run test:e2e -- --grep "Slack connections"'
run_gate frontend-build FR-114 bash -c 'cd gui && npm run build'
run_gate fixture-contract FR-114 bash -c '
  rg -q "connectionRef: conn-managed-shared-fixture" fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml &&
  rg -q "reactionRouting: disabled" fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml &&
  ! rg -q "kind: SecretStore|signing|bot-token|xox[baprs]-" fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml &&
  bash -n scripts/qa/test-slack-managed-live-smoke.sh scripts/qa/certify-slack-managed-live.sh &&
  ! rg -q "xox[baprs]-[A-Za-z0-9-]+" config/qa/slack-live.env.example scripts/qa/test-slack-managed-live-smoke.sh scripts/qa/certify-slack-managed-live.sh
'
run_gate documentation-lint documentation ./scripts/qa-doc-lint.sh

if [[ "${SKIP_FR113_AGGREGATE:-0}" != "1" ]]; then
  if [[ "${FR114_ALLOW_DIRTY:-0}" == "1" ]]; then
    run_gate manual-slack-regression FR-113 env FR113_ALLOW_DIRTY=1 \
      ./scripts/qa/test-slack-skill-automation-release.sh
  else
    run_gate manual-slack-regression FR-113 \
      ./scripts/qa/test-slack-skill-automation-release.sh
  fi
fi

for forbidden in \
  xoxb-fr114-fixture-secret \
  fr114-signing-secret \
  fr114-oauth-code \
  fr114-oauth-state \
  private-fr114-workspace.slack.com; do
  if rg -F "$forbidden" "$LOG_ROOT" >/dev/null 2>&1; then
    FAILED=1
    echo "FR-114 diagnostic logs contain forbidden provider data" >&2
    exit 1
  fi
done
COMPLETED+=("PASS diagnostic-privacy owner=FR-114 duration=0s")

TOTAL_ELAPSED=$(( $(date +%s) - STARTED_AT ))
echo ""
echo "Managed Slack shared OAuth QA: ${#COMPLETED[@]} gates passed in ${TOTAL_ELAPSED}s"
printf '  %s\n' ${COMPLETED[@]+"${COMPLETED[@]}"}
