#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$REPO_ROOT/fixtures/driver/codex-cli-0.144.5-resume.json"
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in cargo jq rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 2
  }
done

cd "$REPO_ROOT"

if cargo test -p orchestrator-runner codex_resume --quiet; then
  pass "command construction and recorded protocol mapping tests"
else
  fail "runner Codex resume tests"
fi

if jq -e '
  .schema_version == 1
  and .provider == "codex"
  and .transport == "cli"
  and .codex_cli_version == "0.144.5"
  and .session_placeholder == "<SESSION_ID>"
  and (.first_events | length) == 4
  and (.resume_events | length) == 4
' "$FIXTURE" >/dev/null; then
  pass "fixture metadata and event counts are pinned"
else
  fail "fixture metadata or event counts"
fi

if rg -q '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' "$FIXTURE"; then
  fail "fixture contains an unsanitized UUID"
else
  pass "fixture contains no live session UUID"
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo "Codex session resume QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi

echo "Codex session resume QA: $PASS passed, 0 failed"
