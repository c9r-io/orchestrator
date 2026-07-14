#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LOG_ROOT="$(mktemp -d)"
STARTED_AT="$(date +%s)"
COMPLETED=()

cleanup() {
  if [[ "${KEEP_RELEASE_QA:-0}" == "1" ]]; then
    echo "Release QA logs retained at: $LOG_ROOT" >&2
  else
    rm -rf "$LOG_ROOT"
  fi
}
trap cleanup EXIT

for command in bash cargo git jq mktemp npm rg sqlite3 tee; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Process Console release QA requires a clean worktree" >&2
  git status --short >&2
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
    COMPLETED+=("FAIL $name owner=$owner duration=${elapsed}s")
    echo "<== [$name] FAIL owner=$owner duration=${elapsed}s command=$*" >&2
    printf '%s\n' "${COMPLETED[@]}" >&2
    return 1
  fi
}

run_gate fresh-rust-build FR-106 \
  cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui
run_gate fresh-web-build FR-106 bash -c 'cd gui && npm run build'
run_gate workspace-tests repository cargo test --workspace
run_gate strict-clippy repository cargo clippy --workspace --all-targets -- -D warnings
run_gate qa-doc-lint documentation ./scripts/qa-doc-lint.sh

run_gate timeline FR-095 ./scripts/qa/test-process-timeline.sh
run_gate attention FR-096 ./scripts/qa/test-attention-inbox.sh
run_gate handoff-resume FR-097 ./scripts/qa/test-handoff-safe-resume.sh
run_gate session FR-098,FR-102,FR-105 env SKIP_BUILD=1 ./scripts/qa/test-agent-session-control-plane.sh
run_gate source-slack FR-099 ./scripts/qa/test-source-events-slack.sh
run_gate action-audit FR-101 ./scripts/qa/test-control-plane-action-audit.sh
run_gate console-ui FR-100 ./scripts/qa/test-process-console-ui.sh
run_gate vertical-flow FR-103 ./scripts/qa/test-process-console-vertical-flow.sh
run_gate process-metrics FR-104 ./scripts/qa/test-process-console-metrics.sh

TOTAL_ELAPSED=$(( $(date +%s) - STARTED_AT ))
echo ""
echo "Process Console v1 release QA: ${#COMPLETED[@]} gates passed in ${TOTAL_ELAPSED}s"
printf '  %s\n' "${COMPLETED[@]}"
