#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

status=0

echo "[qa-doc-lint] Checking banned patterns..."
BANNED_PATTERN='orchestrator/config/default\.yaml|cd orchestrator|--workspace-id|orchestrator agent health|config bootstrap --from|--config <file>|--config <path>'
if rg -n "$BANNED_PATTERN" \
  docs/qa/orchestrator docs/design_doc docs/report \
  -g '!docs/qa/orchestrator/00-command-contract.md' >/tmp/qa_doc_lint_banned.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found banned patterns:"
  cat /tmp/qa_doc_lint_banned.txt
  status=1
fi

echo "[qa-doc-lint] Checking legacy sqlite/global-fixture reset patterns..."
LEGACY_RESET_PATTERN="rm -f data/agent_orchestrator\\.db|find fixtures/ticket -name '\\*\\.md' ! -name 'README\\.md' -delete"
if rg -n "$LEGACY_RESET_PATTERN" docs/qa -g '*.md' >/tmp/qa_doc_lint_legacy_reset.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found legacy reset patterns (use project reset flow):"
  cat /tmp/qa_doc_lint_legacy_reset.txt
  status=1
fi

echo "[qa-doc-lint] Checking task create commands require --project..."
while IFS=: read -r file line _; do
  snippet="$(sed -n "${line},$((line + 4))p" "$file")"
  if ! printf '%s\n' "$snippet" | rg -q -- '--project'; then
    echo "[qa-doc-lint] task create missing --project near ${file}:${line}"
    status=1
  fi
done < <(
  rg -n "task create" docs/qa -g '*.md' \
    | rg -v "task create --help|task create --format|does not depend on"
)

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
