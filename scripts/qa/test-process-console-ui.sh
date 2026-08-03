#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
gate_runlog_arm "scripts/qa/test-process-console-ui.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

for command in npm cargo rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

echo "Process Console UI QA"

(
  cd "$REPO_ROOT/gui"
  npm run test:coverage
  npm run test:e2e
  npm run build
  npm audit
)

(
  cd "$REPO_ROOT"
  cargo test -p orchestrator-gui errors::tests
  rg -q 'page: "attention"' gui/src/App.tsx
  rg -q 'page: "processes"' gui/src/App.tsx
  rg -q 'page: "sessions"' gui/src/App.tsx
  rg -q 'page: "sources"' gui/src/App.tsx
  rg -q 'page: "system"' gui/src/App.tsx
  rg -q 'prefers-reduced-motion' gui/src/styles/tokens.css
  rg -q '@supports not.*backdrop-filter' gui/src/styles/tokens.css
  rg -q 'data-transparency="reduced"' gui/src/styles/tokens.css
  rg -q 'x-request-id|request-id' crates/gui/src/errors.rs
)

echo "Process Console UI QA: PASS"
