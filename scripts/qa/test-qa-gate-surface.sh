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

for command in jq rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

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
check_surface_complete() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL"
  local disk declared missing_from_manifest missing_from_disk
  disk="$(cd "$root" && ls scripts/qa/*.sh scripts/qa/*.rb 2>/dev/null | sort)"
  declared="$(jq -r '.scripts[].path' "$manifest" | sort)"

  missing_from_manifest="$(comm -23 <(printf '%s\n' "$disk") <(printf '%s\n' "$declared"))"
  if [[ -n "$missing_from_manifest" ]]; then
    echo "    unclassified script(s) on disk:" >&2
    printf '      %s\n' $missing_from_manifest >&2
    return 1
  fi

  missing_from_disk="$(comm -13 <(printf '%s\n' "$disk") <(printf '%s\n' "$declared"))"
  if [[ -n "$missing_from_disk" ]]; then
    echo "    manifest entries with no script on disk:" >&2
    printf '      %s\n' $missing_from_disk >&2
    return 1
  fi
  return 0
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

# Extract one job block from a workflow file: from `  <job>:` to the next
# top-level job key at the same indentation.
workflow_job_block() {
  local workflow_file="$1" job="$2"
  awk -v job="  ${job}:" '
    $0 == job { inblock = 1; print; next }
    inblock && /^  [A-Za-z0-9_-]+:/ { inblock = 0 }
    inblock { print }
  ' "$workflow_file"
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
    block="$(workflow_job_block "$root/$workflow" "$job")"
    if [[ -z "$block" ]]; then
      echo "    $path: declared job '$job' not found in $workflow" >&2
      rc=1
      continue
    fi
    if [[ "$invoked_by" == "null" ]]; then
      if ! printf '%s\n' "$block" | grep -qF "$path"; then
        echo "    $path: not referenced by job '$job' in $workflow" >&2
        rc=1
      fi
    else
      if [[ ! -f "$root/$invoked_by" ]]; then
        echo "    $path: declared invoker does not exist: $invoked_by" >&2
        rc=1
        continue
      fi
      if ! printf '%s\n' "$block" | grep -qF "$invoked_by"; then
        echo "    $path: invoker $invoked_by is not referenced by job '$job' in $workflow" >&2
        rc=1
      fi
      if ! grep -qF "$path" "$root/$invoked_by"; then
        echo "    $path: declared invoker $invoked_by does not reference it" >&2
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
bundle_has_unpinned_provider() {
  local bundle="$1"
  [[ -f "$bundle" ]] || return 1
  grep -Eq '^[[:space:]]*provider:[[:space:]]*"?(claude|codex)' "$bundle" || return 1
  # Pinned only when every provider declaration is matched by a fake binary override.
  local providers pins
  providers="$(grep -Ec '^[[:space:]]*provider:[[:space:]]*"?(claude|codex)' "$bundle")"
  pins="$(grep -Ec '^[[:space:]]*binary:[[:space:]]*"?fake-' "$bundle" || true)"
  [[ "$pins" -lt "$providers" ]]
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
          echo "    $path: $evidence declares a claude/codex agent without a binary: fake-* pin" >&2
          rc=1
        fi
        ;;
      path-shadow)
        if ! grep -Eq 'cp .*"\$QA_ROOT/bin/(claude|codex)"' "$root/$path"; then
          echo "    $path: path-shadow isolation requires copying a fake provider into \$QA_ROOT/bin" >&2
          rc=1
        fi
        if ! grep -Eq 'export PATH="\$QA_ROOT/bin:\$PATH"' "$root/$path"; then
          echo "    $path: path-shadow isolation requires exporting \$QA_ROOT/bin ahead of PATH" >&2
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
CI_CLAIM_PATTERN='release gate|由 CI|CI 门禁|\.github/workflows|GitHub Actions|in CI|CI job'
check_no_stale_claims() {
  local root="$1"
  local manifest="$root/$MANIFEST_REL" rc=0 path base hits
  while read -r path; do
    [[ -z "$path" ]] && continue
    base="$(basename "$path")"
    hits="$(cd "$root" && rg -n --no-heading -g '*.md' -F "$base" docs .claude/skills 2>/dev/null \
      | rg -P "$CI_CLAIM_PATTERN" || true)"
    if [[ -n "$hits" ]]; then
      echo "    $path is not ci-required but is documented as CI-enforced:" >&2
      printf '      %s\n' "$hits" >&2
      rc=1
    fi
  done < <(jq -r '.scripts[] | select(.enforcement != "ci-required") | .path' "$manifest")
  return $rc
}

run_all_checks() {
  local root="$1"
  check_surface_complete "$root" || return 1
  check_reason_and_owner "$root" || return 1
  check_wiring_truth "$root" || return 1
  check_provider_isolation "$root" || return 1
  check_no_stale_claims "$root" || return 1
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
  (cd "$REPO_ROOT" && tar cf - \
    config/governance/qa-gate-surface.json \
    scripts/qa-doc-lint.sh \
    scripts/qa \
    fixtures/manifests/bundles \
    .github/workflows \
    docs \
    .claude/skills) | (cd "$BASE" && tar xf -)

  new_case() {
    local name="$1"
    local dir="$FIXTURE_ROOT/$name"
    cp -R "$BASE" "$dir"
    echo "$dir"
  }

  # A fixture must fail the check it targets, and must fail that check for its own
  # reason rather than by tripping an earlier one. So assert both: the named check
  # rejects the defect, and every other check still passes on the same tree.
  ALL_CHECKS=(check_surface_complete check_reason_and_owner check_wiring_truth
              check_provider_isolation check_no_stale_claims)

  expect_fail() {
    local name="$1"
    local dir="$2"
    local target="$3"
    local why="$4"
    local other

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
  d="$(new_case f4)"
  jq '(.scripts[] | select(.path == "scripts/qa/test-coordination-governance.sh") | .job) = "clippy"' \
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

  echo ""
  echo "FR-127 gate surface fixtures: $PASS passed, $FAIL failed"
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

# ── Verification mode ───────────────────────────────────────────────────────────

echo "=== FR-127: QA gate enforcement surface ==="
echo ""

if check_surface_complete "$REPO_ROOT"; then
  pass "every scripts/qa gate is classified and every classified gate exists on disk"
else
  fail "manifest and scripts/qa disagree"
fi

if check_reason_and_owner "$REPO_ROOT"; then
  pass "every non-ci-required gate declares a reason and an owner document that exists"
else
  fail "a non-ci-required gate is missing its reason or owner document"
fi

if check_wiring_truth "$REPO_ROOT"; then
  pass "every ci-required gate is referenced by the workflow job it declares"
else
  fail "a ci-required gate is not actually wired into its declared workflow job"
fi

if check_provider_isolation "$REPO_ROOT"; then
  pass "every ci-required gate has an asserted provider isolation mechanism"
else
  fail "a ci-required gate can reach an unpinned provider binary"
fi

if check_no_stale_claims "$REPO_ROOT"; then
  pass "no document claims CI or release-gate enforcement for a gate that has none"
else
  fail "a document claims CI enforcement that does not exist"
fi

echo ""
CI_COUNT="$(jq '[.scripts[] | select(.enforcement == "ci-required")] | length' "$REPO_ROOT/$MANIFEST_REL")"
TOTAL="$(jq '.scripts | length' "$REPO_ROOT/$MANIFEST_REL")"
echo "Enforcement surface: $CI_COUNT of $TOTAL gates are ci-required"
echo "FR-127 gate surface: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
