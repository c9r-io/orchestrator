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
  # --omit=dev, deliberately. This is the only npm audit in the repository, and
  # it gates a release: `build` and `gui-build` reach this gate through
  # release.yml's manual-gate-freshness job. Unscoped, it fails on advisories in
  # devDependencies — measured at FR-165, three in undici reached only as
  # jsdom -> undici, a test-time dependency that is not in the bundle. Those
  # recur on upstream's schedule, which has nothing to do with whether this
  # release is safe to cut, and a gate that is red for reasons unrelated to its
  # subject is one people learn to route around.
  #
  # What ships is what this now asks about: `npm audit --omit=dev` reports 0
  # today. Dev-tree advisories are not thereby unowned — .github/dependabot.yml
  # covers /gui, so jsdom bumps arrive as PRs.
  npm audit --omit=dev
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
