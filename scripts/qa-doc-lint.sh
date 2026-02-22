#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

status=0

echo "[qa-doc-lint] Checking banned patterns..."
BANNED_PATTERN='orchestrator/config/default\.yaml|cd orchestrator|--workspace-id|orchestrator agent health'
if rg -n "$BANNED_PATTERN" \
  docs/qa/orchestrator docs/design_doc docs/report \
  -g '!docs/qa/orchestrator/00-command-contract.md' >/tmp/qa_doc_lint_banned.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found banned patterns:"
  cat /tmp/qa_doc_lint_banned.txt
  status=1
fi

echo "[qa-doc-lint] Checking scenario count (<=5 per doc)..."
while IFS= read -r file; do
  count=$(rg -n '^## Scenario' "$file" | wc -l | tr -d ' ')
  if [[ "$count" -gt 5 ]]; then
    echo "[qa-doc-lint] Too many scenarios ($count): $file"
    status=1
  fi
done < <(rg --files docs/qa/orchestrator -g '*.md' | sort)

echo "[qa-doc-lint] Checking orchestrator QA index coverage..."
while IFS= read -r file; do
  if ! rg -q "$file" docs/qa/README.md; then
    echo "[qa-doc-lint] Missing from docs/qa/README.md index: $file"
    status=1
  fi
done < <(rg --files docs/qa/orchestrator -g '*.md' | sort)

if [[ "$status" -ne 0 ]]; then
  echo "[qa-doc-lint] FAILED"
  exit 1
fi

echo "[qa-doc-lint] PASS"
