#!/usr/bin/env bash
set -euo pipefail

# Check ripgrep dependency
if ! command -v rg &>/dev/null; then
  echo "[qa-doc-lint] ERROR: ripgrep (rg) is not installed." >&2
  echo "[qa-doc-lint] Install with: brew install ripgrep" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

fail=0

# Existing orchestrator-specific guardrails.
echo "[qa-doc-lint] Checking banned patterns..."
BANNED_PATTERN='orchestrator/config/default\.yaml|cd orchestrator|--workspace-id|orchestrator agent health|config bootstrap --from|--config <file>|--config <path>|localhost:1423/api|/api/task-options|scripts/orchestrator\.sh'
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
# Also check skill files for actionable (non-warning) db-delete commands.
# Match lines that contain `rm -f data/agent_orchestrator` as a bash command (starts with rm),
# but skip lines that are comments/warnings (start with # or contain NEVER/CRITICAL/DO NOT).
if rg -n "rm -f data/agent_orchestrator" .claude/skills -g '*.md' 2>/dev/null \
   | rg -v '^\s*#|NEVER|CRITICAL|Do NOT|DO NOT' >/tmp/qa_doc_lint_skill_reset.txt 2>/dev/null; then
  if [[ -s /tmp/qa_doc_lint_skill_reset.txt ]]; then
    echo "[qa-doc-lint] Found actionable db-delete in skill files (use project reset flow):"
    cat /tmp/qa_doc_lint_skill_reset.txt
    fail=1
  fi
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
    | rg -v -e "task create --help" -e "task create --format" -e "does not depend on" -e "task create supports" -e "task create does NOT" -e "task create --no-start is defined" -e "^\S+:\d+:\|" -e "\`task create\` supports" -e "\`task create\` does NOT" -e "\`task create --no-start\` creates" -e "\`task create --no-start\` is defined" || true
)

echo "[qa-doc-lint] Checking workflow ID cross-reference against fixtures..."
# Extract --workflow <id> from orchestrator QA docs and verify each ID exists in fixture YAMLs.
# Scoped to docs/qa/orchestrator/*.md only — scripts and self-bootstrap docs often embed inline
# workflow definitions that aren't in fixture bundles.
fixture_workflows=$(rg -A3 'kind: Workflow' fixtures/manifests/bundles/*.yaml 2>/dev/null \
  | rg 'name:' | sed 's/.*name: //' | sort -u)
while IFS=: read -r file line match; do
  wf_id=$(printf '%s' "$match" | rg -o '\-\-workflow\s+(\S+)' -r '$1')
  # Skip placeholders (<...>), shell variables ($...), and quoted vars ("$...")
  [[ -z "$wf_id" || "$wf_id" == *'<'* || "$wf_id" == *'$'* || "$wf_id" == *'"'* ]] && continue
  if ! printf '%s\n' "$fixture_workflows" | rg -qx "$wf_id"; then
    echo "[qa-doc-lint] Unknown workflow ID '$wf_id' at ${file}:${line} (not in any fixture)"
    fail=1
  fi
done < <(rg -n -- '--workflow\s+\S+' docs/qa/orchestrator -g '*.md' 2>/dev/null || true)

echo "[qa-doc-lint] Checking edit subcommand structure..."
# Bare 'edit <resource>' without 'export' or 'open' subcommand is invalid.
if rg -n 'orchestrator\s+edit\s+(?!export|open|--|-f|<)\S+' docs/qa -g '*.md' --pcre2 \
    >/tmp/qa_doc_lint_edit.txt 2>/dev/null; then
  echo "[qa-doc-lint] Found bare 'edit <resource>' (must use 'edit export' or 'edit open'):"
  cat /tmp/qa_doc_lint_edit.txt
  fail=1
fi

echo "[qa-doc-lint] Checking scenario count (<=5) for orchestrator docs..."
while IFS= read -r file; do
  count=$( (rg -n '^## Scenario' "$file" || true) | wc -l | tr -d ' ')
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
qa_docs=$(printf "%s\n" "$all_docs" | grep -v 'docs/qa/README.md' | grep -v 'docs/qa/_' | grep -v 'docs/qa/script/README.md' || true)
all_without_readme=$(printf "%s\n" "$qa_docs" | sed 's#^docs/qa/##' | sort)
indexed=$(rg -o '`docs/qa/[^`]+\.md`' docs/qa/README.md | sed -E 's#^`docs/qa/##; s#`$##' | grep -v '^_' | sort -u || true)

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
# Note: Use \bTab\b to avoid matching "Table" in markdown headers
ui_docs=$(rg -l "Portal UI|sidebar|navigation|\bTab\b|Quick Links|button|侧边栏|导航" docs/qa --glob '*.md' || true)
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

echo "[qa-doc-lint] Checking site guide files are not tracked (should be gitignored)..."
if git ls-files --error-unmatch site/en/guide/ site/zh/guide/ 2>/dev/null | head -1 >/dev/null 2>&1; then
  echo "[qa-doc-lint] ERROR: site guide files should be gitignored (generated by scripts/sync-docs.mjs)"
  fail=1
fi

if (( fail > 0 )); then
  echo "[qa-doc-lint] FAILED"
  exit 1
fi

echo "[qa-doc-lint] PASS"
