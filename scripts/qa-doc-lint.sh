#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

fail=0

# Existing orchestrator-specific guardrails.
echo "[qa-doc-lint] Checking banned patterns..."
BANNED_PATTERN='orchestrator/config/default\.yaml|cd orchestrator|--workspace-id|orchestrator agent health|config bootstrap --from|--config <file>|--config <path>|localhost:1423/api|/api/task-options'
if rg -n "$BANNED_PATTERN" \
  docs/qa/orchestrator docs/design_doc docs/report \
  -g '!docs/qa/orchestrator/00-command-contract.md' >/tmp/qa_doc_lint_banned.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found banned patterns:"
  cat /tmp/qa_doc_lint_banned.txt
  fail=1
fi

echo "[qa-doc-lint] Checking legacy sqlite/global-fixture reset patterns..."
LEGACY_RESET_PATTERN="rm -f data/agent_orchestrator\\.db|find fixtures/ticket -name '\\*\\.md' ! -name 'README\\.md' -delete"
if rg -n "$LEGACY_RESET_PATTERN" docs/qa -g '*.md' >/tmp/qa_doc_lint_legacy_reset.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found legacy reset patterns (use project reset flow):"
  cat /tmp/qa_doc_lint_legacy_reset.txt
  fail=1
fi

echo "[qa-doc-lint] Checking task create commands require --project..."
while IFS=: read -r file line _; do
  snippet="$(sed -n "${line},$((line + 4))p" "$file")"
  if ! printf '%s\n' "$snippet" | rg -q -- '--project'; then
    echo "[qa-doc-lint] task create missing --project near ${file}:${line}"
    fail=1
  fi
done < <(
  rg -n "task create" docs/qa -g '*.md' \
    | rg -v "task create --help|task create --format|does not depend on" || true
)

echo "[qa-doc-lint] Checking scenario count (<=5) for orchestrator docs..."
while IFS= read -r file; do
  count=$(rg -n '^## Scenario' "$file" | wc -l | tr -d ' ')
  if [[ "$count" -gt 5 ]]; then
    echo "[qa-doc-lint] Too many scenarios ($count): $file"
    fail=1
  fi
done < <(rg --files docs/qa/orchestrator -g '*.md' 2>/dev/null | sort || true)

echo "[qa-doc-lint] Checking orchestrator QA index coverage..."
while IFS= read -r file; do
  if ! rg -q "$file" docs/qa/README.md; then
    echo "[qa-doc-lint] Missing from docs/qa/README.md index: $file"
    fail=1
  fi
done < <(rg --files docs/qa/orchestrator -g '*.md' 2>/dev/null | sort || true)

# Governance checks (project-wide).
all_docs=$(find docs/qa -name '*.md' | sort)
qa_docs=$(printf "%s\n" "$all_docs" | grep -v 'docs/qa/README.md' | grep -v 'docs/qa/_' || true)
all_without_readme=$(printf "%s\n" "$qa_docs" | sed 's#^docs/qa/##' | sort)
indexed=$(rg -o "\(\./[^)]+\.md\)" docs/qa/README.md | sed -E 's#^\(\./##; s#\)$##' | grep -v '^_' | sort -u || true)

echo "[qa-doc-lint] Checking README index drift..."
not_indexed=$(comm -23 <(printf "%s\n" "$all_without_readme") <(printf "%s\n" "$indexed") || true)
indexed_missing=$(comm -13 <(printf "%s\n" "$all_without_readme") <(printf "%s\n" "$indexed") || true)

if [[ -n "$not_indexed" ]]; then
  echo "[qa-doc-lint] Missing in README:"
  printf '%s\n' "$not_indexed"
  fail=1
fi

if [[ -n "$indexed_missing" ]]; then
  echo "[qa-doc-lint] Indexed but missing in filesystem:"
  printf '%s\n' "$indexed_missing"
  fail=1
fi

echo "[qa-doc-lint] Checking checklist sections..."
missing_checklist=0
while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  if ! rg -q "## Checklist|## Regression Checklist|## 检查清单|## 回归测试检查清单" "$f"; then
    echo "[qa-doc-lint] Missing checklist section: ${f#docs/qa/}"
    missing_checklist=1
  fi
done < <(printf "%s\n" "$qa_docs")
if (( missing_checklist > 0 )); then
  fail=1
fi

echo "[qa-doc-lint] Checking UI entry visibility hints (warning-only)..."
ui_docs=$(rg -l "Portal UI|sidebar|navigation|Tab|Quick Links|button|侧边栏|导航" docs/qa --glob '*.md' || true)
while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  if ! rg -q "Entry Visibility|入口可见性" "$f"; then
    echo "[qa-doc-lint][WARN] UI doc lacks explicit entry visibility scenario: ${f#docs/qa/}"
  fi
done < <(printf "%s\n" "$ui_docs")

echo "[qa-doc-lint] Checking auth-session executable guidance (warning-only)..."
auth_docs=$(rg -l -F "close browser" docs/qa --glob '*.md' || true)
while IFS= read -r f; do
  [[ -z "$f" ]] && continue
  if ! rg -q "incognito|private window|auth9_session|sign out|persistent session" "$f"; then
    echo "[qa-doc-lint][WARN] Auth/session negative check may be non-executable: ${f#docs/qa/}"
  fi
done < <(printf "%s\n" "$auth_docs")

if (( fail > 0 )); then
  echo "[qa-doc-lint] FAILED"
  exit 1
fi

echo "[qa-doc-lint] PASS"
