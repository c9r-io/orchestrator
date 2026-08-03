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
  if [[ "$FAILED" == "1" || "${KEEP_RELEASE_QA:-0}" == "1" ]]; then
    echo "FR-113 release logs retained at: $LOG_ROOT" >&2
  else
    rm -rf "$LOG_ROOT"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-slack-skill-automation-release.sh"

for command in bash cargo curl git jq mktemp npm openssl python3 rg sqlite3 tee; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    FAILED=1
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR113_ALLOW_DIRTY:-0}" != "1" && -n "$(gate_runlog_worktree_status "$REPO_ROOT")" ]]; then
  echo "Slack Skill automation release QA requires a clean worktree" >&2
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
    echo "<== [$name] PASS duration=${elapsed}s"
  else
    elapsed=$(( $(date +%s) - started ))
    COMPLETED+=("FAIL $name owner=$owner duration=${elapsed}s log=$log_file")
    FAILED=1
    echo "<== [$name] FAIL owner=$owner duration=${elapsed}s log=$log_file" >&2
    printf '  %s\n' ${COMPLETED[@]+"${COMPLETED[@]}"} >&2
    return 1
  fi
}

run_gate fresh-rust-build FR-113 \
  cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui
run_gate fresh-web-build FR-113 bash -c 'cd gui && npm run build'
run_gate workspace-tests repository \
  cargo test --workspace --all-targets --all-features
run_gate strict-clippy repository \
  cargo clippy --workspace --all-targets --all-features -- -D warnings
run_gate frontend-unit-coverage FR-112 bash -c 'cd gui && npm run test:coverage'
run_gate frontend-playwright FR-112 bash -c 'cd gui && npm run test:e2e'
run_gate qa-doc-lint documentation ./scripts/qa-doc-lint.sh

run_gate reaction-contract FR-107 ./scripts/qa/test-slack-reaction-source.sh
run_gate source-task-template FR-108 ./scripts/qa/test-source-task-template.sh
run_gate source-task-binding FR-109 ./scripts/qa/test-source-task-binding.sh
run_gate canonical-task-routing FR-110 ./scripts/qa/test-slack-reaction-task-routing.sh
run_gate automation-operations FR-111 env SKIP_DEPENDENCY_GATES=1 \
  ./scripts/qa/test-source-automation-operations.sh
run_gate automation-console FR-112 env SKIP_BUILD=1 SKIP_FRONTEND=1 SKIP_DEPENDENCY_GATES=1 \
  ./scripts/qa/test-source-automation-ui.sh
run_gate release-vertical FR-113 ./scripts/qa/test-slack-skill-automation-vertical.sh
run_gate guide-contract FR-113 bash -c '
  target/debug/orchestrator guide "source automation" --format json >/dev/null &&
  target/debug/orchestrator guide "source template" --format json >/dev/null &&
  target/debug/orchestrator guide "source binding" --format json >/dev/null &&
  for term in setup preview enable inspect diagnose suspend upgrade rollback; do
    rg -qi "$term" docs/guide/slack-reaction-skill-automation.md || exit 1
  done
'

for forbidden in \
  qa-slack-release-signing-secret \
  qa-slack-release-valid-token \
  qa-slack-release-invalid-token \
  qa-source-routing-signing-secret \
  qa-source-routing-fake-token \
  qa-release-workspace.slack.com; do
  if rg -F "$forbidden" "$LOG_ROOT" >/dev/null 2>&1; then
    FAILED=1
    echo "release diagnostic logs contain forbidden private fixture data" >&2
    exit 1
  fi
done
COMPLETED+=("PASS diagnostic-privacy owner=FR-113 duration=0s")

TOTAL_ELAPSED=$(( $(date +%s) - STARTED_AT ))
echo ""
echo "Slack Reaction Skill Automation release QA: ${#COMPLETED[@]} gates passed in ${TOTAL_ELAPSED}s"
printf '  %s\n' ${COMPLETED[@]+"${COMPLETED[@]}"}
