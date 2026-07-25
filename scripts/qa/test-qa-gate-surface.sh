#!/usr/bin/env bash
#
# FR-127: QA gate enforcement surface.
#
# Every scripts/qa gate must declare how it is enforced. This script verifies the
# manifest against the repository so that a gate can never silently fall into the
# "only the author knows to run it" state, and so that no ci-required gate can
# reach a real provider binary.
#
# Usage:
#   test-qa-gate-surface.sh                 verify the real repository
#   test-qa-gate-surface.sh --fixture-test  prove each check fails on an injected defect

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MANIFEST_REL="config/governance/qa-gate-surface.json"

for command in jq rg git ruby; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORKFLOW_MODEL="$REPO_ROOT/scripts/lib/workflow_model.rb"
MANIFEST_MODEL="$REPO_ROOT/scripts/lib/manifest_model.rb"

# shellcheck source=../lib/gate_preamble.sh
. "$REPO_ROOT/scripts/lib/gate_preamble.sh"

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# ── The five checks, all evaluated against $ROOT so fixtures can run them on a copy ──

# Check 1: bidirectional set equality between disk and manifest.
#
# The scan is recursive. FR-127 wrote `ls scripts/qa/*.sh scripts/qa/*.rb`,
# which classifies exactly the files that sit at the top level, and FR-134 found
# scripts/qa/lib/slack-live-certification-lib.sh already living below it,
# tracked and completely invisible to this check. A subdirectory is not an
# exemption. Files that genuinely are not gates are declared in supportFiles
# with a role and a reason, because "the glob does not reach it" is not a
# statement anyone can review.
check_surface_complete() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL"
  local disk declared missing_from_manifest missing_from_disk
  disk="$(cd "$root" && find scripts/qa -type f \( -name '*.sh' -o -name '*.rb' \) 2>/dev/null | LC_ALL=C sort)"
  declared="$(jq -r '.scripts[].path, (.supportFiles // [])[].path' "$manifest" | LC_ALL=C sort)"

  missing_from_manifest="$(comm -23 <(printf '%s\n' "$disk") <(printf '%s\n' "$declared"))"
  if [[ -n "$missing_from_manifest" ]]; then
    echo "    unclassified script(s) on disk:" >&2
    printf '      %s\n' $missing_from_manifest >&2
    return 1
  fi

  # The reverse direction is "does the declared path exist", not "is it in the
  # discovered set". Discovery looks for executables; a support file may be a
  # JSON case table, and comparing the two sets would demand that data files
  # end in .sh.
  local entry
  missing_from_disk=""
  while read -r entry; do
    [[ -z "$entry" ]] && continue
    [[ -f "$root/$entry" ]] || missing_from_disk+="$entry"$'\n'
  done < <(printf '%s\n' "$declared")
  if [[ -n "$missing_from_disk" ]]; then
    echo "    manifest entries with no file on disk:" >&2
    printf '      %s\n' $missing_from_disk >&2
    return 1
  fi
  return 0
}

# Check 1b: a support file is a declaration about a file that is not a gate, so
# it has to name a role the manifest defines and give a reason. Without this,
# supportFiles becomes an exemption list anyone can grow to silence check 1.
check_support_files_declared() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path role reason
  while IFS=$'\t' read -r path role reason; do
    [[ -z "$path" ]] && continue
    if ! jq -e --arg role "$role" '.supportFileRoles | has($role)' "$manifest" >/dev/null; then
      echo "    $path: support file declares an unknown role: $role" >&2
      rc=1
    fi
    if [[ -z "$reason" || "$reason" == "null" ]]; then
      echo "    $path: support file has no reason" >&2
      rc=1
    fi
  done < <(jq -r '(.supportFiles // [])[] | [.path, (.role // "null"), (.reason // "")] | @tsv' "$manifest")
  return $rc
}

# Check 2: non-ci-required entries carry a reason and an owner document that exists.
check_reason_and_owner() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path owner
  while IFS=$'\t' read -r path owner; do
    [[ -z "$path" ]] && continue
    if [[ -z "$owner" || "$owner" == "null" ]]; then
      echo "    $path: non-ci-required entry has no owner document" >&2
      rc=1
      continue
    fi
    if [[ ! -f "$root/$owner" ]]; then
      echo "    $path: owner document does not exist: $owner" >&2
      rc=1
    fi
  done < <(jq -r '
    .scripts[]
    | select(.enforcement != "ci-required")
    | [.path, (.owner // "null")]
    | @tsv' "$manifest")

  while read -r path; do
    [[ -z "$path" ]] && continue
    echo "    $path: non-ci-required entry has an empty or missing reason" >&2
    rc=1
  done < <(jq -r '
    .scripts[]
    | select(.enforcement != "ci-required")
    | select((.reason // "") | length == 0)
    | .path' "$manifest")

  return $rc
}

# Does the job execute this command? Answered from the workflow's step
# structure, not from its text.
#
# FR-127 asked `grep -F "$path" "$job_block"`. FR-134 reproduced four things
# that satisfies and none of which runs: a `run:` line commented out with an
# explanation beside it, a step disabled by `if: false`, the script named in a
# step's `name:`, and the script mentioned inside a heredoc body. The first is
# the realistic one — "someone disabled the gate and left a note" is how this
# degrades in practice — and the existing fixture tested a misdirected job
# name instead, which routed around it.
workflow_job_runs() {
  local workflow_file="$1" job="$2" command="$3"
  ruby "$WORKFLOW_MODEL" executes "$workflow_file" "$job" "$command" 2>/dev/null
}

workflow_has_job() {
  local workflow_file="$1" job="$2"
  ruby "$WORKFLOW_MODEL" jobs "$workflow_file" 2>/dev/null | grep -qxF "$job"
}

# Check 3: every ci-required entry is genuinely wired into the declared workflow job.
# This is the durable form of "no gate may claim CI enforcement it does not have".
check_wiring_truth() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0
  local path workflow job invoked_by block
  while IFS=$'\t' read -r path workflow job invoked_by; do
    [[ -z "$path" ]] && continue
    if [[ "$workflow" == "null" || "$job" == "null" ]]; then
      echo "    $path: ci-required entry must declare workflow and job" >&2
      rc=1
      continue
    fi
    if [[ ! -f "$root/$workflow" ]]; then
      echo "    $path: declared workflow does not exist: $workflow" >&2
      rc=1
      continue
    fi
    if ! workflow_has_job "$root/$workflow" "$job"; then
      echo "    $path: declared job '$job' not found in $workflow" >&2
      rc=1
      continue
    fi
    if [[ "$invoked_by" == "null" ]]; then
      if ! workflow_job_runs "$root/$workflow" "$job" "./$path"; then
        echo "    $path: job '$job' in $workflow does not execute it" >&2
        echo "      (a commented-out run:, an if: false step, a name: mention or a" >&2
        echo "       heredoc body all reference the script without running it)" >&2
        rc=1
      fi
    else
      if [[ ! -f "$root/$invoked_by" ]]; then
        echo "    $path: declared invoker does not exist: $invoked_by" >&2
        rc=1
        continue
      fi
      if ! workflow_job_runs "$root/$workflow" "$job" "./$invoked_by"; then
        echo "    $path: job '$job' in $workflow does not execute its invoker $invoked_by" >&2
        rc=1
      fi
      # The invoker is a shell script, not a workflow, so its own reference is
      # read as text. Comment stripping is what keeps a disabled call from
      # counting; a full shell parse would be a second implementation of bash
      # for one link in the chain.
      if ! sed -E 's/(^|[[:space:]])#.*$//' "$root/$invoked_by" | grep -qF "$path"; then
        echo "    $path: declared invoker $invoked_by does not call it" >&2
        rc=1
      fi
    fi
  done < <(jq -r '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workflow // "null"), (.job // "null"), (.invokedBy // "null")]
    | @tsv' "$manifest")
  return $rc
}

# Does a manifest bundle contain a claude/codex agent with no fake binary pin?
#
# Asked per agent. FR-127 compared whole-file counts — as many `binary: fake-`
# lines as `provider:` lines meant pinned — which is a different claim from the
# contract's "every claude/codex agent in the bundle also declares
# binary: fake-*". FR-134 reproduced the gap by appending an unpinned
# `provider: claude` agent next to an unrelated agent carrying
# `binary: fake-decoy`: two providers, two pins, gate green, real CLI reachable.
# A total over a file cannot express a property of each object in it.
bundle_has_unpinned_provider() {
  local bundle="$1"
  [[ -f "$bundle" ]] || return 1
  ruby "$MANIFEST_MODEL" unpinned "$bundle" >/dev/null 2>&1
}

# Names the offending agents, so the failure says which one rather than that
# the file as a whole is wrong.
bundle_unpinned_agents() {
  ruby "$MANIFEST_MODEL" unpinned "$1" 2>/dev/null | cut -f1 | paste -sd, -
}

# Which real providers a bundle actually declares. A path-shadow gate has to
# shadow at least these; shadowing claude while the bundle also names codex is
# the hole this closes.
bundle_providers() {
  ruby "$MANIFEST_MODEL" agents "$1" 2>/dev/null \
    | cut -f2 | grep -E '^(claude|codex)$' | LC_ALL=C sort -u
}

# Runs the shared shadow assertion against a synthetic PATH, both ways. An
# assertion that cannot fail is not an assertion, and this is the only part of
# the path-shadow contract that can be established without paying for a full
# parity run: the mechanism is executed, and it is required to reject the
# defect it exists to reject.
provider_shadow_assertion_works() {
  local root="$1"
  local lib="$root/scripts/lib/provider_isolation.sh"
  [[ -f "$lib" ]] || return 1

  local probe
  probe="$(mktemp -d)"
  mkdir -p "$probe/shadow" "$probe/elsewhere"
  printf '#!/bin/sh\nexit 0\n' > "$probe/shadow/claude"
  printf '#!/bin/sh\nexit 0\n' > "$probe/elsewhere/claude"
  chmod 755 "$probe/shadow/claude" "$probe/elsewhere/claude"

  local shadowed unshadowed
  ( . "$lib"; PATH="$probe/shadow:$probe/elsewhere:$PATH" \
      assert_provider_shadow "$probe/shadow" claude ) >/dev/null 2>&1
  shadowed=$?
  ( . "$lib"; PATH="$probe/elsewhere:$PATH" \
      assert_provider_shadow "$probe/shadow" claude ) >/dev/null 2>&1
  unshadowed=$?
  rm -rf "$probe"

  # Accepts a provider inside the shadow, rejects one outside it.
  [[ "$shadowed" -eq 0 && "$unshadowed" -ne 0 ]]
}

# Every real provider the gate's own fixture bundle declares must appear in the
# assertion's argument list. Shadowing claude while the bundle also names codex
# leaves codex reachable, and the gate would still look isolated.
path_shadow_covers_bundle() {
  local root="$1" path="$2" rc=0 bundle provider asserted
  asserted="$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$path" \
    | grep -o 'assert_provider_shadow.*' || true)"
  while read -r bundle; do
    [[ -z "$bundle" ]] && continue
    [[ -f "$root/$bundle" ]] || continue
    while read -r provider; do
      [[ -z "$provider" ]] && continue
      if ! printf '%s' "$asserted" | grep -qw "$provider"; then
        echo "    $path: $bundle declares provider $provider, which the shadow assertion does not cover" >&2
        rc=1
      fi
    done < <(bundle_providers "$root/$bundle")
  done < <(grep -oE 'fixtures/[A-Za-z0-9_/.-]+\.ya?ml' "$root/$path" | LC_ALL=C sort -u)
  return $rc
}

# Check 4: provider isolation is asserted for every ci-required gate.
check_provider_isolation() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0
  local path mode evidence bundle
  while IFS=$'\t' read -r path mode evidence; do
    [[ -z "$path" ]] && continue
    case "$mode" in
      fixture-pinned)
        if [[ "$evidence" == "null" ]]; then
          echo "    $path: fixture-pinned isolation requires providerIsolation.evidence" >&2
          rc=1
          continue
        fi
        if [[ ! -f "$root/$evidence" ]]; then
          echo "    $path: isolation evidence bundle does not exist: $evidence" >&2
          rc=1
          continue
        fi
        if bundle_has_unpinned_provider "$root/$evidence"; then
          echo "    $path: $evidence declares claude/codex agent(s) without a binary: fake-* pin:" >&2
          echo "      $(bundle_unpinned_agents "$root/$evidence")" >&2
          rc=1
        fi
        ;;
      path-shadow)
        # Four conditions. The text ones are kept because removing the shadow
        # setup is a real defect and cheap to see; they are no longer the only
        # ones, because on their own they certified an isolation that had been
        # commented out — a commented line contains the characters the grep
        # was looking for. Comments are stripped first, which is what fixes
        # them; the executed conditions are what stops them being load-bearing.
        #
        # 1. The fake provider is still copied into the shadow directory.
        # 2. The shadow is still put ahead of PATH.
        # 3. The gate calls the shared assertion, and that assertion is
        #    executed here, both ways, against a synthetic PATH. This is the
        #    mechanism running, not a description of it.
        # 4. Every real provider the gate's bundle declares is named in the
        #    assertion's arguments.
        local source_text
        source_text="$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$path")"
        if ! printf '%s\n' "$source_text" | grep -Eq 'cp .*"\$QA_ROOT/bin/(claude|codex)"'; then
          echo "    $path: path-shadow isolation requires copying a fake provider into \$QA_ROOT/bin" >&2
          rc=1
        fi
        if ! printf '%s\n' "$source_text" | grep -Eq 'export PATH="\$QA_ROOT/bin:\$PATH"'; then
          echo "    $path: path-shadow isolation requires exporting \$QA_ROOT/bin ahead of PATH" >&2
          rc=1
        fi
        if ! printf '%s\n' "$source_text" | grep -q 'assert_provider_shadow'; then
          echo "    $path: path-shadow isolation requires calling assert_provider_shadow" >&2
          echo "      after the PATH export, so the shadow proves itself on every run" >&2
          rc=1
        elif ! provider_shadow_assertion_works "$root"; then
          echo "    $path: scripts/lib/provider_isolation.sh does not detect a missing shadow" >&2
          rc=1
        fi
        if ! path_shadow_covers_bundle "$root" "$path"; then
          rc=1
        fi
        ;;
      no-provider)
        # Any fixture bundle the script names must not carry an unpinned provider.
        while read -r bundle; do
          [[ -z "$bundle" ]] && continue
          if bundle_has_unpinned_provider "$root/$bundle"; then
            echo "    $path: declared no-provider but references $bundle, which has an unpinned claude/codex agent" >&2
            rc=1
          fi
        done < <(grep -oE 'fixtures/[A-Za-z0-9_/.-]+\.ya?ml' "$root/$path" | sort -u)
        ;;
      null|"")
        echo "    $path: ci-required entry must declare providerIsolation.mode" >&2
        rc=1
        ;;
      *)
        echo "    $path: unknown providerIsolation.mode: $mode" >&2
        rc=1
        ;;
    esac
  done < <(jq -r '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.providerIsolation.mode // "null"), (.providerIsolation.evidence // "null")]
    | @tsv' "$manifest")
  return $rc
}

# Check 5: no document may claim CI or release-gate enforcement for a gate that has none.
#
# Scope is every tracked Markdown file minus declared exemptions. FR-127 scanned
# `docs` and `.claude/skills`, which left 41 tracked files unread — README.md,
# CHANGELOG.md, AGENTS.md, SKILLS.md, CONTRIBUTING.md and every crate README —
# and a false claim planted in README.md passed. The pattern that works is the
# one test-agent-driver-documentation-alignment.sh already uses and that
# qa-doc-gen states outright: a whitelist of known files is not an acceptable
# scope, because it guards what was known when it was written.
#
# The exemption list is empty and a stale entry fails, so it cannot quietly
# become the whitelist by another name.
CI_CLAIM_PATTERN='release gate|由 CI|CI 门禁|\.github/workflows|GitHub Actions|in CI|CI job'
scanned_markdown() {
  local root="$1"
  local exempt
  exempt="$(jq -r '(.staleClaimExemptions // [])[].path' "$root/$MANIFEST_REL" | LC_ALL=C sort)"
  comm -23 <(cd "$root" && git ls-files '*.md' 2>/dev/null | LC_ALL=C sort) \
           <(printf '%s\n' "$exempt")
}

check_no_stale_claims() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path base hits corpus
  corpus="$(scanned_markdown "$root")"
  [[ -z "$corpus" ]] && {
    echo "    no tracked Markdown found; the scan would pass vacuously" >&2
    return 1
  }
  while read -r path; do
    [[ -z "$path" ]] && continue
    base="$(basename "$path")"
    hits="$(cd "$root" && printf '%s\n' "$corpus" | tr '\n' '\0' \
      | xargs -0 rg -n --no-heading -F "$base" 2>/dev/null | rg -P "$CI_CLAIM_PATTERN" || true)"
    if [[ -n "$hits" ]]; then
      echo "    $path is not ci-required but is documented as CI-enforced:" >&2
      printf '      %s\n' "$hits" >&2
      rc=1
    fi
  done < <(jq -r '.scripts[] | select(.enforcement != "ci-required") | .path' "$manifest")
  return $rc
}

# Check 5b: an exemption outlives the thing it excuses. Every exempted path must
# still be tracked, must carry a reason, and must still contain a claim — an
# exemption for a file that no longer says anything is a licence nobody revoked.
check_no_stale_claim_exemptions() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path reason
  while IFS=$'\t' read -r path reason; do
    [[ -z "$path" ]] && continue
    if ! (cd "$root" && git ls-files --error-unmatch "$path" >/dev/null 2>&1); then
      echo "    stale claim exemption names a file that is not tracked: $path" >&2
      rc=1
      continue
    fi
    if [[ -z "$reason" || "$reason" == "null" ]]; then
      echo "    stale claim exemption has no reason: $path" >&2
      rc=1
      continue
    fi
    if ! (cd "$root" && rg -P "$CI_CLAIM_PATTERN" -q "$path" 2>/dev/null); then
      echo "    stale claim exemption is no longer needed; $path contains no enforcement claim" >&2
      rc=1
    fi
  done < <(jq -r '(.staleClaimExemptions // [])[] | [.path, (.reason // "")] | @tsv' "$manifest")
  return $rc
}

# A gate's declared dependencies come from scripts/lib/gate_preamble.sh, shared
# with test-ci-environment-parity.sh so the two cannot answer the same question
# differently.
script_required_commands() { gate_required_commands "$1"; }

# The commands a job puts on PATH: the declared runner baseline, plus whatever
# its apt/brew installs and its actions provide, mapped through the manifest.
# The mapping lives in the manifest because "the ripgrep package provides rg" is
# a claim about Debian that a reviewer should be able to see and correct.
job_provided_commands() {
  local root="$1" workflow="$2" job="$3"
  local manifest="$root/$MANIFEST_REL"
  {
    jq -r '.commandSources.runnerBaseline[]' "$manifest"
    while IFS=$'\t' read -r kind value; do
      [[ -z "$kind" ]] && continue
      case "$kind" in
        apt|brew)
          jq -r --arg p "$value" '.commandSources.packages[$p][]? // empty' "$manifest"
          ;;
        action)
          jq -r --arg a "$value" '.commandSources.actions[$a][]? // empty' "$manifest"
          ;;
        action-tool)
          echo "$value"
          ;;
      esac
    done < <(ruby "$WORKFLOW_MODEL" installs "$root/$workflow" "$job" 2>/dev/null)
  } | LC_ALL=C sort -u
}

# Check 6: a ci-required gate's dependencies must be satisfied by the job that
# runs it. "Wired" and "able to run" are different claims; FR-127 asserted only
# the first, and test-coordination-strangler.sh satisfied it while failing in CI
# on every push.
check_job_dependencies() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path workflow job missing
  while IFS=$'\t' read -r path workflow job; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    [[ -f "$root/$workflow" ]] || continue
    missing="$(comm -23 <(script_required_commands "$root/$path") \
                        <(job_provided_commands "$root" "$workflow" "$job"))"
    if [[ -n "$missing" ]]; then
      echo "    $path: job '$job' in $workflow does not provide: $(printf '%s ' $missing)" >&2
      echo "      the gate exits on its own missing-command preamble, asserting nothing" >&2
      rc=1
    fi
  done < <(jq -r '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workflow // "null"), (.job // "null")]
    | @tsv' "$manifest")
  return $rc
}

# Check 7: a ci-required gate that runs the whole workspace must exclude what
# the sibling jobs exclude, or say why not.
#
# DD-139 called test-filesystem-trigger.sh's `cargo test --workspace` an
# accepted duplication of the sibling test job. It is not a duplicate, it is a
# superset, and the extra member is the one crate no job can build on Linux.
# That was invisible locally because macOS supplies the Tauri frameworks.
check_workspace_scope() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path declared exclude
  local -a excludes=()
  while read -r exclude; do
    [[ -n "$exclude" ]] && excludes+=("$exclude")
  done < <(jq -r '.workspaceScope.excludes[]' "$manifest")

  while IFS=$'\t' read -r path declared; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    [[ "$declared" != "null" && -n "$declared" ]] && continue
    local line
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      for exclude in "${excludes[@]}"; do
        if [[ "$line" != *"--exclude $exclude"* ]]; then
          echo "    $path: runs the workspace without --exclude $exclude and declares no reason:" >&2
          echo "      $(echo "$line" | sed 's/^[[:space:]]*//')" >&2
          rc=1
        fi
      done
    # Quoted strings are stripped alongside comments: `pass "cargo test
    # --workspace"` is a message reporting on the command, not the command.
    # Matching it would make the check unsatisfiable, and the natural way to
    # silence that is to reword the message — which fixes nothing.
    done < <(sed -E 's/(^|[[:space:]])#.*$//; s/"[^"]*"//g' "$root/$path" \
      | grep -E 'cargo (test|clippy|build|check)[^|;&]*--workspace' || true)
  done < <(jq -r '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workspaceScopeReason // "null")]
    | @tsv' "$manifest")
  return $rc
}

# Check 8: a ci-required gate may not throw away the output of a command whose
# failure it reports.
#
# The CI log for a failing test-filesystem-trigger.sh read `FAIL: cargo test
# --workspace` and nothing else, because the command was run as
# `>/dev/null 2>&1`. Diagnosing it needed a local reproduction and a
# cross-comparison against a sibling job. A gate that can fail without saying
# why costs more than it saves.
check_diagnostics_preserved() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path hits
  while read -r path; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    hits="$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$path" \
      | grep -nE 'cargo [^|;&]*>[[:space:]]*/dev/null[[:space:]]*2>&1' || true)"
    if [[ -n "$hits" ]]; then
      echo "    $path: discards the output of a cargo command it reports on:" >&2
      printf '      %s\n' "$hits" >&2
      rc=1
    fi
  done < <(jq -r '.scripts[] | select(.enforcement == "ci-required") | .path' "$manifest")
  return $rc
}

# Check 9: every job running a gate that is not no-provider installs the stubs.
#
# The exit-97 stubs are the backstop for when a gate's own isolation fails. They
# were installed in the governance job only, so the coordination-strangler job —
# whose gate rests entirely on fixture pinning, the mechanism FR-134 defect 3
# defeated — had no second line at all.
check_provider_stub_coverage() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 workflow job action exempt
  action="$(jq -r '.providerStubs.action' "$manifest")"
  while IFS=$'\t' read -r workflow job; do
    [[ -z "$workflow" ]] && continue
    [[ -f "$root/$workflow" ]] || continue
    exempt="$(jq -r --arg j "$job" \
      '[.providerStubs.exemptJobs[]? | select(.job == $j and ((.reason // "") | length > 0))] | length' "$manifest")"
    [[ "$exempt" -gt 0 ]] && continue
    if ! ruby "$WORKFLOW_MODEL" installs "$root/$workflow" "$job" 2>/dev/null \
      | grep -qxF "action	$action"; then
      echo "    job '$job' in $workflow runs a gate that can reach a provider but does not install the stubs" >&2
      echo "      add: uses: $action" >&2
      rc=1
    fi
  done < <(jq -r '
    .scripts[]
    | select(.enforcement == "ci-required")
    | select((.providerIsolation.mode // "no-provider") != "no-provider")
    | [(.workflow // "null"), (.job // "null")]
    | @tsv' "$manifest" | LC_ALL=C sort -u)
  return $rc
}

# The registry. Both modes read it: verification runs every entry, and the
# fixture mode asserts that it names every check_* the file defines and that
# each one has at least one negative fixture. A check that exists but is not
# registered runs nowhere while still looking like enforcement, which FR-134
# names as the commonest way a gate degrades.
ALL_CHECKS=(
  check_surface_complete
  check_support_files_declared
  check_reason_and_owner
  check_wiring_truth
  check_provider_isolation
  check_no_stale_claims
  check_no_stale_claim_exemptions
  check_job_dependencies
  check_workspace_scope
  check_diagnostics_preserved
  check_provider_stub_coverage
)

run_all_checks() {
  local root="$1"
  local check
  for check in "${ALL_CHECKS[@]}"; do
    "$check" "$root" || return 1
  done
  return 0
}

# ── Fixture mode ────────────────────────────────────────────────────────────────

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo "=== FR-127: QA gate enforcement surface (negative fixtures) ==="
  echo ""

  FIXTURE_ROOT="$(mktemp -d)"
  cleanup_fixtures() { rm -rf "$FIXTURE_ROOT"; }
  trap cleanup_fixtures EXIT

  # A defect-free copy of the governed inputs. Only files the checks read are copied,
  # so the fixtures never touch the working tree.
  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE"
  # scripts/lib carries the workflow, manifest and isolation models the checks
  # execute. Omitting it makes every check die on a missing file, which reads as
  # "the fixture failed" and would let a defect fixture pass for the wrong
  # reason. .git is copied because check_no_stale_claims derives its corpus from
  # git ls-files rather than from a list.
  (cd "$REPO_ROOT" && tar cf - \
    config/governance/qa-gate-surface.json \
    scripts/qa-doc-lint.sh \
    scripts/qa \
    scripts/lib \
    fixtures/manifests/bundles \
    .github/workflows \
    docs \
    .claude/skills) | (cd "$BASE" && tar xf -)

  # A throwaway index, so `git ls-files` inside a case reports that case's files.
  # Copying .git would drag in the whole object store for every fixture.
  (cd "$BASE" && git init -q . && git add -A >/dev/null 2>&1 &&
    git -c user.email=qa@local -c user.name=qa commit -qm base >/dev/null 2>&1) || {
    echo "could not build the fixture git index" >&2
    exit 1
  }

  new_case() {
    local name="$1"
    local dir="$FIXTURE_ROOT/$name"
    cp -R "$BASE" "$dir"
    echo "$dir"
  }

  # A fixture must fail the check it targets, and must fail that check for its own
  # reason rather than by tripping an earlier one. So assert both: the named check
  # rejects the defect, and every other check still passes on the same tree.
  TARGETED=()

  # Applies a mutation and proves it landed. A fixture whose mutation silently
  # fails to match reports "the check accepted the injected defect" when nothing
  # was injected — it accuses the check of the fixture's own bug. That happened
  # here the moment ci.yml's steps gained `id:` lines and two pattern-based
  # fixtures stopped matching, so the guard is not hypothetical.
  inject() {
    local label="$1" file="$2"
    shift 2
    local before after
    before="$(shasum "$file" | cut -d' ' -f1)"
    "$@"
    after="$(shasum "$file" | cut -d' ' -f1)"
    if [[ "$before" == "$after" ]]; then
      fail "$label: the mutation did not apply to ${file##*/}; the fixture proves nothing"
      return 1
    fi
    return 0
  }

  expect_fail() {
    local name="$1"
    local dir="$2"
    local target="$3"
    local why="$4"
    local other
    TARGETED+=("$target")

    if "$target" "$dir" >/dev/null 2>&1; then
      fail "$name: $target accepted the injected defect ($why)"
      return
    fi
    for other in "${ALL_CHECKS[@]}"; do
      [[ "$other" == "$target" ]] && continue
      if ! "$other" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $other, so the fixture does not isolate $target"
        return
      fi
    done
    pass "$name: $why (isolated to $target)"
  }

  # Positive control first — the unmodified copy must pass.
  if run_all_checks "$BASE" > "$FIXTURE_ROOT/base.log" 2>&1; then
    pass "positive control: unmodified repository passes all five checks"
  else
    fail "positive control: unmodified repository does not pass"
    cat "$FIXTURE_ROOT/base.log" >&2
  fi

  # 1. Unclassified script on disk.
  d="$(new_case f1)"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$d/scripts/qa/test-unclassified.sh"
  expect_fail "fixture 1" "$d" check_surface_complete "an unclassified scripts/qa script fails the surface compare"

  # 2. Manifest entry whose script is absent from disk.
  d="$(new_case f2)"
  rm "$d/scripts/qa/test-webhook-trigger.sh"
  expect_fail "fixture 2" "$d" check_surface_complete "a manifest entry with no script on disk fails the surface compare"

  # 3. manual-runbook entry with an empty reason.
  d="$(new_case f3)"
  jq '(.scripts[] | select(.path == "scripts/qa/test-webhook-trigger.sh") | .reason) = ""' \
    "$BASE/$MANIFEST_REL" > "$d/$MANIFEST_REL"
  expect_fail "fixture 3" "$d" check_reason_and_owner "a manual-runbook entry with an empty reason fails the completeness check"

  # 4. ci-required entry whose declared job does not reference it.
  #    Repointed at coordination-strangler rather than clippy: that job installs
  #    ruby, which this gate needs, so the fixture isolates the wiring failure
  #    instead of also tripping the dependency check.
  d="$(new_case f4)"
  jq '(.scripts[] | select(.path == "scripts/qa/test-coordination-governance.sh") | .job) = "coordination-strangler"' \
    "$BASE/$MANIFEST_REL" > "$d/$MANIFEST_REL"
  expect_fail "fixture 4" "$d" check_wiring_truth "a ci-required entry pointing at a job that does not run it fails the wiring check"

  # 5. The PATH shadow removed from the production parity gate.
  #    This is the only barrier between CI and a real claude binary.
  d="$(new_case f5)"
  grep -v 'export PATH="\$QA_ROOT/bin:\$PATH"' \
    "$BASE/scripts/qa/test-agent-driver-production-parity.sh" \
    > "$d/scripts/qa/test-agent-driver-production-parity.sh"
  expect_fail "fixture 5" "$d" check_provider_isolation "removing the export PATH shadow fails the provider isolation check"

  # 6. The fake binary pin removed from a fixture-pinned bundle.
  d="$(new_case f6)"
  grep -v 'binary: fake-' \
    "$BASE/fixtures/manifests/bundles/coordination-strangler-parity.yaml" \
    > "$d/fixtures/manifests/bundles/coordination-strangler-parity.yaml"
  expect_fail "fixture 6" "$d" check_provider_isolation "removing binary: fake-* from a pinned bundle fails the provider isolation check"

  # 7. A document claiming CI enforcement for a manual-runbook gate.
  d="$(new_case f7)"
  printf '\nThis contract is enforced by the release gate via test-webhook-trigger.sh.\n' \
    >> "$d/docs/qa/orchestrator/128-webhook-trigger-infrastructure.md"
  expect_fail "fixture 7" "$d" check_no_stale_claims "a doc claiming CI enforcement for a manual-runbook gate fails the stale claim check"

  # ── FR-134: the four reproduced defects, each as the mutation that reproduced it ──

  # 8. The realistic wiring mutation: a step commented out with an explanation.
  #    Fixture 4 tests a misdirected job name, which routes around this entirely.
  #    Deletion is the case an author has in mind; commenting out with a note is
  #    what actually happens during a flaky-test triage.
  d="$(new_case f8)"
  if inject "fixture 8" "$d/.github/workflows/ci.yml" \
    perl -pi -e 's{^(\s*)run: \./scripts/qa/test-filesystem-trigger\.sh$}{$1# disabled: ./scripts/qa/test-filesystem-trigger.sh was flaky}' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 8" "$d" check_wiring_truth "a run: step commented out with an explanation is not wiring"
  fi

  # 9. The same claim made three other ways, none of which executes anything.
  #    One fixture per shape would triple the runtime for one assertion, so they
  #    share a tree: any of them counting as wiring fails the check.
  d="$(new_case f9)"
  if inject "fixture 9" "$d/.github/workflows/ci.yml" \
    perl -pi -e '
      s{^(\s*)- name: Legacy coordination decommission contracts$}{$1- name: runs ./scripts/qa/test-legacy-coordination-decommission.sh};
      s{^(\s*)run: \./scripts/qa/test-legacy-coordination-decommission\.sh$}{$1if: false\n$1run: |\n$1  cat > /dev/null <<EOF\n$1  ./scripts/qa/test-legacy-coordination-decommission.sh\n$1  EOF};
    ' "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 9" "$d" check_wiring_truth "an if: false step, a name: mention and a heredoc body are not wiring"
  fi

  # 10. An unpinned agent balanced by a fake binary on an unrelated one. Under
  #     the whole-file count this reads as providers=2 pins=2 and passes.
  d="$(new_case f10)"
  cat >> "$d/fixtures/manifests/bundles/coordination-strangler-parity.yaml" <<'BUNDLE'
---
apiVersion: orchestrator/v1
kind: Agent
metadata:
  name: fr134-unpinned
spec:
  driver:
    provider: claude
---
apiVersion: orchestrator/v1
kind: Agent
metadata:
  name: fr134-decoy
spec:
  driver:
    provider: mock
    binary: fake-decoy
BUNDLE
  expect_fail "fixture 10" "$d" check_provider_isolation "an unpinned agent is not excused by a fake binary on a different agent"

  # 11. The PATH shadow assertion commented out. Fixture 5 removes the export
  #     line; commenting it out is the mutation the old grep could not see,
  #     because a commented line contains the same characters.
  d="$(new_case f11)"
  if inject "fixture 11" "$d/scripts/qa/test-agent-driver-production-parity.sh" \
    perl -pi -e 's{^assert_provider_shadow}{# assert_provider_shadow}' \
      "$d/scripts/qa/test-agent-driver-production-parity.sh"; then
    expect_fail "fixture 11" "$d" check_provider_isolation "commenting out the shadow assertion fails the isolation check"
  fi

  # 12. The shared assertion neutered so it can no longer detect a missing
  #     shadow. The call site is untouched, so only executing the mechanism
  #     catches this — no amount of reading the gate would.
  d="$(new_case f12)"
  if inject "fixture 12" "$d/scripts/lib/provider_isolation.sh" \
    perl -0pi -e 's/^assert_provider_shadow\(\) \{$/assert_provider_shadow() {\n  return 0/m' \
      "$d/scripts/lib/provider_isolation.sh"; then
    expect_fail "fixture 12" "$d" check_provider_isolation "an isolation assertion that cannot fail is not an assertion"
  fi

  # 13. A claim planted outside the old scan scope. README.md is one of 41
  #     tracked Markdown files that docs + .claude/skills never reached.
  d="$(new_case f13)"
  printf '\nThis contract is enforced by the release gate via test-webhook-trigger.sh.\n' \
    >> "$d/README.md"
  (cd "$d" && git add README.md >/dev/null 2>&1 || true)
  expect_fail "fixture 13" "$d" check_no_stale_claims "a claim in README.md is inside the scan, not outside it"

  # 14. An exemption for a file that no longer makes any claim. Without this the
  #     exemption list is just the old whitelist wearing a reason.
  d="$(new_case f14)"
  jq '.staleClaimExemptions = [{"path": "README.md", "reason": "fixture: nothing to excuse"}]' \
    "$BASE/$MANIFEST_REL" > "$d/$MANIFEST_REL"
  expect_fail "fixture 14" "$d" check_no_stale_claim_exemptions "an exemption for a file with no claim in it is stale"

  # 15. A script in a subdirectory of scripts/qa. The non-recursive glob could
  #     not see one, and the repository already contained such a file.
  d="$(new_case f15)"
  mkdir -p "$d/scripts/qa/lib"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$d/scripts/qa/lib/hidden-gate.sh"
  expect_fail "fixture 15" "$d" check_surface_complete "a script under scripts/qa/lib is classified, not invisible"

  # 16. A support file declaring a role the manifest does not define.
  d="$(new_case f16)"
  jq '(.supportFiles[] | select(.path == "scripts/qa/lib/slack-live-certification-lib.sh") | .role) = "whatever"' \
    "$BASE/$MANIFEST_REL" > "$d/$MANIFEST_REL"
  expect_fail "fixture 16" "$d" check_support_files_declared "a support file cannot invent its own role"

  # 17. ripgrep removed from the job that installs it. This is the defect that
  #     was live in two jobs for a full FR cycle, restated as a fixture.
  d="$(new_case f17)"
  if inject "fixture 17" "$d/.github/workflows/ci.yml" \
    perl -pi -e 's/ jq ruby ripgrep sqlite3 protobuf-compiler/ jq ruby sqlite3 protobuf-compiler/' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 17" "$d" check_job_dependencies "a job that stops installing ripgrep can no longer run the gates that need it"
  fi

  # 18. The workspace exclusion dropped, recreating the superset that could
  #     never build on Linux.
  d="$(new_case f18)"
  if inject "fixture 18" "$d/scripts/qa/test-filesystem-trigger.sh" \
    perl -pi -e 's/ --workspace --exclude orchestrator-gui/ --workspace/' \
      "$d/scripts/qa/test-filesystem-trigger.sh"; then
    expect_fail "fixture 18" "$d" check_workspace_scope "a gate widening past its sibling jobs without a declared reason fails"
  fi

  # 19. A cargo command whose output is thrown away. No --workspace, so this
  #     targets the diagnostics rule and nothing else.
  #     The redirection is assembled from a variable rather than written out:
  #     this file is itself a ci-required gate, so a source line containing the
  #     literal pattern would make the check fail on the check's own fixture.
  d="$(new_case f19)"
  DISCARD='>/dev/null 2>&1'
  printf '\nif cargo test -p agent-orchestrator %s; then :; fi\n' "$DISCARD" \
    >> "$d/scripts/qa/test-filesystem-trigger.sh"
  expect_fail "fixture 19" "$d" check_diagnostics_preserved "a cargo command with its output discarded fails the diagnostics rule"

  # 20. The stub backstop removed from the job whose gate has no other barrier.
  d="$(new_case f20)"
  if inject "fixture 20" "$d/.github/workflows/ci.yml" \
    perl -0pi -e 's{      # This job.s gate is isolated by fixture pinning alone.*?\n      - name: Install failing provider stubs\n        uses: \./\.github/actions/provider-stubs\n\n}{}s' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 20" "$d" check_provider_stub_coverage "a provider-capable job without the stub backstop fails"
  fi

  # ── Behavioural: the diagnostics rule is about output reaching the log ──
  #
  # check_diagnostics_preserved reads the source, which is a proxy for "the CI
  # log will be diagnosable". Pair it with the fact: put a cargo on PATH that
  # fails with a recognisable compiler error and require that error to come
  # back out of the gate. A gate that satisfied the source rule while still
  # swallowing output would pass the check and fail this.
  d="$(new_case behavioural-diagnostics)"
  mkdir -p "$d/fakebin"
  cat > "$d/fakebin/cargo" <<'FAKE'
#!/usr/bin/env bash
echo "error[E0425]: cannot find value \`fr134_sentinel\` in this scope" >&2
exit 1
FAKE
  chmod 755 "$d/fakebin/cargo"
  # Captured to a file rather than piped: the gate is expected to exit non-zero
  # here, and under pipefail a pipeline reports that instead of grep's verdict.
  PATH="$d/fakebin:$PATH" bash "$d/scripts/qa/test-filesystem-trigger.sh" \
    > "$FIXTURE_ROOT/diagnostics.log" 2>&1 || true
  if grep -q 'error\[E0425\].*fr134_sentinel' "$FIXTURE_ROOT/diagnostics.log"; then
    pass "behavioural: a failing cargo command's diagnosis reaches the gate's output"
  else
    fail "behavioural: a failing cargo command's output did not reach the gate's output"
    tail -20 "$FIXTURE_ROOT/diagnostics.log" >&2
  fi

  # ── Meta: the registry and the fixture set have to stay in step ──
  #
  # A check that exists but is not registered runs nowhere. A check that is
  # registered but has no fixture has never been observed rejecting anything.
  # Both look like enforcement from outside, and both are how these gates decay.
  DEFINED="$(grep -oE '^check_[a-z_]+\(\)' "$0" | sed 's/()$//' | LC_ALL=C sort -u)"
  REGISTERED="$(printf '%s\n' "${ALL_CHECKS[@]}" | LC_ALL=C sort -u)"
  if [[ "$DEFINED" == "$REGISTERED" ]]; then
    pass "meta: ALL_CHECKS names every check the file defines"
  else
    fail "meta: ALL_CHECKS and the defined checks differ"
    comm -23 <(printf '%s\n' "$DEFINED") <(printf '%s\n' "$REGISTERED") \
      | sed 's/^/      defined but never run: /' >&2
    comm -13 <(printf '%s\n' "$DEFINED") <(printf '%s\n' "$REGISTERED") \
      | sed 's/^/      registered but not defined: /' >&2
  fi

  UNTESTED="$(comm -23 <(printf '%s\n' "$REGISTERED") \
                       <(printf '%s\n' "${TARGETED[@]}" | LC_ALL=C sort -u))"
  if [[ -z "$UNTESTED" ]]; then
    pass "meta: every registered check is targeted by at least one negative fixture"
  else
    fail "meta: a registered check has no fixture proving it rejects anything"
    printf '      %s\n' $UNTESTED >&2
  fi

  echo ""
  echo "FR-127 gate surface fixtures: $PASS passed, $FAIL failed"
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

# ── Verification mode ───────────────────────────────────────────────────────────

echo "=== FR-127: QA gate enforcement surface ==="
echo ""

if check_surface_complete "$REPO_ROOT"; then
  pass "every scripts/qa file at any depth is classified and every classified path exists on disk"
else
  fail "manifest and scripts/qa disagree"
fi

if check_support_files_declared "$REPO_ROOT"; then
  pass "every support file declares a known role and a reason"
else
  fail "a support file has an unknown role or no reason"
fi

if check_reason_and_owner "$REPO_ROOT"; then
  pass "every non-ci-required gate declares a reason and an owner document that exists"
else
  fail "a non-ci-required gate is missing its reason or owner document"
fi

if check_wiring_truth "$REPO_ROOT"; then
  pass "every ci-required gate is executed by a live step of the workflow job it declares"
else
  fail "a ci-required gate is not actually executed by its declared workflow job"
fi

if check_provider_isolation "$REPO_ROOT"; then
  pass "every ci-required gate has a provider isolation mechanism that was executed and rejects its own absence"
else
  fail "a ci-required gate can reach an unpinned provider binary"
fi

if check_no_stale_claims "$REPO_ROOT"; then
  pass "no tracked Markdown claims CI or release-gate enforcement for a gate that has none"
else
  fail "a document claims CI enforcement that does not exist"
fi

if check_no_stale_claim_exemptions "$REPO_ROOT"; then
  pass "every stale-claim exemption still names a tracked file that still makes a claim"
else
  fail "a stale-claim exemption has outlived the claim it excuses"
fi

if check_job_dependencies "$REPO_ROOT"; then
  pass "every ci-required gate's required commands are provided by the job that runs it"
else
  fail "a ci-required gate exits on a missing command before asserting anything"
fi

if check_workspace_scope "$REPO_ROOT"; then
  pass "every ci-required gate's workspace scope matches its sibling jobs or declares why not"
else
  fail "a ci-required gate runs a wider workspace than any job can build"
fi

if check_diagnostics_preserved "$REPO_ROOT"; then
  pass "no ci-required gate discards the output of a command it reports on"
else
  fail "a ci-required gate can fail without saying why"
fi

if check_provider_stub_coverage "$REPO_ROOT"; then
  pass "every job running a provider-capable gate installs the failing provider stubs"
else
  fail "a provider-capable gate runs in a job with no stub backstop"
fi

echo ""
CI_COUNT="$(jq '[.scripts[] | select(.enforcement == "ci-required")] | length' "$REPO_ROOT/$MANIFEST_REL")"
TOTAL="$(jq '.scripts | length' "$REPO_ROOT/$MANIFEST_REL")"
echo "Enforcement surface: $CI_COUNT of $TOTAL gates are ci-required"
echo "FR-127 gate surface: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
