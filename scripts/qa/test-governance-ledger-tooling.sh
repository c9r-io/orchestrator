#!/usr/bin/env bash
#
# FR-128: governance ledger regeneration and review tooling.
#
# config/governance/coordination-collapse-ledger.json is compared by strict
# equality, so any production Agent spec change turns the gate red. The friction
# is deliberate — every change should pass review — but friction without tooling
# produces rubber-stamping rather than review. These cases verify that the
# regeneration modes make a correct update cheap without making an unreviewed
# one possible.
#
# Every case runs against a throwaway git repository built from the tracked
# files; the working tree is never modified.
#
# Usage:
#   test-governance-ledger-tooling.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/coordination-governance.rb"
LEDGER="config/governance/coordination-collapse-ledger.json"
# The gate requires its shared source scanner (FR-130 / DD-142). A case repo is
# built from `git ls-files`, so any working-tree-only dependency has to be named
# here or the gate under test dies on a missing require before asserting
# anything.
GATE_LIB="scripts/lib/rust_source.rb"

for command in ruby jq git; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr128-ledger-tooling.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

digest() { ruby -rdigest -e 'print Digest::SHA256.file(ARGV[0]).hexdigest' "$1"; }

# A case runs in its own repository so that `git show HEAD:` — which the mismatch
# report uses to recover the reviewed spec — resolves to the state under test
# rather than to whatever the developer happens to have checked out.
new_case() {
  local name
  name="$1"
  local dir
  dir="$WORK/$name"
  mkdir -p "$dir"
  tar -cf - -C "$REPO_ROOT" $(cd "$REPO_ROOT" && git ls-files) | tar -xf - -C "$dir"
  cp "$REPO_ROOT/$LEDGER" "$dir/$LEDGER"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  mkdir -p "$dir/$(dirname "$GATE_LIB")"
  cp "$REPO_ROOT/$GATE_LIB" "$dir/$GATE_LIB"
  git -C "$dir" init -q .
  git -C "$dir" add -A
  git -C "$dir" -c user.email=qa@local -c user.name=qa commit -qm "reviewed state"
  echo "$dir"
}

echo "FR-128 governance ledger tooling"
echo ""

# ── 1. The emitted candidate is the compared value, not a parallel implementation ──

DIR="$(new_case emit-matches-comparison)"
if diff -q \
  <(ruby "$DIR/$GATE" --emit-inventory) \
  <(jq '.retirement.shellRunnerExecutor.productionAgents' "$DIR/$LEDGER") >/dev/null; then
  pass "--emit-inventory reproduces the reviewed inventory byte for byte"
else
  fail "--emit-inventory diverges from the value the gate compares"
fi

# ── 2. Regeneration is a human act; CI must not be able to perform it ──

DIR="$(new_case ci-write-refused)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
CI=1 ruby "$DIR/$GATE" --emit-inventory --emit-baseline --write >/dev/null 2>"$WORK/ci-write.err"
WRITE_STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$WRITE_STATUS" -ne 0 && "$BEFORE" == "$AFTER" ]]; then
  pass "--write refuses under CI (exit $WRITE_STATUS) and leaves the ledger untouched"
else
  fail "--write ran under CI (exit $WRITE_STATUS) or modified the ledger"
fi

# ── 3. A no-op regeneration must not rewrite 510 reviewed lines ──
#
# Run with the CI variables cleared. Case 2 above verifies that --write refuses
# under CI; this case needs it to proceed, so inheriting the environment makes
# the two mutually exclusive. Under CI the refusal exits 2 and set -e takes the
# whole gate with it — which is why this script had never once succeeded in the
# job it was wired into, while passing 8/8 on every developer machine. The
# environment a gate runs in is part of the gate.
DIR="$(new_case write-round-trip)"
BEFORE="$(digest "$DIR/$LEDGER")"
env -u CI -u CONTINUOUS_INTEGRATION -u GITHUB_ACTIONS -u GITLAB_CI \
    -u BUILDKITE -u CIRCLECI -u TEAMCITY_VERSION -u BUILD_NUMBER \
  ruby "$DIR/$GATE" --emit-inventory --emit-baseline --write >/dev/null 2>&1
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$BEFORE" == "$AFTER" ]]; then
  pass "a no-op --write leaves the ledger byte-identical"
else
  fail "a no-op --write reformatted the ledger, burying real changes in noise"
fi

# ── 4. The failure names the agent and the changed spec key, and regeneration recovers ──

DIR="$(new_case spec-change-drill)"
TARGET="docs/workflow/command-rules.yaml"
ruby -e '
path = ARGV[0]
source = File.read(path)
abort "fixture anchor missing" unless source.include?("maxTurns: 6")
File.write(path, source.sub("maxTurns: 6", "maxTurns: 9"))
' "$DIR/$TARGET"
set +e
(cd "$DIR" && ruby "$GATE" >/dev/null 2>"$WORK/drill.err")
DRILL_STATUS=$?
set -e
if [[ "$DRILL_STATUS" -ne 0 ]] &&
  grep -q "command-rules.yaml#session-agent" "$WORK/drill.err" &&
  grep -q "spec key(s): driver" "$WORK/drill.err"; then
  # Cleared for the same reason as case 3: this exercises the recovery path,
  # which the CI refusal is designed to block.
  (cd "$DIR" && env -u CI -u CONTINUOUS_INTEGRATION -u GITHUB_ACTIONS -u GITLAB_CI \
      -u BUILDKITE -u CIRCLECI -u TEAMCITY_VERSION -u BUILD_NUMBER \
    ruby "$GATE" --emit-inventory --write >/dev/null 2>&1)
  set +e
  (cd "$DIR" && ruby "$GATE" >/dev/null 2>&1)
  RECOVERED=$?
  set -e
  CHANGED="$(git -C "$DIR" diff --numstat -- "$LEDGER" | awk '{print $1"+"$2"-"}')"
  if [[ "$RECOVERED" -eq 0 && "$CHANGED" == "1+1-" ]]; then
    pass "a spec change names the agent and key 'driver', and regeneration restores green in one line"
  else
    fail "regeneration did not restore green (exit $RECOVERED) or edited more than the changed entry ($CHANGED)"
  fi
else
  fail "a real spec change produced no per-agent diagnostic"
fi

# ── 5. Tooling must not let an unreviewed change through ──
#
# The sharpest bypass is a reviewer who pastes a regenerated fingerprint over the
# old one without noticing that the Agent also changed driver. The comparison is
# over the whole entry, so a fingerprint-only update must still fail.

DIR="$(new_case partial-ledger-update)"
ruby -e '
path = ARGV[0]
source = File.read(path)
abort "fixture anchor missing" unless source.include?("driver:\n    provider: claude")
File.write(path, source.sub("provider: claude", "provider: shell"))
' "$DIR/$TARGET"
NEW_PRINT="$(cd "$DIR" && ruby "$GATE" --emit-inventory |
  jq -r '.[] | select(.name == "session-agent") | .manifestFingerprint')"
ruby -rjson -e '
ledger = JSON.parse(File.read(ARGV[0]))
entry = ledger["retirement"]["shellRunnerExecutor"]["productionAgents"]
  .find { |agent| agent["name"] == "session-agent" }
entry["manifestFingerprint"] = ARGV[1]
File.write(ARGV[0], JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER" "$NEW_PRINT"
LEDGER_PRINT="$(jq -r '.retirement.shellRunnerExecutor.productionAgents[]
  | select(.name == "session-agent") | .manifestFingerprint' "$DIR/$LEDGER")"
set +e
(cd "$DIR" && ruby "$GATE" >/dev/null 2>"$WORK/partial.err")
PARTIAL_STATUS=$?
set -e
# The precondition is what makes this assertion mean something: the ledger's
# fingerprint is already the current one, so the inventory comparison can only
# still fail on a field the reviewer did not update.
if [[ "$LEDGER_PRINT" == "$NEW_PRINT" && "$PARTIAL_STATUS" -ne 0 ]] &&
  grep -q "production Agent execution inventory differs" "$WORK/partial.err"; then
  pass "a fingerprint-only ledger update still fails on the unreviewed classification change"
else
  fail "a fingerprint-only update let an unreviewed driver change through"
fi

# ── 6. The baseline emitter agrees with the reviewed baseline ──

DIR="$(new_case emit-baseline)"
if diff -q \
  <(ruby "$DIR/$GATE" --emit-baseline) \
  <(jq '.sourceBaseline' "$DIR/$LEDGER") >/dev/null; then
  pass "--emit-baseline reproduces the reviewed sourceBaseline"
else
  fail "--emit-baseline diverges from the reviewed sourceBaseline"
fi

# ── 7. The scanner means what sourceBaseline.scope says ──
#
# The scope claims inline cfg(test) modules are excluded. Before FR-128 only a
# single trailing `mod tests { ... }` was stripped, so ten test-only lines were
# counted as production coordination debt. Asserting that the stripper function
# behaves would only prove the function exists; this adds a test module the old
# implementation could not recognise — not named `tests`, not at end of file —
# and requires the emitted baseline to be unmoved by it. If stripping is not
# actually applied on the counting path, the count rises and this fails.

DIR="$(new_case scope-fidelity)"
PROBE="core/src/prehook/mod.rs"
BASELINE_BEFORE="$(cd "$DIR" && ruby "$GATE" --emit-baseline)"
ruby -e '
path = ARGV[0]
lines = File.readlines(path)
probe = <<~RUST
  #[cfg(test)]
  mod fr128_scope_probe {
      fn probe() {
          let _ = "captures json_path";
          let _ = PipelineVariables::default();
      }
  }
RUST
lines.insert(lines.length / 2, probe)
File.write(path, lines.join)
' "$DIR/$PROBE"
BASELINE_AFTER="$(cd "$DIR" && ruby "$GATE" --emit-baseline)"
if [[ "$BASELINE_BEFORE" == "$BASELINE_AFTER" ]]; then
  pass "a mid-file cfg(test) module named something other than 'tests' does not move the baseline"
else
  fail "test-module lines are counted as production source touches, contradicting sourceBaseline.scope"
fi

# ── 8. The ratchets are exact, so a decrease cannot pass silently ──

DIR="$(new_case ratchet-direction)"
ruby -rjson -e '
ledger = JSON.parse(File.read(ARGV[0]))
ledger["sourceBaseline"]["capturesOrJsonPath"] += 1
File.write(ARGV[0], JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER"
set +e
(cd "$DIR" && ruby "$GATE" >/dev/null 2>"$WORK/ratchet.err")
RATCHET_STATUS=$?
set -e
if [[ "$RATCHET_STATUS" -ne 0 ]] && grep -q "decreased from" "$WORK/ratchet.err"; then
  pass "a baseline above the real count fails, so stale slack cannot accumulate"
else
  fail "a baseline above the real count passed, leaving the ledger free to overstate debt"
fi

echo ""
echo "FR-128 governance ledger tooling: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
