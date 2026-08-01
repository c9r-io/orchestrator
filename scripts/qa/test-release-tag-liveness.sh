#!/usr/bin/env bash
#
# FR-151: release tag liveness — the newest release tag must have produced a
# release, or at least a release.yml run.
#
# v0.3.1 was a phantom: the tag reached the remote inside a multi-tag push
# (GitHub creates no events for a push carrying more than three tags), so
# release.yml never ran, and for four months every surface downstream of the
# tag — GitHub Releases, crates.io, the Homebrew tap — silently stayed at
# 0.3.0 while the repository believed 0.3.1 had shipped. Nothing was watching
# the gap between "a tag exists" and "the release pipeline saw it"; DD-161
# recorded that class as deliberately unbuilt. This gate builds it.
#
# The assertion observes the fact, not a proxy (§4.4): for the highest
# semver-ordered v* tag on the remote, either a GitHub Release with that tag
# exists (the pipeline's terminal artifact), or a release.yml run for that
# tag ref exists (covers the in-flight window right after a push). A tag with
# neither is the phantom signature and fails the gate, naming the tag.
#
# Failure modes are closed (§4.4 shape 5): an API call that dies or returns
# an empty tag set is a failed assertion with a diagnostic, never a skip —
# zero readable tags and a healthy history are not the same colour.
#
# Historical scope: exactly one tag, v0.3.1, is exempt — the recorded phantom
# this FR reconciles (its changes shipped in 0.4.0; see DD-162). A closed,
# dated, one-element set: it names a tag that already exists and can never
# absorb a future instance, which is what distinguishes it from a §4.4
# shape 2/8 exemption.
#
# Usage:
#   test-release-tag-liveness.sh                 verify the real repository
#   test-release-tag-liveness.sh --fixture-test  prove the check fails on the phantom signature

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

command -v gh >/dev/null 2>&1 || {
  echo "missing required command: gh" >&2
  exit 1
}

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# The one recorded phantom, reconciled by FR-151/DD-162. Never extend this
# list — a second entry means a second phantom, and the fix is upstream
# (single-tag pushes), not a wider exemption.
PHANTOM_EXEMPT="v0.3.1"

repo_slug() {
  if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    echo "$GITHUB_REPOSITORY"
    return 0
  fi
  local url
  url="$(git -C "$REPO_ROOT" remote get-url origin)" || return 1
  # git@github.com:owner/repo.git | https://github.com/owner/repo(.git)
  echo "$url" | sed -E 's#^(git@github\.com:|https://github\.com/)##; s#\.git$##'
}

check_liveness() {
  local repo tags latest releases_ok runs
  if ! repo="$(repo_slug)" || [[ -z "$repo" ]]; then
    fail "cannot determine repository slug (no GITHUB_REPOSITORY, origin unreadable)"
    return
  fi

  # Remote tags, not local: the phantom lives on the remote, and a local
  # clone missing the tag would otherwise pass vacuously.
  if ! tags="$(gh api "repos/${repo}/git/matching-refs/tags/v" --jq '.[].ref' 2>&1)"; then
    fail "tag enumeration failed (fail closed, not skip): ${tags}"
    return
  fi
  latest="$(printf '%s\n' "$tags" | sed -n 's#^refs/tags/\(v[0-9][^^]*\)$#\1#p' | sort -V | tail -1)"
  if [[ -z "$latest" ]]; then
    fail "no v* tags readable on ${repo} — empty read fails closed"
    return
  fi

  if [[ "$latest" == "$PHANTOM_EXEMPT" ]]; then
    pass "latest tag ${latest} is the recorded phantom (DD-162), exempt until 0.4.0 ships"
    return
  fi

  if gh release view "$latest" --repo "$repo" --json tagName >/dev/null 2>&1; then
    releases_ok=1
  else
    releases_ok=0
  fi
  if [[ "$releases_ok" == "1" ]]; then
    pass "latest tag ${latest} has a GitHub Release"
    return
  fi

  # No release yet: tolerate the in-flight window iff the pipeline at least
  # saw the tag. The run's own status is not asserted here — a failed run is
  # visible red in the Actions tab; the phantom's defining property is that
  # nothing at all was triggered.
  if ! runs="$(gh run list --workflow=release.yml --repo "$repo" --branch "$latest" --json databaseId --jq 'length' 2>&1)"; then
    fail "run enumeration for ${latest} failed (fail closed, not skip): ${runs}"
    return
  fi
  if [[ "$runs" =~ ^[0-9]+$ ]] && [[ "$runs" -ge 1 ]]; then
    pass "latest tag ${latest} has no Release yet but release.yml has seen it (${runs} run(s))"
  else
    fail "phantom release tag: ${latest} has no GitHub Release and no release.yml run — the tag reached the remote but the pipeline never saw it (v0.3.1 signature; push tags one at a time and verify a run starts)"
  fi
}

run_fixture_test() {
  local stub_dir out status
  stub_dir="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$stub_dir'" RETURN

  cat > "$stub_dir/gh" <<'STUB'
#!/usr/bin/env bash
# Fixture stub: simulates the GitHub API per GATE_FIXTURE_SCENARIO.
case "${GATE_FIXTURE_SCENARIO}:${1}" in
  phantom:api)
    printf 'refs/tags/v9.9.9\n'
    ;;
  phantom:release)
    echo "release not found" >&2
    exit 1
    ;;
  phantom:run)
    echo "0"
    ;;
  api-down:api)
    echo "simulated api outage" >&2
    exit 1
    ;;
  healthy:api)
    printf 'refs/tags/v9.9.8\n'
    ;;
  healthy:release)
    echo '{"tagName":"v9.9.8"}'
    ;;
  *)
    echo "gh stub: unhandled ${GATE_FIXTURE_SCENARIO}:${1}" >&2
    exit 97
    ;;
esac
STUB
  chmod +x "$stub_dir/gh"

  # Fixture 1: the phantom signature — a tag with no release and no run must
  # fail, and the diagnostic must name the tag (§4.4 shape 7: assert the
  # diagnostic, not the exit code alone).
  status=0
  out="$(GATE_FIXTURE_SCENARIO=phantom GITHUB_REPOSITORY=example/fixture \
        PATH="$stub_dir:$PATH" bash "${BASH_SOURCE[0]}" 2>&1)" || status=$?
  if [[ "$status" -ne 0 ]] && grep -q "phantom release tag: v9.9.9" <<< "$out"; then
    pass "fixture: phantom signature fails and the diagnostic names v9.9.9"
  else
    fail "fixture: phantom signature not detected (status=${status}); output: ${out}"
  fi

  # Fixture 2: an unreadable API must fail closed, not skip.
  status=0
  out="$(GATE_FIXTURE_SCENARIO=api-down GITHUB_REPOSITORY=example/fixture \
        PATH="$stub_dir:$PATH" bash "${BASH_SOURCE[0]}" 2>&1)" || status=$?
  if [[ "$status" -ne 0 ]] && grep -q "fail closed" <<< "$out"; then
    pass "fixture: API outage fails closed with a diagnostic"
  else
    fail "fixture: API outage did not fail closed (status=${status}); output: ${out}"
  fi

  # Fixture 3 (positive control): a tag with a release passes — proves the
  # two red fixtures fail through the checks, not through a broken harness.
  status=0
  out="$(GATE_FIXTURE_SCENARIO=healthy GITHUB_REPOSITORY=example/fixture \
        PATH="$stub_dir:$PATH" bash "${BASH_SOURCE[0]}" 2>&1)" || status=$?
  if [[ "$status" -eq 0 ]] && grep -q "v9.9.8 has a GitHub Release" <<< "$out"; then
    pass "fixture: healthy tag passes (positive control)"
  else
    fail "fixture: positive control failed (status=${status}); output: ${out}"
  fi
}

if [[ "${1:-}" == "--fixture-test" ]]; then
  run_fixture_test
else
  check_liveness
fi

echo ""
echo "${PASS} passed, ${FAIL} failed"
[[ "$FAIL" -eq 0 ]]
