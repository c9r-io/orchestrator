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
# shellcheck source=../lib/gate_jq.sh
. "$REPO_ROOT/scripts/lib/gate_jq.sh"
# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

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
#
# The root is `scripts`, not `scripts/qa`. FR-158 measured the difference: 28 of
# 122 tracked scripts sat outside the scanned root, and they included every
# shared library the ci-required gates source — scripts/lib/rust_source.rb,
# workflow_model.rb, gate_jq.sh and nine more. The gates were governed and the
# engine they run on was not, which is the arrangement this manifest exists to
# make impossible. Check 14 covers what a workflow *runs*; nothing covered what
# a gate *sources*, and a library is where a defect reaches every caller at once.
#
# `.mjs` joins .sh and .rb for the reason WorkflowModel::SCRIPT_TOKEN now carries
# it: the extension list was doing the same work as a hand-written file list.
check_surface_complete() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL"
  local disk declared missing_from_manifest missing_from_disk
  disk="$(cd "$root" && find scripts -type f \( -name '*.sh' -o -name '*.rb' -o -name '*.mjs' \) 2>/dev/null | LC_ALL=C sort)"
  # Piping jq into sort would hand back sort's status, and this function runs in
  # condition position where set -e is off, so a jq failure here used to leave
  # $declared empty. That direction happens to fail closed — every file on disk
  # then looks unclassified — but the diagnostic would blame the repository for
  # a broken manifest, so it is read properly rather than left to luck.
  declared="$(gate_jq_rows require-rows "$manifest" '.scripts[].path, (.supportFiles // [])[].path')" || return 1
  declared="$(printf '%s\n' "$declared" | LC_ALL=C sort)"

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
  local manifest="$root/$MANIFEST_REL" rc=0 path role reason rows status
  # allow-empty: a repository with no support files is legitimate, which is what
  # the `// []` in the query already says.
  rows="$(gate_jq_rows allow-empty "$manifest" '(.supportFiles // [])[] | [.path, (.role // "null"), (.reason // "")] | @tsv')" || return 1
  while IFS=$'\t' read -r path role reason; do
    [[ -z "$path" ]] && continue
    # `jq -e` returns 1 for a false result and 5 for an error, and the two used
    # to be conflated by `if !`: a malformed manifest was reported as "declares
    # an unknown role", which sends the reader after the wrong file.
    jq -e --arg role "$role" '.supportFileRoles | has($role)' "$manifest" >/dev/null 2>&1
    status=$?
    if [[ "$status" -eq 1 ]]; then
      echo "    $path: support file declares an unknown role: $role" >&2
      rc=1
    elif [[ "$status" -ne 0 ]]; then
      echo "    $manifest: jq exited $status testing the role of $path" >&2
      rc=1
    fi
    if [[ -z "$reason" || "$reason" == "null" ]]; then
      echo "    $path: support file has no reason" >&2
      rc=1
    fi
  done <<< "$rows"
  return $rc
}

# Check 2: non-ci-required entries carry a reason and an owner document that exists.
check_reason_and_owner() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path owner rows
  # allow-empty on both: a surface where every gate is ci-required would select
  # nothing here, and that is the healthy end state rather than a defect. The
  # second query selects violations, so zero rows is exactly what passing means.
  rows="$(gate_jq_rows allow-empty "$manifest" '
    .scripts[]
    | select(.enforcement != "ci-required")
    | [.path, (.owner // "null")]
    | @tsv')" || return 1
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
  done <<< "$rows"

  rows="$(gate_jq_rows allow-empty "$manifest" '
    .scripts[]
    | select(.enforcement != "ci-required")
    | select((.reason // "") | length == 0)
    | .path')" || return 1
  while read -r path; do
    [[ -z "$path" ]] && continue
    echo "    $path: non-ci-required entry has an empty or missing reason" >&2
    rc=1
  done <<< "$rows"

  return $rc
}

# "Does the job execute this command?" is answered from the workflow's step
# structure, not from its text, by the workflow model's executes predicate.
#
# FR-127 asked `grep -F "$path" "$job_block"`. FR-134 reproduced four things
# that satisfies and none of which runs: a `run:` line commented out with an
# explanation beside it, a step disabled by `if: false`, the script named in a
# step's `name:`, and the script mentioned inside a heredoc body. The first is
# the realistic one — "someone disabled the gate and left a note" is how this
# degrades in practice — and the existing fixture tested a misdirected job
# name instead, which routed around it.
#
# Check 3: every ci-required entry is genuinely wired into the declared workflow job.
# This is the durable form of "no gate may claim CI enforcement it does not have".
#
# All the model questions go out in one batch before the loop. Asked per entry,
# the wiring check spawned two ruby processes per ci-required gate, and the
# fixture mode runs this check once per fixture tree — interpreter start-up
# alone was the single largest cost in the gate's recorded step (FR-140: a
# gate's cost is part of its design). The batch emits one query line per
# manifest row, blank rows included, so the answers pair back up with the rows
# by line position and nothing can slip out of register.
check_wiring_truth() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0
  local path workflow job invoked_by verdict rows queries results
  # require-rows: this is the ci-required population itself. A surface with zero
  # CI-enforced gates is not a state this repository can reach, so reading none
  # means the query or the manifest is broken, not that the work is done. This
  # is the check the FR-140 typo silenced.
  rows="$(gate_jq_rows require-rows "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workflow // "null"), (.job // "null"), (.invokedBy // "null")]
    | @tsv')" || return 1
  # Exactly one executes-question exists per row: the gate itself when it is
  # run directly, its invoker when it is not. Rows the loop below rejects
  # before reading the verdict still send a (harmless) question, because the
  # pairing is positional.
  queries=""
  while IFS=$'\t' read -r path workflow job invoked_by; do
    if [[ -z "$path" || "$workflow" == "null" ]]; then
      queries+=$'\t\t\n'
    elif [[ "$invoked_by" == "null" ]]; then
      queries+="$root/$workflow"$'\t'"$job"$'\t'"./$path"$'\n'
    else
      queries+="$root/$workflow"$'\t'"$job"$'\t'"./$invoked_by"$'\n'
    fi
  done <<< "$rows"
  results="$(printf '%s' "$queries" | ruby "$WORKFLOW_MODEL" executes-batch 2>/dev/null)" || {
    echo "    the workflow model could not answer the wiring queries" >&2
    return 1
  }
  while IFS=$'\t' read -r path workflow job invoked_by verdict; do
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
    if [[ "$verdict" == "no-such-job" || "$verdict" == "no-such-workflow" ]]; then
      echo "    $path: declared job '$job' not found in $workflow" >&2
      rc=1
      continue
    fi
    if [[ "$invoked_by" == "null" ]]; then
      if [[ "$verdict" != "runs" ]]; then
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
      if [[ "$verdict" != "runs" ]]; then
        echo "    $path: job '$job' in $workflow does not execute its invoker $invoked_by" >&2
        rc=1
      fi
      # The invoker is a shell script, not a workflow, so its own reference is
      # read as text. Comment stripping is what keeps a disabled call from
      # counting; a full shell parse would be a second implementation of bash
      # for one link in the chain.
      if ! grep -qF "$path" <<< "$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$invoked_by")"; then
        echo "    $path: declared invoker $invoked_by does not call it" >&2
        rc=1
      fi
    fi
  done < <(paste <(printf '%s\n' "$rows") <(printf '%s\n' "$results"))
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
      if ! grep -qw "$provider" <<< "$asserted"; then
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
  local path mode evidence bundle rows
  # This is the query FR-140's typo broke. Writing `"providerIsolation":
  # "no-provider"` instead of `{"mode": "no-provider"}` made jq exit 5 here, the
  # loop read zero rows, and the check returned success — the gate printed
  # "13 passed, 0 failed" while enforcing nothing. require-rows, because the
  # ci-required population cannot be empty.
  rows="$(gate_jq_rows require-rows "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.providerIsolation.mode // "null"), (.providerIsolation.evidence // "null")]
    | @tsv')" || return 1
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
        if ! grep -Eq 'cp .*"\$QA_ROOT/bin/(claude|codex)"' <<< "$source_text"; then
          echo "    $path: path-shadow isolation requires copying a fake provider into \$QA_ROOT/bin" >&2
          rc=1
        fi
        if ! grep -Eq 'export PATH="\$QA_ROOT/bin:\$PATH"' <<< "$source_text"; then
          echo "    $path: path-shadow isolation requires exporting \$QA_ROOT/bin ahead of PATH" >&2
          rc=1
        fi
        if ! grep -q 'assert_provider_shadow' <<< "$source_text"; then
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
  done <<< "$rows"
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
  # An unread exemption list would silently widen the scanned corpus rather than
  # narrow it, so this one fails closed. It is read properly anyway: a caller
  # cannot tell the difference between "no exemptions" and "could not read them"
  # unless the reader says so.
  exempt="$(gate_jq_rows allow-empty "$root/$MANIFEST_REL" '(.staleClaimExemptions // [])[].path')" || return 1
  exempt="$(printf '%s\n' "$exempt" | LC_ALL=C sort)"
  comm -23 <(cd "$root" && git ls-files '*.md' 2>/dev/null | LC_ALL=C sort) \
           <(printf '%s\n' "$exempt")
}

# Prose only: fenced code blocks are dropped before matching.
#
# A claim is something a document asserts. A reproduction command inside a fence
# is sample text — it shows what an injected defect looks like, and treating it
# as an assertion makes the gate unable to document its own fixtures. FR-134's
# text was itself blocked by this while describing the four reproductions, and
# QA 183 was blocked again while recording them.
#
# The alternative was an exemption per document, which would grow by one entry
# every time someone writes about the gate — the enumeration failure this FR
# spent its length removing. The markdown link gate already draws exactly this
# line for exactly this reason, so the two now agree about what a fence means.
# Prose in the same file is still read: fixtures 7 and 13 append plain sentences
# and are still caught.
#
# One awk over the whole corpus, not one per file. The per-file form spawned a
# process for each of ~590 files on every call, and this check is called once
# per fixture per run — tens of thousands of spawns, which is tolerable on a
# developer machine and not on a runner.
prose_only_corpus() {
  local root="$1"
  (cd "$root" && scanned_markdown "$root" | tr '\n' '\0' | xargs -0 awk '
    FNR == 1 { fence = 0 }
    /^[ \t]*(```|~~~)/ { fence = !fence; next }
    !fence { printf "%s:%d:%s\n", FILENAME, FNR, $0 }
  ')
}

check_no_stale_claims() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path base hits corpus claims rows
  corpus="$(prose_only_corpus "$root")"
  [[ -z "$corpus" ]] && {
    echo "    no tracked Markdown prose found; the scan would pass vacuously" >&2
    return 1
  }
  # A hit is a line that names the gate AND makes an enforcement claim. The two
  # filters commute, so the expensive one — the claim pattern over the whole
  # corpus — runs once here, and the loop probes the surviving lines per gate
  # instead of pushing megabytes of prose through a pipeline per manifest entry.
  claims="$(rg -P "$CI_CLAIM_PATTERN" <<< "$corpus" || true)"
  # allow-empty: a surface where every gate became ci-required would select
  # nothing, and that is the healthy end state rather than a broken read.
  rows="$(gate_jq_rows allow-empty "$manifest" '.scripts[] | select(.enforcement != "ci-required") | .path')" || return 1
  while read -r path; do
    [[ -z "$path" ]] && continue
    base="$(basename "$path")"
    hits="$(printf '%s\n' "$claims" | grep -F "$base" || true)"
    if [[ -n "$hits" ]]; then
      echo "    $path is not ci-required but is documented as CI-enforced:" >&2
      printf '      %s\n' "$hits" >&2
      rc=1
    fi
  done <<< "$rows"
  return $rc
}

# Check 5b: an exemption outlives the thing it excuses. Every exempted path must
# still be tracked, must carry a reason, and must still contain a claim — an
# exemption for a file that no longer says anything is a licence nobody revoked.
check_no_stale_claim_exemptions() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path reason rows
  # allow-empty, and this is the case that makes the declaration mandatory
  # rather than defaulted: the exemption list is empty today, and an empty
  # exemption list is the best possible state. require-rows here would demand
  # that somebody keep an exemption alive to keep the gate quiet.
  rows="$(gate_jq_rows allow-empty "$manifest" '(.staleClaimExemptions // [])[] | [.path, (.reason // "")] | @tsv')" || return 1
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
  done <<< "$rows"
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
#
# Every jq read here is captured and tested before use. A `{ …; } | sort -u`
# block hands back sort's status, so an unreadable manifest used to shrink the
# provided-command set to nothing — which made check 6 report that the job
# provides none of the gate's dependencies. That failed closed, but it accused
# the workflow of a defect the manifest has; check 6 now observes this
# function's status and fails on its own diagnostic instead.
job_provided_commands() {
  local root="$1" workflow="$2" job="$3"
  local manifest="$root/$MANIFEST_REL"
  local baseline installs provided kind value extra
  baseline="$(gate_jq_rows require-rows "$manifest" '.commandSources.runnerBaseline[]')" || return 1
  installs="$(ruby "$WORKFLOW_MODEL" installs "$root/$workflow" "$job" 2>/dev/null)"
  provided="$baseline"
  while IFS=$'\t' read -r kind value; do
    [[ -z "$kind" ]] && continue
    case "$kind" in
      apt|brew)
        extra="$(gate_jq_rows allow-empty "$manifest" --arg p "$value" '.commandSources.packages[$p][]? // empty')" || return 1
        ;;
      action)
        extra="$(gate_jq_rows allow-empty "$manifest" --arg a "$value" '.commandSources.actions[$a][]? // empty')" || return 1
        ;;
      action-tool)
        extra="$value"
        ;;
      *)
        extra=""
        ;;
    esac
    [[ -n "$extra" ]] && provided="$provided"$'\n'"$extra"
  done <<< "$installs"
  printf '%s\n' "$provided" | LC_ALL=C sort -u
}

# Check 6: a ci-required gate's dependencies must be satisfied by the job that
# runs it. "Wired" and "able to run" are different claims; FR-127 asserted only
# the first, and test-coordination-strangler.sh satisfied it while failing in CI
# on every push.
check_job_dependencies() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path workflow job missing rows
  local provided_cache cache_file
  # What a job provides is a property of the (workflow, job) pair, and most
  # ci-required gates share one pair. Asked per gate this spawned ruby and a
  # dozen manifest reads per entry, which the fixture mode then multiplied by
  # its tree count (FR-140: a gate's cost is part of its design). Cached per
  # pair for this call only — the cache lives and dies inside one tree, so no
  # answer can leak between fixture trees. A pair whose derivation fails is
  # still this check failing, exactly as the uncached form's `|| return 1`s
  # inside job_provided_commands intended.
  provided_cache="$(mktemp -d)"
  rows="$(gate_jq_rows require-rows "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workflow // "null"), (.job // "null")]
    | @tsv')" || { rm -rf "$provided_cache"; return 1; }
  while IFS=$'\t' read -r path workflow job; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    [[ -f "$root/$workflow" ]] || continue
    cache_file="$provided_cache/$(printf '%s|%s' "$workflow" "$job" | tr -c 'A-Za-z0-9' '_')"
    if [[ ! -f "$cache_file" ]]; then
      if ! job_provided_commands "$root" "$workflow" "$job" > "$cache_file"; then
        rm -rf "$provided_cache"
        return 1
      fi
    fi
    missing="$(comm -23 <(script_required_commands "$root/$path") "$cache_file")"
    if [[ -n "$missing" ]]; then
      echo "    $path: job '$job' in $workflow does not provide: $(printf '%s ' $missing)" >&2
      echo "      the gate exits on its own missing-command preamble, asserting nothing" >&2
      rc=1
    fi
  done <<< "$rows"
  rm -rf "$provided_cache"
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
  local rows
  # require-rows: an empty exclude list does not make this check pass, it makes
  # it inert — the inner loop never runs and every workspace-wide gate goes
  # unexamined. If the list ever legitimately empties, changing this one word is
  # a reviewable diff; silently reading zero was not.
  rows="$(gate_jq_rows require-rows "$manifest" '.workspaceScope.excludes[]')" || return 1
  while read -r exclude; do
    [[ -n "$exclude" ]] && excludes+=("$exclude")
  done <<< "$rows"

  local scope_rows
  scope_rows="$(gate_jq_rows require-rows "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workspaceScopeReason // "null")]
    | @tsv')" || return 1
  while IFS=$'\t' read -r path declared; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    [[ "$declared" != "null" && -n "$declared" ]] && continue
    local line
    while IFS= read -r line; do
      [[ -z "$line" ]] && continue
      for exclude in ${excludes[@]+"${excludes[@]}"}; do
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
  done <<< "$scope_rows"
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
  local manifest="$root/$MANIFEST_REL" rc=0 path hits rows
  rows="$(gate_jq_rows require-rows "$manifest" '.scripts[] | select(.enforcement == "ci-required") | .path')" || return 1
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
  done <<< "$rows"
  return $rc
}

# Check 9: a gate that reads git history must run in a job that has some.
#
# actions/checkout fetches one commit unless told otherwise, so `git merge-base`,
# `git cat-file` and `git diff <sha>^ <sha>` all fail in CI while passing on any
# developer machine, which has the whole clone. FR-134 found three assertions in
# this state — and not just any three: they are the retirement-parity evidence
# that the governance process requires before a removal can be called closed.
# The recorded baseline commit is reachable, the compatibility window is an
# ordered interval, and the runner-removal patch is mechanically revertible.
# None of them had ever been verified in CI, and the failure was invisible
# because an earlier step in the same job stopped the run before reaching them.
# Matches the shell form `git merge-base …` and the Ruby form
# `git("merge-base", …)`, because ci-liveness.rb uses the second and the first
# pattern written here missed it entirely. A gate that reaches history through a
# helper needs the history just as much as one that types the command out.
GIT_HISTORY_PATTERN='\bgit\b[^|;&]{0,80}\b(merge-base|cat-file|rev-list|describe)\b|git (diff|show|log)[^|;&]*(COMMIT|\^)'
check_git_history_available() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path workflow job depth rows
  local depth_cache cache_file
  # A job's checkout depth is a property of the (workflow, job) pair; cached per
  # pair for this call only, for the same reason and with the same scope as
  # check 6's provided-command cache.
  depth_cache="$(mktemp -d)"
  rows="$(gate_jq_rows require-rows "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | [.path, (.workflow // "null"), (.job // "null")]
    | @tsv')" || { rm -rf "$depth_cache"; return 1; }
  while IFS=$'\t' read -r path workflow job; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    [[ -f "$root/$workflow" ]] || continue
    rg -qP "$GIT_HISTORY_PATTERN" <<< "$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$path")" || continue
    cache_file="$depth_cache/$(printf '%s|%s' "$workflow" "$job" | tr -c 'A-Za-z0-9' '_')"
    if [[ ! -f "$cache_file" ]]; then
      ruby "$WORKFLOW_MODEL" checkout-depth "$root/$workflow" "$job" 2>/dev/null > "$cache_file" || true
    fi
    depth="$(<"$cache_file")"
    if [[ "$depth" != "0" ]]; then
      echo "    $path reads git history but job '$job' checks out with fetch-depth $depth" >&2
      echo "      every history query fails on a shallow clone, and passes on any" >&2
      echo "      developer machine, so the gate is green locally and dead in CI" >&2
      rc=1
    fi
  done <<< "$rows"
  rm -rf "$depth_cache"
  return $rc
}

# Check 10: every job running a gate that is not no-provider installs the stubs.
#
# The exit-97 stubs are the backstop for when a gate's own isolation fails. They
# were installed in the governance job only, so the coordination-strangler job —
# whose gate rests entirely on fixture pinning, the mechanism FR-134 defect 3
# defeated — had no second line at all.
check_provider_stub_coverage() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 workflow job action exempt rows
  action="$(gate_jq_rows require-rows "$manifest" '.providerStubs.action')" || return 1
  # allow-empty: a surface on which no ci-required gate can reach a provider is
  # the state this check exists to drive towards, so zero rows is success rather
  # than a broken read. A jq failure is still caught, by the status.
  rows="$(gate_jq_rows allow-empty "$manifest" '
    .scripts[]
    | select(.enforcement == "ci-required")
    | select((.providerIsolation.mode // "no-provider") != "no-provider")
    | [(.workflow // "null"), (.job // "null")]
    | @tsv')" || return 1
  rows="$(printf '%s\n' "$rows" | LC_ALL=C sort -u)"
  while IFS=$'\t' read -r workflow job; do
    [[ -z "$workflow" ]] && continue
    [[ -f "$root/$workflow" ]] || continue
    exempt="$(gate_jq_rows require-rows "$manifest" --arg j "$job" \
      '[.providerStubs.exemptJobs[]? | select(.job == $j and ((.reason // "") | length > 0))] | length')" || return 1
    [[ "$exempt" -gt 0 ]] && continue
    if ! grep -qxF "action	$action" \
      <<< "$(ruby "$WORKFLOW_MODEL" installs "$root/$workflow" "$job" 2>/dev/null)"; then
      echo "    job '$job' in $workflow runs a gate that can reach a provider but does not install the stubs" >&2
      echo "      add: uses: $action" >&2
      rc=1
    fi
  done <<< "$rows"
  return $rc
}

# Check 11: a job that swallows a step's failure must aggregate that step.
#
# FR-134 made the governance steps `continue-on-error: true` so one run reports
# every problem instead of only the first, and put a final step that reads each
# outcome and fails the job. That is the right structure. What guarded it was a
# hand-written `OUTCOMES` list — the enumeration shape FR-134 spent its length
# removing everywhere else, reappearing inside its own fix. The list has grown
# 19 → 20 → 21 → 22 across four FRs, once per cycle, and nothing has ever
# checked it. Add a gate, forget the `OUTCOMES` line, and the classification,
# wiring and dependency checks all still pass while that gate fails on every run
# and the job reports success.
#
# Three ways a swallowed failure disappears, and all three are asserted:
#
#   - the step has no `id`. Nothing can reference it, so it can never be
#     aggregated by construction. FR-137 specified the check over steps "with an
#     id and continue-on-error", which does not see this case at all — and this
#     is the cheaper mistake to make, because an id is only ever added when
#     someone already intends to read the outcome.
#   - the step has an `id` no one reads as `.outcome`. The omission direction:
#     silent, the job stays green, the gate is dead.
#   - an `.outcome` names a step the job does not have. The dangling direction.
#     FR-137 argued this "resolves to empty forever, with the same effect as the
#     omission". Measured, it is the opposite: an absent step's outcome is the
#     empty string, the aggregate's loop finds it is neither success nor
#     skipped, and the job fails — permanently, and for a reason that names a
#     gate which no longer exists. It is loud rather than silent, and it is
#     still a defect: the job can never go green again, and the step that was
#     renamed is now unaggregated in the first sense.
#
# Every job of every discovered workflow, not the one job that does this today.
# Naming `governance` here would make this check the same kind of list it exists
# to abolish. FR-137's non-goal asked for the narrow form; the general form
# passes on this repository unchanged, because `governance` is the only job with
# a `continue-on-error` step at all.
check_continue_on_error_aggregated() {
  local root="$1"
  local rc=0 facts pairs workflow job coe_ids step_ids refs anonymous offenders

  facts="$(ruby "$WORKFLOW_MODEL" outcome-facts "$root" 2>&1)" || {
    echo "    could not read workflow outcome facts from $root:" >&2
    printf '      %s\n' "$facts" >&2
    return 1
  }

  pairs="$(awk -F'\t' '$1 == "coe" || $1 == "ref" { print $2 "\t" $3 }' <<< "$facts" \
    | LC_ALL=C sort -u)"

  while IFS=$'\t' read -r workflow job; do
    [[ -z "$workflow" ]] && continue

    # A step whose failure is swallowed and which carries no id.
    anonymous="$(awk -F'\t' -v w="$workflow" -v j="$job" \
      '$1 == "coe" && $2 == w && $3 == j && $4 == "" { print $5 }' <<< "$facts")"
    if [[ -n "$anonymous" ]]; then
      echo "    $workflow job '$job': a continue-on-error step has no id, so no step can read its outcome:" >&2
      printf '      %s\n' "$anonymous" >&2
      echo "      its failure is swallowed and unreportable; give it an id and aggregate it" >&2
      rc=1
    fi

    coe_ids="$(awk -F'\t' -v w="$workflow" -v j="$job" \
      '$1 == "coe" && $2 == w && $3 == j && $4 != "" { print $4 }' <<< "$facts" | LC_ALL=C sort -u)"
    step_ids="$(awk -F'\t' -v w="$workflow" -v j="$job" \
      '$1 == "step" && $2 == w && $3 == j { print $4 }' <<< "$facts" | LC_ALL=C sort -u)"
    refs="$(awk -F'\t' -v w="$workflow" -v j="$job" \
      '$1 == "ref" && $2 == w && $3 == j { print $4 }' <<< "$facts" | LC_ALL=C sort -u)"

    # Omission: swallowed, and nothing reads the outcome.
    offenders="$(comm -23 <(printf '%s\n' "$coe_ids") <(printf '%s\n' "$refs"))"
    if [[ -n "$offenders" ]]; then
      echo "    $workflow job '$job': a continue-on-error step's outcome is never read:" >&2
      printf '      %s\n' $offenders >&2
      echo "      the step can fail on every run while the job reports success" >&2
      rc=1
    fi

    # Dangling: an outcome is read for a step that is not in this job.
    offenders="$(comm -23 <(printf '%s\n' "$refs") <(printf '%s\n' "$step_ids"))"
    if [[ -n "$offenders" ]]; then
      echo "    $workflow job '$job': an outcome is read for a step id the job does not define:" >&2
      printf '      %s\n' $offenders >&2
      echo "      the reference resolves to the empty string, so the job can never pass again" >&2
      rc=1
    fi
  done <<< "$pairs"

  return $rc
}

# Check 16: a new ci-required gate names the failure shape that requires it.
#
# This is FR-158's actual subject. FR-127 through FR-149 produced 23 FRs in six
# days and most of them were governance work; the surface grew because each
# individual gate was defensible and nothing ever asked what the whole thing
# cost. A ci-required gate is paid on every push by everyone forever, and
# "which recorded way of being wrong does this catch" is the cheapest filter
# against adding one out of unease. A gate that cannot answer it is usually a
# test, and a test belongs in cargo test at a fraction of the price.
#
# The 52 exemptions are the gates that existed when the rule was written. That
# list may only shrink, which is what separates it from the enumeration §4.4
# shape 2 condemns: a guard-list is wrong the moment something lands outside it,
# while this one is a statement about a past commit and cannot go stale. It is
# self-cleaning in the one direction that matters — an exemption naming a path
# that is no longer a ci-required gate fails here, so it cannot outlive the gate
# it excuses — and it grows only by someone editing the manifest, which is the
# visible, reviewable act the rule exists to force.
check_new_gates_name_their_shape() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL"
  local rc=0 gates exempt path shape

  gates="$(gate_jq_rows require-rows "$manifest" \
    '.scripts[] | select(.enforcement == "ci-required") | [.path, (.shape // "")] | @tsv')" || return 1
  exempt="$(gate_jq_rows allow-empty "$manifest" \
    '(.shapeRationale.exemptions // [])[]')" || return 1

  while IFS=$'\t' read -r path shape; do
    [[ -z "$path" ]] && continue
    [[ -n "$shape" ]] && continue
    if ! grep -qxF "$path" <<< "$exempt"; then
      echo "    $path: ci-required with no 'shape' and no exemption" >&2
      echo "      name the §4.4 failure shape this gate exists to catch, or make it" >&2
      echo "      a cargo test — a gate is a permanent cost on every push" >&2
      rc=1
    fi
  done <<< "$gates"

  # The exemption list may not outlive what it excuses. Without this it would
  # become a permanent amnesty by attrition: gates retire, the entries stay, and
  # a future path colliding with a retired one inherits the exemption silently.
  local ci_paths
  ci_paths="$(gate_jq_rows require-rows "$manifest" \
    '.scripts[] | select(.enforcement == "ci-required") | .path')" || return 1
  while read -r path; do
    [[ -z "$path" ]] && continue
    if ! grep -qxF "$path" <<< "$ci_paths"; then
      echo "    $path: named in shapeRationale.exemptions but is not a ci-required gate" >&2
      echo "      the exemption has outlived the gate it excused; delete the entry" >&2
      rc=1
    fi
  done <<< "$exempt"

  return $rc
}

# Check 15: every manual-runbook gate records its own runs.
#
# 35 gates are executed by a person and nothing observed when that last happened.
# ci-job-liveness.json tracks workflow jobs and cannot see them, which is how
# test-coordination-collapse.sh stayed broken from 07-25 and
# test-wp05-integration.sh from 2026-03-26 — four months — both found by reading
# rather than by any signal.
#
# The required set is derived from the manifest, never listed: the next gate
# classified manual-runbook is covered the moment it is classified, and a list
# here would guard exactly the 35 that existed today.
#
# Both conditions are structural, and neither is sufficient alone:
#
#   - the gate sources scripts/lib/gate_runlog.sh, and
#   - it calls gate_runlog_arm with its own manifest path.
#
# Matching its own path matters. Copying a block from a neighbouring gate is how
# these files get written, and an armed gate recording under the neighbour's name
# leaves both entries lying — one fresh that nobody ran, one stale that someone
# did. That is worse than no record, because it reads as a working ledger.
#
# What this cannot see is whether the arming actually fires: text presence is
# §4.4 shape 1, satisfied by a commented-out call. The behavioural half is
# fixture 29, which runs a gate with its own EXIT trap under the real library and
# asserts the record, the cleanup and the exit status together. Neither half is
# the check; both together are.
check_manual_gates_record_runs() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL"
  local rc=0 gates path

  gates="$(gate_jq_rows require-rows "$manifest" \
    '.scripts[] | select(.enforcement == "manual-runbook") | .path')" || return 1

  # "Declared but absent from disk" belongs to check 1 and is deliberately not
  # repeated here: two checks reporting one defect is what stops a negative
  # fixture from isolating either of them, and fixture 2 caught exactly that.
  while read -r path; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue

    # Anchored at the start of a line, so a commented-out call does not satisfy
    # it. Written with grep -F first, which fixture 29 failed against on the
    # first attempt: `# gate_runlog_arm "..."` contains the literal string, and
    # the check certified an arming that cannot run — §4.4 shape 1 in the code
    # written to enforce §4.4.
    if ! grep -qE '^[[:space:]]*\.[[:space:]].*scripts/lib/gate_runlog\.sh' "$root/$path"; then
      echo "    $path: manual-runbook gate does not source scripts/lib/gate_runlog.sh" >&2
      echo "      its runs are invisible, so the freshness ledger cannot tell" >&2
      echo "      'nobody has run this in a year' from 'someone ran it this morning'" >&2
      rc=1
    fi
    if ! grep -qE "^[[:space:]]*gate_runlog_arm \"${path//./\\.}\"[[:space:]]*$" "$root/$path"; then
      echo "    $path: no active gate_runlog_arm call naming its own path" >&2
      echo "      either the call is absent, commented out, or names another gate;" >&2
      echo "      a gate armed under another gate's path records into that gate's" >&2
      echo "      entry, leaving one fresh that nobody ran and one stale that someone did" >&2
      rc=1
    fi
  done <<< "$gates"

  return $rc
}

# Check 16: daemon teardown goes through the shared library, nowhere else.
#
# FR-160 measured the shape this ratchets: 23 gates read a daemon PID from a
# pidfile and called `wait` on it, which returns immediately for a non-child,
# so every cleanup's `rm -rf` raced a live writer (run 30795701182). The repair
# is scripts/lib/gate_daemon.sh; this check is what keeps the other 24 sites
# true after the 25th is written by copying a neighbour — §4.4 shape 2, with
# FR-159's local-repair-recorded-as-done as shape 9 beside it.
#
# Two conditions, mirroring check 15's structure. Scope is derived
# (`git ls-files`, never a roster) and comments are stripped first — for an
# absence condition, stripping prevents a commented-out example from reading
# as a violation, the inverse of the check-15 `grep -F` lesson.
#   A (absence): no live `kill`/`wait` — with or without a signal flag — aimed
#     at a variable whose name contains DAEMON. The library is the one place
#     allowed to signal a daemon PID, and it names its variables `pid`.
#   B (pairing): a file that assigns a daemon PID (any non-empty
#     `*DAEMON*PID=` right-hand side) must source the library and call
#     gate_daemon_stop at least once.
# What this cannot see, stated rather than papered over: whether
# gate_daemon_stop is *reached* at runtime (the probe and the per-gate
# execution records in QA 211 carry that half), and a gate that names its
# variable SERVER_PID escapes the scope predicate — "DAEMON" is a fact about
# today's tree (25/25 use it), recorded as a known limit in DD 174 rather
# than widened into a regex that would start flagging the session PIDs the
# FR's cross-check warning protects.
check_daemon_teardown_shared() {
  local root="$1"
  local rc=0 files file stripped hits
  files="$(git -C "$root" ls-files 'scripts/**/*.sh')" || {
    echo "    git ls-files failed; the teardown scan read nothing" >&2
    return 1
  }
  # Empty input fails closed (§4.4 shape 5): zero scanned files and a clean
  # scan are different facts, and only one of them is evidence.
  if [[ -z "$files" ]]; then
    echo "    no tracked shell scripts under scripts/; the teardown scan read nothing" >&2
    return 1
  fi
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    [[ "$file" == "scripts/lib/gate_daemon.sh" ]] && continue
    [[ -f "$root/$file" ]] || continue
    stripped="$(sed -E 's/(^|[[:space:]])#.*$//' "$root/$file")"
    hits="$(grep -nE '(^|[^A-Za-z_"])(kill|wait)( -[A-Za-z0-9]+)? "\$[A-Za-z_]*DAEMON[A-Za-z_]*"' <<<"$stripped" || true)"
    if [[ -n "$hits" ]]; then
      echo "    $file: signals or waits on a daemon PID directly:" >&2
      sed 's/^/      /' <<<"$hits" >&2
      echo "      route the stop through gate_daemon_stop (scripts/lib/gate_daemon.sh);" >&2
      echo "      \`wait\` on a pidfile PID is a no-op and the cleanup's rm -rf races a live writer" >&2
      rc=1
    fi
    # Captured, not piped into `grep -q`: an early-leaving reader under
    # pipefail turns the producer's EPIPE into the condition's status — the
    # scanner this repository runs against itself flagged the first draft.
    assignments="$(grep -E '(^|[[:space:]])[A-Za-z_]*DAEMON[A-Za-z_]*PID=' <<<"$stripped" \
      | grep -vE 'PID=""[[:space:]]*$' || true)"
    if [[ -n "$assignments" ]]; then
      if ! grep -qE '^[[:space:]]*\.[[:space:]].*scripts/lib/gate_daemon\.sh' <<<"$stripped"; then
        echo "    $file: assigns a daemon PID but never sources scripts/lib/gate_daemon.sh" >&2
        rc=1
      fi
      if ! grep -q 'gate_daemon_stop' <<<"$stripped"; then
        echo "    $file: assigns a daemon PID but never calls gate_daemon_stop" >&2
        rc=1
      fi
    fi
  done <<< "$files"
  return $rc
}

# Check 14: every scripts/** executable a workflow job runs is declared here.
#
# Checks 1 and 3 together look like they cover this and do not. Check 1 compares
# the manifest against `find scripts/qa`, so a gate living in scripts/ rather
# than scripts/qa/ is outside the discovered set entirely; check 3 asks whether
# each *declared* entry is really executed, which is the opposite direction and
# says nothing about a script nobody declared. FR-147 measured the hole: three
# gates — qa-doc-lint.sh, coverage-governance.sh, check-async-lock-governance.sh
# — had been running in ci.yml for months with no entry here, so every scanner
# that derives its scope from this manifest (jq-status-observed.rb,
# fixture-target-drift.rb) had never read them. The most pointed instance:
# test-agent-driver-documentation-alignment.sh named qa-doc-lint.sh as its
# `invokedBy`, so the callee was governed while the caller was not.
#
# Scope is every workflow, not ci.yml. Narrowing to ci.yml because that is where
# the known gaps were is §4.4 shape 2 aimed at this check — it would guard the
# one workflow its author had in mind and let the next instance land silently in
# another. Two of the four workflows here already run governance gates.
#
# The undeclared set must be empty. A script that is not a gate is declared in
# supportFiles with a role and a reason, per path and never as a directory or a
# glob: a subtree exemption goes on absorbing files that do not exist yet and
# never produces a line in any log.
check_workflow_execution_declared() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0
  local records declared path workflow job undeclared=""

  # The executed set is derived from the workflow model, so the three things
  # that are not execution stay out of it: a commented-out `run:`, an
  # `if: false` step, and a script named inside a heredoc body. A grep over the
  # workflow files would count all three and this check would certify a gate
  # that never runs.
  #
  # The status is observed. Left in condition position the ruby call would run
  # with `set -e` disabled for its whole call tree, and a model that died on a
  # malformed workflow would hand back an empty set that reads exactly like
  # "every executed script is declared".
  if records="$(ruby "$WORKFLOW_MODEL" executed-scripts "$root" 2>&1)"; then
    :
  else
    echo "    could not derive the executed set from the workflow model:" >&2
    printf '      %s\n' "$records" >&2
    return 1
  fi

  # Fail closed on an empty read. This population cannot be empty — the
  # repository has four workflows and this very gate is one of the scripts they
  # run — so reading nothing means the model or the checkout is broken, not that
  # the work is done. Zero rows and N passing rows are indistinguishable in an
  # exit code, which is how a sibling gate once printed "13 passed, 0 failed"
  # over a manifest it could not parse.
  if [[ -z "$records" ]]; then
    echo "    the workflow model reported no executed scripts at all" >&2
    echo "      the repository runs governance gates from .github/workflows, so an" >&2
    echo "      empty set is a broken derivation, not a clean result" >&2
    return 1
  fi

  declared="$(gate_jq_rows require-rows "$manifest" \
    '.scripts[].path, (.supportFiles // [])[].path')" || return 1

  while IFS=$'\t' read -r path workflow job; do
    [[ -z "$path" ]] && continue
    if ! grep -qxF "$path" <<< "$declared"; then
      undeclared+="      $path (run by $workflow job '$job')"$'\n'
      rc=1
    fi
  done <<< "$records"

  if [[ -n "$undeclared" ]]; then
    echo "    workflow job(s) execute script(s) absent from $MANIFEST_REL:" >&2
    printf '%s' "$undeclared" >&2
    echo "      declare each as a gate in scripts[], or as a non-gate in" >&2
    echo "      supportFiles[] with a role and a reason" >&2
  fi

  # Being declared is not enough; the declaration has to be one that permits
  # direct execution. Of the supportFile roles, `fixture`, `library`,
  # `developer-tool` and `spike` all say the file is not invoked as a gate by a
  # workflow — a fixture is data or a fake binary a gate consumes, a library runs
  # only inside its callers, and the last two are not run by CI at all. Only
  # `release-tooling` and `generator` describe a file a workflow runs at top
  # level. Without this condition the trigger rule below is bypassed by
  # relabelling: declare a governance gate `library`, and it is declared, exempt,
  # and never checked again. That is the cheaper mutation and the one worth
  # blocking.
  #
  # `generator` exists because `release-tooling`'s discipline is its trigger
  # rule, and that rule cannot reach a generator run by a branch-push workflow:
  # scripts/sync-docs.mjs is executed by docs.yml on every push to main, so it is
  # neither a library nor release-tooling, and before FR-158 widened
  # WorkflowModel::SCRIPT_TOKEN to .mjs nothing here could see it at all. Its
  # discipline is `verifiedBy` instead — a named ci-required gate that
  # regenerates the artifact and compares — checked below rather than trusted,
  # because a role whose only condition is a free-text field is not a condition.
  local support_rows support_path support_role
  support_rows="$(gate_jq_rows require-rows "$manifest" \
    '(.supportFiles // [])[] | [.path, .role] | @tsv')" || return 1
  while IFS=$'\t' read -r support_path support_role; do
    [[ -z "$support_path" ]] && continue
    [[ "$support_role" == "release-tooling" || "$support_role" == "generator" ]] && continue
    while IFS=$'\t' read -r path workflow job; do
      [[ "$path" == "$support_path" ]] || continue
      echo "    $support_path: declared supportFiles role '$support_role', but $workflow" >&2
      echo "      job '$job' executes it directly. That role means the file is never" >&2
      echo "      invoked as a gate itself; a script a workflow runs is either a gate" >&2
      echo "      in scripts[] or release-tooling or generator" >&2
      rc=1
    done <<< "$records"
  done <<< "$support_rows"

  # `generator` is only an exemption while its verifier is real. The field must
  # name a path this manifest classifies as a ci-required gate: a missing field,
  # a dangling path, or a verifier that is itself manual-runbook all collapse the
  # role back into an unconditional amnesty. Checked for every generator entry,
  # not only for the ones a workflow was observed running, because the role's
  # claim is about the artifact and not about today's execution set.
  local gen_rows gen_path gen_verifier ci_required
  ci_required="$(gate_jq_rows require-rows "$manifest" \
    '.scripts[] | select(.enforcement == "ci-required") | .path')" || return 1
  gen_rows="$(gate_jq_rows allow-empty "$manifest" \
    '(.supportFiles // [])[] | select(.role == "generator") | [.path, (.verifiedBy // "")] | @tsv')" || return 1
  while IFS=$'\t' read -r gen_path gen_verifier; do
    [[ -z "$gen_path" ]] && continue
    if [[ -z "$gen_verifier" ]]; then
      echo "    $gen_path: role 'generator' with no verifiedBy; the role asserts that a" >&2
      echo "      ci-required gate proves this generator's output, and nothing names one" >&2
      rc=1
    elif ! grep -qxF "$gen_verifier" <<< "$ci_required"; then
      echo "    $gen_path: verifiedBy names '$gen_verifier', which is not a ci-required" >&2
      echo "      gate in this manifest; the generator would run on every push with" >&2
      echo "      nothing on any push proving what it produced" >&2
      rc=1
    fi
  done <<< "$gen_rows"

  # The exemption is conditional, and this is what makes it so. `release-tooling`
  # says "this runs only to build or publish an artifact", and that claim is
  # false the moment the script also runs on ordinary development activity — a
  # branch push or a pull request. Without this, the role would be a permanent
  # amnesty: move a governance gate into scripts/package-release.sh's entry and
  # nothing would ever look at it again. Derived from each workflow's parsed
  # trigger map rather than from a list of workflow names, because a list is the
  # failure this check exists to avoid.
  # Classify each workflow once. Asked per (entry, record) pair this spawned ruby
  # four deep inside a nested loop, and the fixture mode runs this check some
  # thirty times — the governance job has a recorded budget and FR-140 is explicit
  # that a gate's cost is part of its design.
  local dev_triggered="" candidate
  while read -r candidate; do
    [[ -z "$candidate" ]] && continue
    if ruby "$WORKFLOW_MODEL" development-triggered "$root/$candidate" >/dev/null 2>&1; then
      dev_triggered+="$candidate"$'\n'
    fi
  done <<< "$(cut -f2 <<< "$records" | LC_ALL=C sort -u)"

  local exempt exempt_path
  exempt="$(gate_jq_rows allow-empty "$manifest" \
    '(.supportFiles // [])[] | select(.role == "release-tooling") | .path')" || return 1
  while read -r exempt_path; do
    [[ -z "$exempt_path" ]] && continue
    while IFS=$'\t' read -r path workflow job; do
      [[ "$path" == "$exempt_path" ]] || continue
      if grep -qxF "$workflow" <<< "$dev_triggered"; then
        echo "    $exempt_path: declared release-tooling, but $workflow job '$job' runs it" >&2
        echo "      on branch pushes or pull requests, so it is enforcement on every" >&2
        echo "      change; classify it in scripts[] instead" >&2
        rc=1
      fi
    done <<< "$records"
  done <<< "$exempt"

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
  check_git_history_available
  check_continue_on_error_aggregated
  check_workflow_execution_declared
  check_manual_gates_record_runs
  check_new_gates_name_their_shape
  check_daemon_teardown_shared
)

run_all_checks() {
  local root="$1"
  local check
  for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
    "$check" "$root" || return 1
  done
  return 0
}

describe_check() {
  case "$1" in
    check_surface_complete)
      echo "every scripts file at any depth is classified and every classified path exists on disk|manifest and scripts disagree" ;;
    check_manual_gates_record_runs)
      echo "every manual-runbook gate sources the runlog library and arms it with its own path|a manual-runbook gate's executions are invisible to the freshness ledger" ;;
    check_new_gates_name_their_shape)
      echo "every ci-required gate added since FR-158 names the failure shape requiring it, and no exemption outlives its gate|a new ci-required gate was added without stating what it catches" ;;
    check_daemon_teardown_shared)
      echo "every gate that assigns a daemon PID stops it through gate_daemon.sh, and nothing signals or waits on one directly|a script tears down its daemon outside the shared contract, where wait on a pidfile PID never waits" ;;
    check_support_files_declared)
      echo "every support file declares a known role and a reason|a support file has an unknown role or no reason" ;;
    check_reason_and_owner)
      echo "every non-ci-required gate declares a reason and an owner document that exists|a non-ci-required gate is missing its reason or owner document" ;;
    check_wiring_truth)
      echo "every ci-required gate is executed by a live step of the workflow job it declares|a ci-required gate is not actually executed by its declared workflow job" ;;
    check_provider_isolation)
      echo "every ci-required gate has a provider isolation mechanism that was executed and rejects its own absence|a ci-required gate can reach an unpinned provider binary" ;;
    check_no_stale_claims)
      echo "no tracked Markdown prose claims CI or release-gate enforcement for a gate that has none|a document claims CI enforcement that does not exist" ;;
    check_no_stale_claim_exemptions)
      echo "every stale-claim exemption still names a tracked file that still makes a claim|a stale-claim exemption has outlived the claim it excuses" ;;
    check_job_dependencies)
      echo "every ci-required gate's required commands are provided by the job that runs it|a ci-required gate exits on a missing command before asserting anything" ;;
    check_workspace_scope)
      echo "every ci-required gate's workspace scope matches its sibling jobs or declares why not|a ci-required gate runs a wider workspace than any job can build" ;;
    check_diagnostics_preserved)
      echo "no ci-required gate discards the output of a command it reports on|a ci-required gate can fail without saying why" ;;
    check_provider_stub_coverage)
      echo "every job running a provider-capable gate installs the failing provider stubs|a provider-capable gate runs in a job with no stub backstop" ;;
    check_git_history_available)
      echo "every ci-required gate that reads git history runs in a job that fetches it|a ci-required gate queries history its job did not fetch" ;;
    check_continue_on_error_aggregated)
      echo "every step whose failure a job swallows is aggregated by that job, and every outcome read names a step that exists|a job swallows a step's failure without aggregating it, or reads an outcome for a step it does not have" ;;
    check_workflow_execution_declared)
      echo "every script a workflow job executes is declared here, and no release-tooling exemption runs on a branch push or a pull request|a workflow job runs a script this manifest has never heard of, so every scanner deriving scope from it is blind to that script" ;;
    *) return 1 ;;
  esac
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
  # This list is itself the enumeration §4.4 shape 2 warns about, and it has
  # already gone stale once: FR-147 classified three gates that live in scripts/
  # rather than scripts/qa/, and only qa-doc-lint.sh was here. A path the fixture
  # tree lacks makes check_workflow_execution_declared report it as undeclared in
  # every case, which reads as "the check works" while every fixture below is
  # actually failing for the fixture's own reason. Derived from the manifest
  # instead of typed, so classifying a fourth script outside scripts/qa needs no
  # edit here.
  # `.mjs` is included for the same reason check 1 and SCRIPT_TOKEN now include
  # it. Without it the coverage and sync-docs entries are declared but absent
  # from the fixture tree, and check 1's reverse direction — "a manifest entry
  # with no file on disk" — fails in every case, which reads as thirty working
  # fixtures when it is thirty fixtures failing for the harness's own reason.
  MANIFEST_OUTSIDE_QA="$(jq -r '
    (.scripts[].path, (.supportFiles // [])[].path)
    | select(startswith("scripts/") and (startswith("scripts/qa/") | not))
    | select(endswith(".sh") or endswith(".rb") or endswith(".mjs"))' \
    "$REPO_ROOT/$MANIFEST_REL" | LC_ALL=C sort -u)"
  if [[ -z "$MANIFEST_OUTSIDE_QA" ]]; then
    echo "the manifest named no scripts outside scripts/qa; the fixture tree would" >&2
    echo "be built without the files check_workflow_execution_declared reads" >&2
    exit 1
  fi

  # shellcheck disable=SC2086
  (cd "$REPO_ROOT" && tar cf - \
    config/governance/qa-gate-surface.json \
    $MANIFEST_OUTSIDE_QA \
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
  # The body moved to scripts/lib/gate_fixture.sh, where FR-143 generalised it
  # for the nine other gates that needed it and had it nowhere. Kept as a name
  # because thirty call sites below read `inject`, and because two copies of
  # this logic in one repository is the drift the extraction exists to prevent.
  #
  # What the shared version adds: the target must be an existing regular file
  # (a fixture in a sibling gate wrote to a directory once its target list went
  # empty), and a mutation command that fails is reported with its own stderr
  # rather than taking the run down.
  inject() {
    fixture_mutate "$@"
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
    for other in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
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
    perl -pi -e 's{^(\s*)run: FR085_SKIP_WORKSPACE=1 \./scripts/qa/test-filesystem-trigger\.sh$}{$1# disabled: ./scripts/qa/test-filesystem-trigger.sh was flaky}' \
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

  # 13b. Positive control for the same rule: the identical sentence inside a
  #      fenced block is a reproduction being described, not a claim being made.
  #      Without this the gate cannot document its own fixtures, and the FR that
  #      specified it was itself blocked while writing them down. This is the
  #      assertion that would fail if someone "fixed" the false positive by
  #      dropping fence handling instead of adding an exemption.
  d="$(new_case f13b)"
  {
    printf '\nReproduction:\n\n```bash\n'
    printf 'printf "Enforced by the release gate via test-webhook-trigger.sh." >> README.md\n'
    printf '```\n'
  } >> "$d/README.md"
  (cd "$d" && git add README.md >/dev/null 2>&1 || true)
  if check_no_stale_claims "$d" >/dev/null 2>&1; then
    pass "fixture 13b: the same sentence inside a fenced block is described, not claimed"
  else
    fail "fixture 13b: a fenced reproduction was read as a claim; the gate cannot document itself"
  fi

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

  # 21. The governance job's checkout reverted to a shallow clone. This is the
  #     defect as it actually existed: three retirement-parity assertions —
  #     the recorded baseline commit is reachable, the compatibility window is
  #     an ordered interval, and the removal patch is reverse-applicable — had
  #     never once passed in CI, and passed on every developer machine.
  d="$(new_case f21)"
  if inject "fixture 21" "$d/.github/workflows/ci.yml" \
    perl -0pi -e 's{(  governance:.*?uses: actions/checkout\@v7\n)        with:\n          fetch-depth: 0\n}{$1}s' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 21" "$d" check_git_history_available "a gate reading git history in a shallow-checkout job fails"
  fi

  # ── FR-137: the aggregate list was an enumeration nobody guarded ──

  # 22. The omission direction, which is the reproduction FR-137 was filed on: a
  #     gate that fails on every run, inside a job that reports success, because
  #     one line was not added to OUTCOMES. Nothing else in this file sees it —
  #     the step is classified, wired, and has its dependencies.
  d="$(new_case f22)"
  if inject "fixture 22" "$d/.github/workflows/ci.yml" \
    perl -0pi -e 's{^(      - name: Governance result$)}{      - name: FR-137 orphan gate\n        id: fr137-orphan\n        continue-on-error: true\n        run: exit 1\n\n$1}m' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 22" "$d" check_continue_on_error_aggregated "a continue-on-error step missing from OUTCOMES fails on every run inside a job that passes"
  fi

  # 22b. Positive control for the same rule, and the half of FR-137's acceptance
  #      criterion that says the step passes "once added to OUTCOMES". Fixtures
  #      22, 23 and 24 only ever ask the check to say no, and every one of them
  #      is satisfied by a check that rejects any edited ci.yml — or that always
  #      returns 1. This is the assertion that makes it say yes, for the right
  #      reason, on a tree that is not the pristine one.
  d="$(new_case f22b)"
  if inject "fixture 22b" "$d/.github/workflows/ci.yml" \
    perl -0pi -e 's{^(      - name: Governance result$)}{      - name: FR-137 orphan gate\n        id: fr137-orphan\n        continue-on-error: true\n        run: exit 1\n\n$1}m; s{^(            execution-migration=\$\{\{ steps\.execution-migration\.outcome \}\})$}{$1\n            fr137-orphan=\${{ steps.fr137-orphan.outcome }}}m' \
      "$d/.github/workflows/ci.yml"; then
    if check_continue_on_error_aggregated "$d" >/dev/null 2>&1; then
      pass "fixture 22b: the same step with its OUTCOMES line added passes, so the check is not just rejecting the edit"
    else
      fail "fixture 22b: a correctly aggregated step was rejected"
      check_continue_on_error_aggregated "$d" >&2 || true
    fi
  fi

  # 23. The dangling direction: a record naming a step that is not there, which
  #     is what a rename leaves behind. Only the OUTCOMES block is touched, so
  #     this cannot also trip the omission direction — the two are asserted
  #     separately because one fixture satisfying both would leave either rule
  #     free to be deleted.
  d="$(new_case f23)"
  if inject "fixture 23" "$d/.github/workflows/ci.yml" \
    perl -pi -e 's{^(            execution-migration=\$\{\{ steps\.execution-migration\.outcome \}\})$}{$1\n            fr137-ghost=\${{ steps.fr137-ghost.outcome }}}' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 23" "$d" check_continue_on_error_aggregated "an OUTCOMES record naming a step the job does not define fails the aggregation check"
  fi

  # 24. The same swallowed failure with no id at all — unaggregatable by
  #     construction, since there is nothing to write on the left of `.outcome`.
  #     This is the mutation the implementation is least likely to catch, and
  #     the one FR-137's own requirement (steps "with an id and
  #     continue-on-error") would have walked straight past. It is also the
  #     likelier accident: an id is typed only when someone already means to
  #     read the outcome, so forgetting the id and forgetting the OUTCOMES line
  #     are the same lapse.
  d="$(new_case f24)"
  if inject "fixture 24" "$d/.github/workflows/ci.yml" \
    perl -0pi -e 's{^(      - name: Governance result$)}{      - name: FR-137 anonymous gate\n        continue-on-error: true\n        run: exit 1\n\n$1}m' \
      "$d/.github/workflows/ci.yml"; then
    expect_fail "fixture 24" "$d" check_continue_on_error_aggregated "a continue-on-error step with no id cannot be aggregated by anything"
  fi

  # ── FR-147: the manifest is complete with respect to what CI executes ──
  #
  # All three targets are derived from the manifest rather than named. The
  # subject of this check is a set that is meant to grow, so a fixture that
  # hardcodes a path only works until the next gate is classified — nine recorded
  # times a fixture's named target moved and eight stayed green (§4.4 shape 7).
  #
  # These three call fixture_mutate under its own name rather than through
  # inject(). fixture-target-drift.rb recognises the landing proof by the
  # statement's leading word, and `inject` is a local alias it cannot see
  # through: it reported all three of these as unproven mutations. The thirty
  # older call sites are invisible to that rule for an unrelated reason — they
  # rewrite with `perl -pi -e`, which its in-place pattern does not match — so
  # the blind spot has never had a reason to show before now. Naming the shared
  # function directly is both the honest form and the one the scanner can read.
  # Recorded in DD-160 as a known limit of that gate.

  # 25. A gate ci.yml still runs, with its manifest entry deleted. The FR asked
  #     for exactly this.
  #
  #     The mutation removes the file from disk as well, and that is not
  #     cosmetic. Until FR-158 the entry only had to be one outside scripts/qa,
  #     because scripts/qa was the only tree check 1 could see — which was also
  #     the reason the hole existed. Check 1 now scans all of scripts, so
  #     deleting a manifest entry alone leaves the file on disk unclassified and
  #     trips check 1 too, and expect_fail rightly refuses a fixture that fails
  #     two checks. Deleting both restores the isolation and states a sharper
  #     case: the gate is gone from the repository and from the manifest, the
  #     workflow still calls it by name, and only this check can say so. The
  #     executed set is parsed out of the workflow, not read off the disk, which
  #     is precisely why it still sees the call.
  d="$(new_case f25)"
  victim="$(jq -r '
    [.scripts[] | select(.enforcement == "ci-required")
      | select(.path | startswith("scripts/qa/") | not) | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$victim" ]]; then
    fail "fixture 25: the manifest declares no ci-required gate outside scripts/qa, so the case that motivated this check cannot be built"
  else
    # `if fixture_mutate`, never `elif`: fixture-target-drift.rb recognises the
    # landing proof only at the head of a statement, and an elif reads to it as
    # an unwrapped in-place rewrite. The landing proof is on the manifest; the
    # file removal rides along in the same command so the two cannot separate.
    if fixture_mutate "fixture 25" "$d/$MANIFEST_REL" \
      ruby -rjson -e '
        path, victim, root = ARGV
        data = JSON.parse(File.read(path))
        data["scripts"].reject! { |entry| entry["path"] == victim }
        # The shape exemption goes with the gate. Leaving it behind is a real
        # defect, but it is fixture 32s defect, and a mutation that trips two
        # checks isolates neither.
        data["shapeRationale"]["exemptions"].delete(victim) if data["shapeRationale"]
        File.write(path, JSON.pretty_generate(data) + "\n")
        script = File.join(root, victim)
        File.delete(script) if File.file?(script)
      ' "$d/$MANIFEST_REL" "$victim" "$d"; then
      expect_fail "fixture 25" "$d" check_workflow_execution_declared \
        "a gate deleted from the repository and the manifest, still named by a workflow job, fails the completeness compare"
    fi
  fi

  # 26. The relabelling bypass, and the mutation an author is least likely to
  #     have in mind. Deleting a release-tooling entry is the obvious defect and
  #     fixture 25 already covers that shape; the cheap way to silence this check
  #     is to leave the path declared and change its role to one that carries no
  #     trigger condition. `library` is a role the manifest defines, so
  #     check_support_files_declared is satisfied and only the direct-execution
  #     rule can object.
  d="$(new_case f26)"
  exempt_victim="$(jq -r '
    [(.supportFiles // [])[] | select(.role == "release-tooling") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$exempt_victim" ]]; then
    fail "fixture 26: no release-tooling exemption exists to attack"
  else
    if fixture_mutate "fixture 26" "$d/$MANIFEST_REL" \
      ruby -rjson -e '
        path, victim = ARGV
        data = JSON.parse(File.read(path))
        data["supportFiles"].each { |entry| entry["role"] = "library" if entry["path"] == victim }
        File.write(path, JSON.pretty_generate(data) + "\n")
      ' "$d/$MANIFEST_REL" "$exempt_victim"; then
      expect_fail "fixture 26" "$d" check_workflow_execution_declared \
        "a directly executed script relabelled from release-tooling to library fails the role rule"
    fi
  fi

  # 27. The exemption is conditional on the trigger, so trip the condition rather
  #     than the declaration: add a ci.yml step that runs the release script. The
  #     entry stays exactly as it is and stays valid on its own terms; what
  #     changes is that the script now runs on every branch push and pull
  #     request, which is what the role denies. An exemption nobody has tried to
  #     trip is an exemption whose reach is a guess (§4.4 shape 8).
  d="$(new_case f27)"
  exempt_victim="$(jq -r '
    [(.supportFiles // [])[] | select(.role == "release-tooling") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$exempt_victim" ]]; then
    fail "fixture 27: no release-tooling exemption exists to attack"
  else
    if fixture_mutate "fixture 27" "$d/.github/workflows/ci.yml" \
      ruby -e '
        path, victim = ARGV
        text = File.read(path)
        step = "      - name: FR-147 release script on the development path\n" \
               "        run: ./#{victim} --dry-run\n\n"
        # No raise when the anchor is gone. An abort here would end the run on
        # set -e with the summary line unprinted, and a truncated run reads
        # exactly like a complete one. Writing nothing leaves the digest
        # unchanged, which is the state fixture_mutate turns into one named
        # failed assertion.
        text.sub!(/^      - name: Governance result$/) { "#{step}      - name: Governance result" }
        File.write(path, text)
      ' "$d/.github/workflows/ci.yml" "$exempt_victim"; then
      expect_fail "fixture 27" "$d" check_workflow_execution_declared \
        "a release-tooling script run by a branch-push workflow loses its exemption"
    fi
  fi

  # 28. `generator` is the second role permitting direct execution, and its whole
  #     discipline is one field. Deleting the entry, or relabelling it, are the
  #     shapes fixtures 25 and 26 already cover. The mutation an author is least
  #     likely to have in mind is the one that keeps everything looking correct:
  #     leave the role, leave a verifiedBy, and point it at a gate that is not
  #     ci-required. The entry still parses, still names a real script this
  #     manifest classifies, and still reads as verified — and nothing on any
  #     push proves the artifact any more. Downgrading the verifier is cheaper
  #     than deleting the field and is what a reclassification would do by
  #     accident.
  d="$(new_case f28)"
  gen_victim="$(jq -r '
    [(.supportFiles // [])[] | select(.role == "generator") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  manual_gate="$(jq -r '
    [.scripts[] | select(.enforcement == "manual-runbook") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$gen_victim" || -z "$manual_gate" ]]; then
    fail "fixture 28: no generator entry or no manual-runbook gate exists to build the case from"
  else
    if fixture_mutate "fixture 28" "$d/$MANIFEST_REL" \
      ruby -rjson -e '
        path, victim, replacement = ARGV
        data = JSON.parse(File.read(path))
        data["supportFiles"].each do |entry|
          entry["verifiedBy"] = replacement if entry["path"] == victim
        end
        File.write(path, JSON.pretty_generate(data) + "\n")
      ' "$d/$MANIFEST_REL" "$gen_victim" "$manual_gate"; then
      expect_fail "fixture 28" "$d" check_workflow_execution_declared \
        "a generator whose verifiedBy names a manual-runbook gate is unverified on every push"
    fi
  fi

  # 28b. The other direction, for the reason fixture 22b exists: "fails when I
  #      break it" and "fails whatever I do to it" have the same green record.
  #      Restoring a real ci-required verifier on the same entry must pass.
  d="$(new_case f28b)"
  ci_gate="$(jq -r '
    [.scripts[] | select(.enforcement == "ci-required") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$gen_victim" || -z "$ci_gate" ]]; then
    fail "fixture 28b: no generator entry or no ci-required gate exists to build the control from"
  else
    if fixture_mutate "fixture 28b" "$d/$MANIFEST_REL" \
      ruby -rjson -e '
        path, victim, replacement = ARGV
        data = JSON.parse(File.read(path))
        data["supportFiles"].each do |entry|
          entry["verifiedBy"] = replacement if entry["path"] == victim
        end
        File.write(path, JSON.pretty_generate(data) + "\n")
      ' "$d/$MANIFEST_REL" "$gen_victim" "$ci_gate"; then
      if check_workflow_execution_declared "$d" >/dev/null 2>&1; then
        pass "fixture 28b: a generator pointed at a different ci-required gate still passes, so 28 is about the enforcement and not about the edit"
      else
        fail "fixture 28b: rewriting verifiedBy to another ci-required gate was rejected; the check is reacting to the edit rather than to the enforcement"
      fi
    fi
  fi

  # 29. Arming is text, and text presence is §4.4 shape 1 — a commented-out call
  #     satisfies it. The mutation comments the call out rather than deleting it,
  #     because deletion is the case the author had in mind and commenting is
  #     what someone actually does while debugging a gate at 2am.
  d="$(new_case f29)"
  manual_victim="$(jq -r '
    [.scripts[] | select(.enforcement == "manual-runbook") | .path][0] // empty' \
    "$d/$MANIFEST_REL")"
  if [[ -z "$manual_victim" ]]; then
    fail "fixture 29: no manual-runbook gate exists to attack"
  else
    if fixture_mutate "fixture 29" "$d/$manual_victim" \
      ruby -e '
        path = ARGV[0]
        text = File.read(path)
        text.sub!(/^(gate_runlog_arm ".*"$)/) { "# #{Regexp.last_match(1)}" }
        File.write(path, text)
      ' "$d/$manual_victim"; then
      expect_fail "fixture 29" "$d" check_manual_gates_record_runs \
        "a manual-runbook gate whose arming is commented out is invisible to the freshness ledger"
    fi
  fi

  # 30. The likelier defect than either deletion or commenting: a gate armed
  #     under a neighbour's path, which is what copying a header block produces.
  #     Every structural condition still holds — the library is sourced, the
  #     function is called, the argument is a real manual-runbook gate — and the
  #     ledger is actively wrong in two entries rather than merely silent.
  d="$(new_case f30)"
  other_manual="$(jq -r --arg v "$manual_victim" '
    [.scripts[] | select(.enforcement == "manual-runbook")
      | select(.path != $v) | .path][0] // empty' "$d/$MANIFEST_REL")"
  if [[ -z "$manual_victim" || -z "$other_manual" ]]; then
    fail "fixture 30: fewer than two manual-runbook gates exist, so the mix-up cannot be built"
  else
    if fixture_mutate "fixture 30" "$d/$manual_victim" \
      ruby -e '
        path, impostor = ARGV
        text = File.read(path)
        text.sub!(/^gate_runlog_arm ".*"$/) { %(gate_runlog_arm "#{impostor}") }
        File.write(path, text)
      ' "$d/$manual_victim" "$other_manual"; then
      expect_fail "fixture 30" "$d" check_manual_gates_record_runs \
        "a gate armed under another gate's path fails, because both ledger entries would then lie"
    fi
  fi

  # 31. A new ci-required gate with no shape and no exemption. This is the rule
  #     working as intended rather than an exotic defect: the mutation is
  #     literally "add a gate", which is the act the rule exists to slow down.
  #     The state under test is "a ci-required gate carrying neither a shape nor
  #     an exemption", and the mutation reaches it by dropping one exemption
  #     rather than by inventing a gate. Inventing one was tried first and
  #     cannot isolate: a fabricated entry has no file (check 1), and giving it
  #     a file still leaves no ci.yml step executing it (check_wiring_truth), so
  #     the case would fail three checks and prove nothing about this one. The
  #     victim is a real, wired, executed gate; the only thing missing is the
  #     answer to what it catches, which is exactly the rule.
  d="$(new_case f31)"
  if fixture_mutate "fixture 31" "$d/$MANIFEST_REL" \
    ruby -rjson -e '
      path = ARGV[0]
      data = JSON.parse(File.read(path))
      data["shapeRationale"]["exemptions"].shift
      File.write(path, JSON.pretty_generate(data) + "\n")
    ' "$d/$MANIFEST_REL"; then
    expect_fail "fixture 31" "$d" check_new_gates_name_their_shape \
      "a ci-required gate with neither a shape nor an exemption fails"
  fi

  # 32. The exemption list outliving what it excuses. The mutation adds a path
  #     that is real, declared and on disk but is not a ci-required gate — the
  #     state a retired gate leaves behind. Reclassifying a live gate would have
  #     been the more literal mutation and it trips the stale-claim check as
  #     well, because documents describe that gate as CI-enforced; this form
  #     isolates the amnesty-by-attrition rule, which is the one that would
  #     otherwise accumulate silently until a future path inherited an exemption
  #     by colliding with a dead one.
  d="$(new_case f32)"
  if fixture_mutate "fixture 32" "$d/$MANIFEST_REL" \
    ruby -rjson -e '
      path = ARGV[0]
      data = JSON.parse(File.read(path))
      retired = data["scripts"]
        .find { |entry| entry["enforcement"] == "manual-runbook" }["path"]
      data["shapeRationale"]["exemptions"] << retired
      data["shapeRationale"]["exemptions"].sort!
      File.write(path, JSON.pretty_generate(data) + "\n")
    ' "$d/$MANIFEST_REL"; then
    expect_fail "fixture 32" "$d" check_new_gates_name_their_shape \
      "an exemption naming a path that is not a ci-required gate fails"
  fi

  # 33. A raw kill+wait on a daemon PID reappears in a migrated gate — the
  #     26th-site mutation check 16's condition A exists for. Appended live
  #     rather than inserted into the cleanup: the check's subject is the
  #     spelling's presence anywhere outside the library, so position must not
  #     matter. The diagnostic is asserted too (§4.4 shape 7): an exit code
  #     cannot distinguish condition A from condition B.
  d="$(new_case f33)"
  # The victim must actually be a daemon gate: fixture 35 unhooks its library
  # source line, which only exists in a gate that has one. Derived by content,
  # not by manifest position — the alphabetically first manual gate is not
  # necessarily a daemon gate, and a victim chosen by position would make
  # fixture 35 pass vacuously the day the ordering changes (§4.4 shape 7).
  # The jq read is a command substitution whose status is observed, not a
  # process-substitution feed nobody checks (§4.4 shape 5; the jq-status gate
  # flagged the first draft of this loop). A failed or empty read leaves no
  # victim and the fixture fails through its named else-branch.
  daemon_victim=""
  manual_paths="$(jq -r '.scripts[] | select(.enforcement == "manual-runbook") | .path' \
    "$d/$MANIFEST_REL")" || manual_paths=""
  while IFS= read -r candidate; do
    [[ -z "$candidate" || ! -f "$d/$candidate" ]] && continue
    if grep -qE '^[[:space:]]*\.[[:space:]].*scripts/lib/gate_daemon\.sh' "$d/$candidate"; then
      daemon_victim="$candidate"
      break
    fi
  done <<< "$manual_paths"
  if [[ -z "$daemon_victim" ]]; then
    fail "fixture 33: no manual-runbook gate sources gate_daemon.sh; nothing to attack"
  else
    # Assembled through %s so this file never contains the forbidden spelling
    # itself — check 16 scans every tracked script, including this one.
    printf 'kill "$%s" 2>/dev/null || true\nwait "$%s" 2>/dev/null || true\n' \
      DAEMON_PID DAEMON_PID >> "$d/$daemon_victim"
    expect_fail "fixture 33" "$d" check_daemon_teardown_shared \
      "a raw kill+wait on a daemon PID outside the library fails the teardown check"
    # Captured, not piped: the check exits 1 here by design, and under
    # pipefail `failing_check | grep -q` reports the check's status even when
    # grep matches — FR-145's shape, aimed at this fixture.
    f33_diag="$(check_daemon_teardown_shared "$d" 2>&1 >/dev/null || true)"
    if grep -q "$daemon_victim: signals or waits on a daemon PID directly" <<<"$f33_diag"; then
      pass "fixture 33: the diagnostic names the file and the offending shape"
    else
      fail "fixture 33: the check failed without naming the file through condition A"
    fi
  fi

  # 34. The same two lines, commented out — and the check must PASS. This is
  #     the mutation the implementation is least likely to catch: an absence
  #     condition that greps the raw file would flag prose and dead examples,
  #     and a check that cries wolf on a comment gets an exemption added, which
  #     is worse than no check. Comment-stripping is load-bearing; prove it.
  d="$(new_case f34)"
  if [[ -n "$daemon_victim" ]]; then
    printf '# kill "$%s" 2>/dev/null || true\n# wait "$%s" 2>/dev/null || true\n' \
      DAEMON_PID DAEMON_PID >> "$d/$daemon_victim"
    if check_daemon_teardown_shared "$d" >/dev/null 2>&1; then
      pass "fixture 34: a commented-out kill+wait is not a violation (comments are stripped)"
    else
      fail "fixture 34: the check flagged a commented-out kill+wait; it is reading raw text"
    fi
  else
    fail "fixture 34: no manual-runbook gate exists to attack"
  fi

  # 35. A gate assigns a daemon PID but never sources the library — condition
  #     B's subject, reached by unhooking a migrated gate rather than by
  #     inventing a file (a new file trips check 1 and cannot isolate). The
  #     stop calls are neutralised to `:` so condition A stays silent and the
  #     fixture isolates the pairing condition alone.
  d="$(new_case f35)"
  if [[ -n "$daemon_victim" ]]; then
    if fixture_mutate "fixture 35" "$d/$daemon_victim" \
      ruby -e '
        path = ARGV[0]
        text = File.read(path)
        text.gsub!(%r{^([[:space:]]*)\.[[:space:]].*scripts/lib/gate_daemon\.sh.*$}) { "#{Regexp.last_match(1)}: gate-daemon-source-removed" }
        text.gsub!("gate_daemon_stop", ": neutralised_stop")
        File.write(path, text)
      ' "$d/$daemon_victim"; then
      expect_fail "fixture 35" "$d" check_daemon_teardown_shared \
        "a gate that assigns a daemon PID without sourcing the library fails the pairing condition"
      f35_diag="$(check_daemon_teardown_shared "$d" 2>&1 >/dev/null || true)"
      if grep -q "$daemon_victim: assigns a daemon PID but never sources" <<<"$f35_diag"; then
        pass "fixture 35: the diagnostic names the file through condition B"
      else
        fail "fixture 35: the check failed without naming the missing source line"
      fi
    fi
  else
    fail "fixture 35: no manual-runbook gate exists to attack"
  fi

  # ── Behavioural: arming a gate that already cleans up records the run,
  #    keeps the cleanup, and preserves the exit status ──
  #
  # check_manual_gates_record_runs reads text and cannot see whether any of this
  # happens. The risk it cannot cover is specific and was measured before the
  # library was written: 30 of the 35 gates run `trap cleanup EXIT`, and a second
  # bare `trap ... EXIT` discards the first silently — which in these scripts is
  # a leaked daemon on a bound port or a leaked data directory. So run a gate
  # shaped like the real ones, under the real library, and assert all three
  # facts at once. Any one of them alone would pass on a broken composition:
  # the record alone is satisfied by clobbering the cleanup, the cleanup alone by
  # never arming, and the exit status alone by doing neither.
  echo ""
  echo "Behavioural: freshness recording composes with an existing EXIT trap"
  behave="$FIXTURE_ROOT/runlog"
  mkdir -p "$behave/scripts/lib" "$behave/scripts/qa" "$behave/config/governance"
  cp "$REPO_ROOT/scripts/lib/gate_runlog.sh" "$behave/scripts/lib/"
  (cd "$behave" && git init -q . &&
    git -c user.email=qa@local -c user.name=qa commit -q --allow-empty -m base) >/dev/null 2>&1
  cat > "$behave/config/governance/manual-gate-freshness.json" <<'JSON'
{
  "version": 1,
  "staleAfterDays": 90,
  "gates": { "scripts/qa/probe.sh": { "owner": "docs/x.md", "lastRun": null } }
}
JSON
  # The probe fails on purpose. A gate that exits 0 cannot distinguish "the
  # status was recorded" from "zero was recorded because zero is the default".
  cat > "$behave/scripts/qa/probe.sh" <<'PROBE'
#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(git rev-parse --show-toplevel)"
. "$REPO_ROOT/scripts/lib/gate_runlog.sh"
cleanup() { echo ran > "$REPO_ROOT/cleanup.marker"; }
trap cleanup EXIT
gate_runlog_arm "scripts/qa/probe.sh"
exit 7
PROBE
  chmod +x "$behave/scripts/qa/probe.sh"
  # The probe exits 7 by design, so `set -e` would end the whole run here and
  # the summary line would never print — a truncated run reads exactly like a
  # complete one (§4.4 shape 7). Disabled around the call for the same reason
  # run_gate does it, and the status is read straight from $? rather than
  # through a pipe.
  set +e
  (cd "$behave" && bash scripts/qa/probe.sh) >/dev/null 2>&1
  behave_status=$?
  set -e
  behave_recorded="$(ruby -rjson -e '
    data = JSON.parse(File.read(ARGV[0]))
    entry = data["gates"]["scripts/qa/probe.sh"]["lastRun"]
    print entry ? entry["exitStatus"] : "none"
  ' "$behave/config/governance/manual-gate-freshness.json" 2>/dev/null)"
  if [[ "$behave_status" -eq 7 && "$behave_recorded" == "7" && -f "$behave/cleanup.marker" ]]; then
    pass "arming records the true exit status, runs the gate's own cleanup, and leaves the gate's status unchanged"
  else
    fail "freshness arming broke the gate's contract (exit $behave_status, recorded '$behave_recorded', cleanup marker $([[ -f "$behave/cleanup.marker" ]] && echo present || echo MISSING))"
  fi
  echo ""

  # ── Behavioural: the aggregated outcomes really decide the job ──
  #
  # check_continue_on_error_aggregated reads structure. It proves each swallowed
  # step's outcome is referenced, and "referenced" is not "load-bearing": an
  # aggregate that printed the table and exited 0 would satisfy it completely
  # while every gate in the job became decoration. So take the real aggregate
  # script out of ci.yml and run it against outcomes it has never seen.
  #
  # The empty-outcome case is here because FR-137 asserted the opposite of what
  # it does. The FR argued a renamed step leaves a record that "resolves to
  # empty forever, with the same effect as the omission". Measured, it is the
  # reverse: the loop finds an outcome that is neither success nor skipped and
  # fails the job, permanently and loudly. The rule survived the correction;
  # its stated reason did not, and this assertion is what pins down which.
  #
  # The extraction names a step by its display text. That is an enumerated
  # target in a file this gate does not own: rename the step and the abort
  # fires, and unwrapped it ended the whole fixture run — thirty-odd assertions
  # that never reported, on a gate whose subject is enforcement that exists and
  # does not execute. This function is the generalisation of inject() from a few
  # hundred lines below, so this file being on the list was not an accident of
  # scope (FR-143).
  AGGREGATE="$FIXTURE_ROOT/aggregate.sh"
  if fixture_produce "aggregate extraction" "$AGGREGATE" \
    ruby -r"$REPO_ROOT/scripts/lib/workflow_model" -e '
    step = WorkflowModel.steps(ARGV[0], "governance")
      .find { |candidate| candidate["name"] == "Governance result" }
    abort("no aggregate step named Governance result") unless step && step["run"]
    File.write(ARGV[1], step["run"])
  ' "$BASE/.github/workflows/ci.yml" "$AGGREGATE"; then

    run_aggregate() {
      OUTCOMES="$1" bash "$AGGREGATE" > "$FIXTURE_ROOT/aggregate.log" 2>&1
    }

    if run_aggregate "$(printf 'liveness=success\nsurface=skipped')"; then
      pass "behavioural: the aggregate passes a run whose outcomes are all success or skipped"
    else
      fail "behavioural: the aggregate rejected a run in which every gate passed"
      cat "$FIXTURE_ROOT/aggregate.log" >&2
    fi

    if run_aggregate "$(printf 'liveness=success\nsurface=failure')"; then
      fail "behavioural: the aggregate passed a run in which a gate reported failure"
      cat "$FIXTURE_ROOT/aggregate.log" >&2
    elif grep -q '^surface  *failure$' "$FIXTURE_ROOT/aggregate.log"; then
      pass "behavioural: one failed outcome fails the job, and the aggregate names which gate"
    else
      fail "behavioural: the aggregate failed the job without naming the gate that failed"
      cat "$FIXTURE_ROOT/aggregate.log" >&2
    fi

    if run_aggregate "$(printf 'liveness=success\nfr137-ghost=')"; then
      fail "behavioural: an outcome that resolved to nothing was counted as a pass"
      cat "$FIXTURE_ROOT/aggregate.log" >&2
    else
      pass "behavioural: a dangling reference's empty outcome fails the job rather than passing quietly"
    fi
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
  REGISTERED="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort -u)"
  if [[ "$DEFINED" == "$REGISTERED" ]]; then
    pass "meta: ALL_CHECKS names every check the file defines"
  else
    fail "meta: ALL_CHECKS and the defined checks differ"
    comm -23 <(printf '%s\n' "$DEFINED") <(printf '%s\n' "$REGISTERED") \
      | sed 's/^/      defined but never run: /' >&2
    comm -13 <(printf '%s\n' "$DEFINED") <(printf '%s\n' "$REGISTERED") \
      | sed 's/^/      registered but not defined: /' >&2
  fi

  # Registered is not the same as run. check_git_history_available was defined
  # and registered and never called by verification mode, because adding it to
  # ALL_CHECKS and adding a call were separate edits. That is enforcement which
  # exists and does not execute — this script's own subject, one level up.
  # Verification now iterates ALL_CHECKS, and this holds the description table
  # to it so a check cannot be registered without a line a reader can see.
  UNDESCRIBED=""
  for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
    describe_check "$check" >/dev/null 2>&1 || UNDESCRIBED+="$check"$'\n'
  done
  if [[ -z "$UNDESCRIBED" ]]; then
    pass "meta: every registered check is run and named by verification mode"
  else
    fail "meta: a registered check has no description, so verification cannot report it"
    printf '      %s\n' $UNDESCRIBED >&2
  fi

  UNTESTED="$(comm -23 <(printf '%s\n' "$REGISTERED") \
                       <(printf '%s\n' ${TARGETED[@]+"${TARGETED[@]}"} | LC_ALL=C sort -u))"
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
#
# Driven from ALL_CHECKS rather than a hand-written list of calls. The two forms
# drift: check_git_history_available was defined, registered, and silently never
# run here, because adding it to the registry and adding a call are separate
# edits and only one of them is anybody's habit. That is the same shape as a
# gate wired into no job — enforcement that exists and does not execute — one
# level up, inside the script that exists to catch it.
#
# A check with no description is a failure, not a default message: the summary
# line is what a reader sees, and an unnamed check reads as noise.

echo "=== FR-127: QA gate enforcement surface ==="
echo ""

for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
  if ! description="$(describe_check "$check")"; then
    fail "$check is registered but has no description; add one beside the check"
    continue
  fi
  if "$check" "$REPO_ROOT"; then
    pass "${description%%|*}"
  else
    fail "${description##*|}"
  fi
done

echo ""
CI_COUNT="$(jq '[.scripts[] | select(.enforcement == "ci-required")] | length' "$REPO_ROOT/$MANIFEST_REL")"
TOTAL="$(jq '.scripts | length' "$REPO_ROOT/$MANIFEST_REL")"
echo "Enforcement surface: $CI_COUNT of $TOTAL gates are ci-required"
echo "FR-127 gate surface: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
