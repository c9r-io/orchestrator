#!/usr/bin/env bash
#
# FR-134 requirement 8: environment equivalence.
#
# test-governance-ledger-tooling.sh passed 8/8 on every developer machine and
# had never once succeeded in the job it was wired into. Its second case
# verifies that `--write` refuses under CI; its third case then called `--write`,
# was refused by the mechanism the case above had just confirmed, and died at
# `set -e`. The gate's own positive path was mutually exclusive with its own
# safety mechanism, and only in the environment where it actually ran.
#
# Nothing structural can see that. The gate is wired, its dependencies are
# present, its assertions are sound — and it is dead. What distinguishes the two
# worlds is one environment variable, so this runs each gate in both worlds and
# requires the same answer.
#
# Scope is derived, not listed: every ci-required gate whose declared
# dependencies do not include cargo. The cargo-bearing gates are excluded on
# cost, not principle — a second full workspace build per gate is not worth it,
# and those gates already run under CI in the real job, which is the same
# observation this makes. That boundary is a real limit, written down here
# rather than left for someone to discover.
#
# Usage:
#   test-ci-environment-parity.sh                 verify every in-scope gate
#   test-ci-environment-parity.sh --fixture-test  prove the comparison can fail

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
MANIFEST_REL="config/governance/qa-gate-surface.json"

# shellcheck source=../lib/gate_preamble.sh
. "$REPO_ROOT/scripts/lib/gate_preamble.sh"

for required in jq ruby; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "missing required command: $required" >&2
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

# Second condition on the recursion, because excluding this file by path only
# closes the cycle it can see. If some other in-scope gate ever invokes this one,
# the path check is blind and the job hangs again — which is the failure mode
# that matters here, since a hang produces no output to diagnose from.
if [[ -n "${FR134_PARITY_RUNNING:-}" ]]; then
  echo "refusing to recurse: this gate is already running in a parent process" >&2
  echo "a gate that runs every ci-required gate must not be one of them" >&2
  exit 3
fi
export FR134_PARITY_RUNNING=1

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr134-env-parity.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# Every variable CiEnv treats as "no human is watching". Cleared together,
# because clearing only CI would leave a runner that exports GITHUB_ACTIONS
# looking interactive and the comparison would prove nothing.
CI_VARS=(CI CONTINUOUS_INTEGRATION GITHUB_ACTIONS GITLAB_CI BUILDKITE CIRCLECI
         TEAMCITY_VERSION BUILD_NUMBER)

clear_ci_env() {
  local unsets=() name
  for name in "${CI_VARS[@]}"; do unsets+=(-u "$name"); done
  env ${unsets[@]+"${unsets[@]}"} "$@"
}

set_ci_env() {
  local unsets=() name
  for name in "${CI_VARS[@]}"; do unsets+=(-u "$name"); done
  env ${unsets[@]+"${unsets[@]}"} CI=1 GITHUB_ACTIONS=true "$@"
}

# The gates to compare: ci-required, on disk, and not paying for a workspace
# build. Derived from the manifest and the gate's own preamble rather than
# listed, so a new gate is in scope the day it lands.
SELF_REL="scripts/qa/$(basename "${BASH_SOURCE[0]}")"

in_scope_gates() {
  local root="$1" path
  while read -r path; do
    [[ -z "$path" ]] && continue
    [[ -f "$root/$path" ]] || continue
    # This script is a ci-required gate with no cargo dependency, so it selects
    # itself, runs itself, and recurses until the job times out. It did: the
    # first CI run of this job sat at 52 minutes before anyone looked. Derived
    # from BASH_SOURCE rather than written as a literal, so renaming the file
    # cannot quietly reopen it.
    [[ "$path" == "$SELF_REL" ]] && continue
    # Declared dependency, not textual mention. "Contains the word cargo"
    # excluded test-qa-gate-surface.sh, which names cargo inside a regular
    # expression and never runs it — the same substitution of text for
    # execution this FR exists to remove.
    gate_requires "$root/$path" cargo && continue
    echo "$path"
  done < <(jq -r '.scripts[] | select(.enforcement == "ci-required") | .path' "$root/$MANIFEST_REL")
}

# Runs one gate in both worlds and reports whether they agree. Returns 0 when
# the exit codes match, whatever those codes are: this asserts equivalence, not
# success. A gate that fails identically in both is a different problem, and one
# this check has no business hiding.
compare_environments() {
  local root="$1" path="$2" interpreter=()
  [[ "$path" == *.rb ]] && interpreter=(ruby)
  [[ "$path" == *.sh ]] && interpreter=(bash)

  local without with
  clear_ci_env ${interpreter[@]+"${interpreter[@]}"} "$root/$path" \
    > "$WORK/without.log" 2>&1 && without=0 || without=$?
  set_ci_env ${interpreter[@]+"${interpreter[@]}"} "$root/$path" \
    > "$WORK/with.log" 2>&1 && with=0 || with=$?

  if [[ "$without" -eq "$with" ]]; then
    return 0
  fi
  echo "    $path: exit $without with CI cleared, exit $with with CI set" >&2
  echo "    --- output under CI ---" >&2
  tail -15 "$WORK/with.log" >&2
  return 1
}

check_environment_parity() {
  local root="$1" rc=0 path
  while read -r path; do
    [[ -z "$path" ]] && continue
    compare_environments "$root" "$path" || rc=1
  done < <(in_scope_gates "$root")
  return $rc
}

ALL_CHECKS=(check_environment_parity)

# ── Fixture mode ────────────────────────────────────────────────────────────────

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo "=== FR-134: CI environment parity (negative fixtures) ==="
  echo ""

  FIXTURE_ROOT="$(mktemp -d)"
  cleanup_fixtures() { rm -rf "$FIXTURE_ROOT"; cleanup; }
  trap cleanup_fixtures EXIT

  # A gate that behaves identically either way, and one that does not. Running
  # the real corpus here would cost minutes to prove a property of the
  # comparison rather than of the gates.
  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE/scripts/qa" "$BASE/config/governance"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$BASE/scripts/qa/test-stable.sh"
  chmod 755 "$BASE/scripts/qa/test-stable.sh"
  cat > "$BASE/$MANIFEST_REL" <<'JSON'
{
  "scripts": [
    { "path": "scripts/qa/test-stable.sh", "enforcement": "ci-required" }
  ]
}
JSON

  if check_environment_parity "$BASE" >/dev/null 2>&1; then
    pass "positive control: a gate indifferent to CI passes the comparison"
  else
    fail "positive control: a gate indifferent to CI was reported as differing"
  fi

  # The reproduction, reduced: a gate that dies only when CI is set. This is
  # test-governance-ledger-tooling.sh's shape before the fix.
  d="$FIXTURE_ROOT/self-lock"
  cp -R "$BASE" "$d"
  cat > "$d/scripts/qa/test-stable.sh" <<'GATE'
#!/usr/bin/env bash
set -euo pipefail
if [[ -n "${CI:-}" ]]; then
  echo "refusing under CI" >&2
  exit 2
fi
exit 0
GATE
  chmod 755 "$d/scripts/qa/test-stable.sh"
  if check_environment_parity "$d" >/dev/null 2>&1; then
    fail "a gate that exits 2 only under CI was not reported"
  else
    pass "a gate that exits 2 only under CI is reported, which no structural check can see"
  fi

  # A gate that fails in both worlds is not an environment difference, and
  # reporting it here would make this check a duplicate of every other one.
  d="$FIXTURE_ROOT/always-red"
  cp -R "$BASE" "$d"
  printf '#!/usr/bin/env bash\nexit 1\n' > "$d/scripts/qa/test-stable.sh"
  chmod 755 "$d/scripts/qa/test-stable.sh"
  if check_environment_parity "$d" >/dev/null 2>&1; then
    pass "a gate that fails identically in both environments is not an environment difference"
  else
    fail "a uniformly failing gate was misreported as an environment difference"
  fi

  # This gate must never select itself. It did, and the job ran for 52 minutes
  # before the recursion was noticed — a hang leaves no failure output to read,
  # so nothing about it looks like a defect until someone checks the clock.
  d="$FIXTURE_ROOT/self-selection"
  cp -R "$BASE" "$d"
  cat > "$d/$MANIFEST_REL" <<'JSON'
{
  "scripts": [
    { "path": "scripts/qa/test-stable.sh", "enforcement": "ci-required" },
    { "path": "scripts/qa/test-ci-environment-parity.sh", "enforcement": "ci-required" }
  ]
}
JSON
  cp "${BASH_SOURCE[0]}" "$d/scripts/qa/test-ci-environment-parity.sh"
  if grep -q 'test-ci-environment-parity' <<< "$(in_scope_gates "$d")"; then
    fail "this gate selected itself; running it would recurse until the job times out"
  else
    pass "this gate excludes itself from the set it runs"
  fi

  # And the independent guard, for a cycle the path check cannot see.
  if FR134_PARITY_RUNNING=1 bash "${BASH_SOURCE[0]}" >/dev/null 2>&1; then
    fail "a nested invocation was allowed to proceed"
  else
    pass "a nested invocation refuses rather than recursing"
  fi

  # Meta, as elsewhere: the registry has to name every check the file defines.
  DEFINED="$(grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()$//' | LC_ALL=C sort -u)"
  REGISTERED="$(printf '%s\n' "${ALL_CHECKS[@]}" | LC_ALL=C sort -u)"
  if [[ "$DEFINED" == "$REGISTERED" ]]; then
    pass "meta: ALL_CHECKS names every check the file defines"
  else
    fail "meta: ALL_CHECKS and the defined checks differ"
  fi

  echo ""
  echo "=== fixtures: $PASS passed, $FAIL failed ==="
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

# ── Verification mode ───────────────────────────────────────────────────────────

echo "=== FR-134: CI environment parity ==="
echo ""

GATES="$(in_scope_gates "$REPO_ROOT" | tr '\n' ' ')"
echo "  comparing: $GATES"
echo ""

if check_environment_parity "$REPO_ROOT"; then
  pass "every in-scope ci-required gate exits identically with and without CI set"
else
  fail "a ci-required gate behaves differently in the environment it actually runs in"
fi

echo ""
echo "=== CI environment parity: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
