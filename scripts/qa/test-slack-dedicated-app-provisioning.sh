#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOG_ROOT="$(mktemp -d)"
STARTED_AT="$(date +%s)"
COMPLETED=()
FAILED=0

cleanup() {
  if [[ "$FAILED" == "1" || "${KEEP_FR115_QA:-0}" == "1" ]]; then
    echo "FR-115 QA logs retained at: $LOG_ROOT" >&2
  else
    rm -rf "$LOG_ROOT"
  fi
}
trap cleanup EXIT

for command in bash cargo git npm rg tee; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    FAILED=1
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR115_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "FR-115 dedicated Slack QA requires a clean worktree" >&2
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
    printf '  %s\n' "${COMPLETED[@]}" >&2
    return 1
  fi
}

run_gate gateway-dedicated-contract FR-115 \
  cargo test -p orchestrator-slack-gateway --all-targets
run_gate source-connection-checkpoints FR-115 \
  cargo test -p agent-orchestrator source_connection --lib
run_gate daemon-dedicated-control FR-115 \
  cargo test -p orchestratord source_connection
run_gate strict-dedicated-clippy FR-115 \
  cargo clippy -p orchestrator-slack-gateway -p agent-orchestrator -p orchestratord \
    -p orchestrator-cli -p orchestrator-gui --all-targets --all-features -- -D warnings
run_gate frontend-dedicated-unit FR-115 bash -c \
  'cd gui && npm test -- --run src/pages/source-connections/SourceConnections.test.tsx'
run_gate frontend-dedicated-e2e FR-115 bash -c \
  'cd gui && npm run test:e2e -- --grep "Slack connections"'
run_gate frontend-build FR-115 bash -c 'cd gui && npm run build'
run_gate fixture-contract FR-115 bash -c '
  rg -q "connectionRef: conn-managed-dedicated-fixture" fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml &&
  rg -q "reactionRouting: disabled" fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml &&
  ! rg -q "kind: SecretStore|signing_secret|client_secret|bot-token|xox[baprs]-" fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml
'
run_gate documentation-lint documentation ./scripts/qa-doc-lint.sh

if [[ "${SKIP_FR114_AGGREGATE:-0}" != "1" ]]; then
  if [[ "${FR115_ALLOW_DIRTY:-0}" == "1" ]]; then
    run_gate shared-oauth-regression FR-114 env FR114_ALLOW_DIRTY=1 SKIP_FR113_AGGREGATE=1 \
      ./scripts/qa/test-slack-managed-shared-oauth.sh
  else
    run_gate shared-oauth-regression FR-114 env SKIP_FR113_AGGREGATE=1 \
      ./scripts/qa/test-slack-managed-shared-oauth.sh
  fi
fi

if [[ "${SKIP_FR113_AGGREGATE:-0}" != "1" ]]; then
  if [[ "${FR115_ALLOW_DIRTY:-0}" == "1" ]]; then
    run_gate badge-runtime-regression FR-113 env FR113_ALLOW_DIRTY=1 \
      ./scripts/qa/test-slack-skill-automation-release.sh
  else
    run_gate badge-runtime-regression FR-113 \
      ./scripts/qa/test-slack-skill-automation-release.sh
  fi
fi

for forbidden in \
  xoxe-fr115-configuration-token \
  fr115-client-secret \
  fr115-signing-secret \
  xoxb-fr115-installation-token \
  fr115-oauth-code \
  fr115-oauth-state \
  private-fr115-workspace.slack.com; do
  if rg -F "$forbidden" "$LOG_ROOT" >/dev/null 2>&1; then
    FAILED=1
    echo "FR-115 diagnostic logs contain forbidden provider data" >&2
    exit 1
  fi
done
COMPLETED+=("PASS diagnostic-privacy owner=FR-115 duration=0s")

TOTAL_ELAPSED=$(( $(date +%s) - STARTED_AT ))
echo ""
echo "Dedicated Slack App provisioning QA: ${#COMPLETED[@]} gates passed in ${TOTAL_ELAPSED}s"
printf '  %s\n' "${COMPLETED[@]}"
