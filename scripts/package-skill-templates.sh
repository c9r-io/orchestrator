#!/usr/bin/env bash
#
# package-skill-templates.sh — Package skill templates for distribution.
#
# Copies templateable skills from .claude/skills/ to skill-templates/,
# sanitizing project-specific references. The output is included in
# releases and installed to ~/.orchestratord/skill-templates/.

set -euo pipefail

SRC=".claude/skills"
DST="skill-templates"

echo "Packaging skill templates..."

mkdir -p "$DST/generic" "$DST/framework" "$DST/sdlc-patterns"

# ── Generic skills (no sanitization needed) ──────────────────────────────────
for skill in performance-testing project-bootstrap; do
  if [[ -d "$SRC/$skill" ]]; then
    rm -rf "$DST/generic/$skill"
    cp -r "$SRC/$skill" "$DST/generic/$skill"
    echo "  generic/$skill"
  fi
done

# ── Framework-specific skills (sanitize project paths) ───────────────────────
for skill in align-tests deploy-gh-k8s e2e-testing grpc-regression ops \
             project-readiness reset-local-env rust-conventions \
             test-authoring test-coverage; do
  if [[ -d "$SRC/$skill" ]]; then
    rm -rf "$DST/framework/$skill"
    cp -r "$SRC/$skill" "$DST/framework/$skill"
    # Sanitize any hardcoded project paths
    find "$DST/framework/$skill" -name '*.md' -exec sed -i \
      -e 's|/Users/chenhan/c9r-io/orchestrator|<project-root>|g' \
      -e 's|/Users/chenhan/|~/|g' \
      {} \;
    echo "  framework/$skill"
  fi
done

# ── SDLC pattern skills (templateable patterns) ─────────────────────────────
for skill in fr-governance qa-testing ticket-fix qa-doc-gen security-test-doc-gen; do
  if [[ -d "$SRC/$skill" ]]; then
    rm -rf "$DST/sdlc-patterns/$skill"
    cp -r "$SRC/$skill" "$DST/sdlc-patterns/$skill"
    find "$DST/sdlc-patterns/$skill" -name '*.md' -exec sed -i \
      -e 's|/Users/chenhan/c9r-io/orchestrator|<project-root>|g' \
      -e 's|/Users/chenhan/|~/|g' \
      -e 's|docs/qa/orchestrator/|docs/qa/<project>/|g' \
      -e 's|docs/design_doc/orchestrator/|docs/design_doc/<project>/|g' \
      -e 's|docs/feature_request/|docs/feature_request/|g' \
      {} \;
    echo "  sdlc-patterns/$skill"
  fi
done

echo ""
echo "Templates packaged to $DST/"
echo "  generic:        $(ls "$DST/generic/" 2>/dev/null | wc -l | tr -d ' ') skills"
echo "  framework:      $(ls "$DST/framework/" 2>/dev/null | wc -l | tr -d ' ') skills"
echo "  sdlc-patterns:  $(ls "$DST/sdlc-patterns/" 2>/dev/null | wc -l | tr -d ' ') skills"
