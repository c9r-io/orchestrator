#!/usr/bin/env bash
# FR-174: decide whether this run does meta-verification inline or defers it.
#
# Prints `full` or `deferred` on stdout, the reason on stderr, and appends
# `tier=<t>` to $GITHUB_OUTPUT when that variable is set. Nothing else.
#
# Why a script rather than a `run:` block in ci.yml: the interesting behaviour
# is what it does when git will not answer, and a block inside a workflow can
# only be checked by reading it. scripts/qa/test-ci-tier.sh drives this against
# real throwaway repositories — a missing base ref, a base that does not exist,
# a diff that fails — and observes the verdict. §4.4's distinction between a
# proxy and an observation is the whole reason FR-174 can be trusted to have
# removed work rather than removed checking.
#
# **Every failure path yields `full`.** Not a default, a decision: the only way
# to reach `deferred` is a diff that was read successfully and contained nothing
# under the tiered roots. An empty diff is `full` too — "no files changed" is
# not evidence that no gate changed, it is evidence that the question was not
# answered.
#
# The roots are FR-174's three plus `.github/workflows/`. That fourth is not in
# the FR and is not optional: a PR editing a workflow is editing the tiering
# mechanism, and a mechanism that can exempt its own edits from verification is
# the one shape this must not have.
#
# Bash 3.2 (macOS ships it, bash32-compat.rb enforces it repository-wide).
set -uo pipefail

TIER_ROOTS_PATTERN='^(scripts/qa/|scripts/lib/|config/governance/|\.github/workflows/)'

emit() {
  # $1 = tier, $2 = reason
  printf '%s\n' "$1"
  printf '%s\n' "$2" >&2
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    printf 'tier=%s\n' "$1" >> "$GITHUB_OUTPUT"
  fi
}

decide() {
  if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
    emit full "event is '${GITHUB_EVENT_NAME:-unset}', not pull_request"
    return
  fi

  base="${GITHUB_BASE_REF:-}"
  if [ -z "$base" ]; then
    emit full "no GITHUB_BASE_REF to diff against"
    return
  fi

  # A shallow checkout has no merge base. Fetching is allowed to fail — the
  # ref may already be present — so the verdict rests on the diff below, not
  # on this.
  git fetch --no-tags --quiet origin "$base" >/dev/null 2>&1 || true

  base_ref="origin/$base"
  if ! git rev-parse --verify --quiet "$base_ref" >/dev/null 2>&1; then
    if git rev-parse --verify --quiet "$base" >/dev/null 2>&1; then
      base_ref="$base"
    else
      emit full "base ref '$base' is not resolvable"
      return
    fi
  fi

  if ! changed="$(git diff --name-only "$base_ref...HEAD" 2>/dev/null)"; then
    emit full "git diff against $base_ref failed"
    return
  fi

  if [ -z "$changed" ]; then
    emit full "diff against $base_ref named no files; refusing to read that as 'no gate changed'"
    return
  fi

  # Here-string, not `printf | grep -q`. Under `set -o pipefail` the reader
  # leaves on the first match, the producer takes EPIPE, and the pipeline
  # reports failure on a *successful* match — which in this condition inverts
  # the verdict to `deferred`, the one direction that silently drops
  # meta-verification. FR-145's defect, in the branch where it fails open.
  if grep -qE "$TIER_ROOTS_PATTERN" <<< "$changed"; then
    emit full "changeset touches a tiered root"
    return
  fi

  emit deferred "changeset touches no tiered root"
}

decide
