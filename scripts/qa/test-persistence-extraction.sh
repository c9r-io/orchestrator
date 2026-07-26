#!/usr/bin/env bash
#
# FR-130 Phase A persistence crate extraction — QA gate.
#
# Phase A moved the persistence layer out of core into
# crates/orchestrator-persistence. Everything that makes that *look* done is
# structural: reference counts fall, symbols leave core/src, a member appears in
# two ledgers. All of it would be equally true of a crate that persists nothing
# and of a core that declares the dependency without using it. The two existing
# ledger gates already hold the structural half; this one exists for what they
# cannot see.
#
# Each case pairs a cheap proxy with something that observes the fact:
#
#   Case 1  cargo tree says the edge is declared — and core is compiled with the
#           declaration commented out, which must fail. Commented out rather than
#           deleted: deletion is the mutation an author has in mind, and a
#           manifest parser that skips comments would pass the deletion test
#           while accepting a commented-out dependency as present.
#   Case 2  the resume sweep is run, and a copy of it shortened with step_by is
#           run too and must fail. `for i in 1..=total` reads as exhaustive; a
#           step_by inserted for speed leaves it passing over a fraction of the
#           chain with every remaining iteration correct, which is exactly what
#           the schema comparison cannot catch. This check began as a grep for
#           the assertion's text, which a commented-out assertion satisfies.
#           Running the mutation is also what caught the chain's length being
#           recorded as 74 in five documents when it is 37.
#   Case 3  a real write/read round trip through every module the extraction
#           touched, plus the negative half against an unmigrated database — a
#           layer that returns empty instead of failing is the state a round trip
#           alone cannot tell from success.
#   Case 4  the layer does not depend on core. This is the invariant Phase A
#           establishes and the one nothing else checks: both ledgers would stay
#           green with a persistence -> core edge in place.
#   Case 5  the snapshot the extraction was measured against is byte-identical
#           to the committed one, asserted against the index rather than by
#           re-running the comparison the test above already ran.
#
# Safety: read-only against the working tree except inside $TMPDIR. No daemon is
# started, no provider is invoked, and the only databases created are temporary
# files under tempfile::tempdir(). The runtime database is never opened.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CRATE="crates/orchestrator-persistence"
MEMBER="orchestrator-persistence"
SNAPSHOT="config/governance/schema-snapshot.sql"

for command in cargo git ruby; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr130-extraction.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

cd "$REPO_ROOT"

echo "Case 1: core links the extracted crate, and cannot build without it"

# The proxy. Necessary — a crate core does not declare cannot be linked — and on
# its own worth nothing, because a declared dependency no source names still
# resolves here.
if cargo tree -p agent-orchestrator --depth 1 2>/dev/null | grep -q "$MEMBER v"; then
  pass "cargo tree reports the agent-orchestrator -> $MEMBER edge"
else
  fail "cargo tree does not report the agent-orchestrator -> $MEMBER edge"
fi

# The observation. A copy of the tracked tree with the dependency line commented
# out must fail to compile core. git archive rather than cp -r: the working tree
# carries a multi-gigabyte target/ directory, and the tracked files are ~15 MB.
DIR="$WORK/commented-dependency"
mkdir -p "$DIR"
git archive HEAD | tar -x -C "$DIR"
ruby -e '
  path = ARGV[0]
  text = File.read(path)
  line = text.lines.find { |candidate| candidate.start_with?("orchestrator-persistence =") }
  abort "core/Cargo.toml does not declare orchestrator-persistence" if line.nil?
  File.write(path, text.sub(line, "# #{line}"))
' "$DIR/core/Cargo.toml"

if grep -q '^# orchestrator-persistence =' "$DIR/core/Cargo.toml"; then
  set +e
  (cd "$DIR" && CARGO_TARGET_DIR="$DIR/target" cargo check -p agent-orchestrator) \
    >"$WORK/case1.log" 2>&1
  CHECK_STATUS=$?
  set -e
  if [[ "$CHECK_STATUS" -ne 0 ]] && grep -q "orchestrator_persistence" "$WORK/case1.log"; then
    pass "commenting out the dependency breaks core's build, naming the missing crate"
  else
    fail "core still built with the dependency commented out (exit $CHECK_STATUS)"
    tail -20 "$WORK/case1.log" >&2
  fi
else
  fail "the fixture did not comment out the dependency line"
fi
echo ""

echo "Case 2: the resume sweep covers every migration the chain applied"

set +e
cargo test -p agent-orchestrator \
  schema_snapshot::tests::an_interrupted_chain_resumes_to_the_same_schema \
  >"$WORK/case2.log" 2>&1
SWEEP_STATUS=$?
set -e
if [[ "$SWEEP_STATUS" -eq 0 ]] && grep -q "1 passed" "$WORK/case2.log"; then
  pass "the interrupted-chain sweep passes and asserts its own extent"
else
  fail "the interrupted-chain sweep did not pass (exit $SWEEP_STATUS)"
  tail -30 "$WORK/case2.log" >&2
fi

# The negative half, run rather than grepped. The first version of this check
# looked for "must be an interrupt point" in the source, which is satisfied by
# the assertion existing — including commented out, since a comment contains the
# same text. A sweep that stops early has to actually fail, so the mutation is
# applied to a copy and the test is run against it.
#
# step_by(5) rather than a deleted line: shortening for speed is the realistic
# edit, and it leaves every remaining iteration correct, which is precisely the
# case a schema comparison cannot catch.
DIR="$WORK/shortened-sweep"
mkdir -p "$DIR"
git archive HEAD | tar -x -C "$DIR"
ruby -e '
  path = ARGV[0]
  text = File.read(path)
  target = "        for stop_after in 1..=total {"
  abort "the resume sweep loop is not where this fixture expects it" unless text.include?(target)
  File.write(path, text.sub(target, "        for stop_after in (1..=total).step_by(5) {"))
' "$DIR/core/src/persistence/schema_snapshot.rs"

set +e
(cd "$DIR" && CARGO_TARGET_DIR="$DIR/target" cargo test -p agent-orchestrator \
  schema_snapshot::tests::an_interrupted_chain_resumes_to_the_same_schema) \
  >"$WORK/case2-mutant.log" 2>&1
MUTANT_STATUS=$?
set -e
if [[ "$MUTANT_STATUS" -ne 0 ]] && grep -q "must be an interrupt point" "$WORK/case2-mutant.log"; then
  pass "a sweep shortened with step_by fails on the extent assertion"
else
  fail "a sweep shortened with step_by still passed (exit $MUTANT_STATUS)"
  tail -20 "$WORK/case2-mutant.log" >&2
fi
echo ""

echo "Case 3: a task written through the layer reads back through the layer"

set +e
cargo test -p "$MEMBER" --test round_trip >"$WORK/case3.log" 2>&1
ROUND_TRIP_STATUS=$?
set -e
if [[ "$ROUND_TRIP_STATUS" -eq 0 ]] && grep -q "2 passed" "$WORK/case3.log"; then
  pass "the round trip and its unmigrated-database negative both pass"
else
  fail "the round trip did not pass (exit $ROUND_TRIP_STATUS)"
  tail -30 "$WORK/case3.log" >&2
fi
echo ""

echo "Case 4: the persistence layer does not depend on core"

# Derived from cargo's own resolution rather than from reading manifests, so a
# transitive path through a third member is caught as well as a direct one.
if cargo tree -p "$MEMBER" 2>/dev/null | grep -q "agent-orchestrator v"; then
  fail "$MEMBER reaches agent-orchestrator; the layer is not below core"
  cargo tree -p "$MEMBER" 2>/dev/null | grep "agent-orchestrator v" >&2
else
  pass "$MEMBER's dependency tree does not reach agent-orchestrator"
fi

# And the same question asked of the extracted sources: a core path named there
# would not compile today, but it is the edit that would make the answer above
# change, so it is worth naming at the point it appears.
if grep -rn "agent_orchestrator::" "$CRATE/src" >"$WORK/case4.txt" 2>/dev/null; then
  fail "the extracted sources name agent_orchestrator::"
  cat "$WORK/case4.txt" >&2
else
  pass "no source under $CRATE/src names agent_orchestrator::"
fi
echo ""

echo "Case 5: the reviewed schema baseline is unchanged"

if git diff --quiet -- "$SNAPSHOT" && git diff --cached --quiet -- "$SNAPSHOT"; then
  pass "$SNAPSHOT is byte-identical to the committed baseline"
else
  fail "$SNAPSHOT differs from the committed baseline"
  git diff -- "$SNAPSHOT" >&2
fi

# The baseline predates the extraction. That is what makes it a baseline rather
# than a record of the outcome, and it is a fact about history, so it is read
# from history.
BASELINE_COMMIT="$(git log --format=%H -1 -- "$SNAPSHOT")"
FIRST_MOVE="$(git log --format=%H --reverse --grep='FR-130 A1' -1)"
if [[ -z "$FIRST_MOVE" ]]; then
  pass "no FR-130 A1 commit in range; baseline ordering not assertable here"
elif [[ -n "$BASELINE_COMMIT" ]] && git merge-base --is-ancestor "$BASELINE_COMMIT" "$FIRST_MOVE"; then
  pass "the baseline was committed before the first extraction commit"
else
  fail "the baseline is not an ancestor of the first extraction commit"
fi
echo ""

echo "FR-130 Phase A persistence crate extraction: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
