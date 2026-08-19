#!/usr/bin/env bash
#
# FR-130 core boundary freeze — QA gate.
#
# Verifies that scripts/qa/core-boundary.rb actually holds the core crate
# boundary, and that core/src/persistence/schema_snapshot.rs actually holds the
# migration chain's schema. A gate observed only passing has not been observed
# doing anything, so every case below is paired with a defect it must reject.
#
# Safety: every mutation happens inside a temporary copy under $TMPDIR. The
# working tree is never written, no daemon is started, no database outside
# $TMPDIR is touched, and no provider is invoked.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/core-boundary.rb"
GATE_LIB="scripts/lib/rust_source.rb"
# Every shared library the two ruby gates require. A case repo is assembled by
# copying, so a library missing from this list makes the gate under test die on
# its require — and a case that expects the gate to fail then passes for the
# wrong reason. Case 9 removes GATE_LIB deliberately and is the exception.
GATE_LIBS=(
  scripts/lib/rust_source.rb
  scripts/lib/rust_lexer.rb
  scripts/lib/ci_env.rb
)
COORD_GATE="scripts/qa/coordination-governance.rb"
COORD_LEDGER="config/governance/coordination-collapse-ledger.json"
LEDGER="config/governance/core-boundary-ledger.json"
SNAPSHOT="config/governance/schema-snapshot.sql"

for command in ruby cargo; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr130-core-boundary.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

digest() { ruby -rdigest -e 'print Digest::SHA256.file(ARGV[0]).hexdigest' "$1"; }

# A case copies only what the gate scans: core/src, the member manifests and
# their sources, the ledgers, and the two ruby gates with their shared library.
# Copying the repository wholesale would drag in target/ and make each case cost
# gigabytes.
new_case() {
  local dir
  dir="$WORK/$1"
  mkdir -p "$dir/config/governance" "$dir/scripts/qa" "$dir/scripts/lib" "$dir/crates" "$dir/docs"
  cp -R "$REPO_ROOT/core" "$dir/core"
  local crate name
  for crate in "$REPO_ROOT"/crates/*/; do
    name="$(basename "$crate")"
    mkdir -p "$dir/crates/$name"
    [[ -f "$crate/Cargo.toml" ]] && cp "$crate/Cargo.toml" "$dir/crates/$name/Cargo.toml"
    [[ -d "$crate/src" ]] && cp -R "$crate/src" "$dir/crates/$name/src"
  done
  cp -R "$REPO_ROOT/docs/workflow" "$dir/docs/workflow"
  cp "$REPO_ROOT/$LEDGER" "$dir/$LEDGER"
  cp "$REPO_ROOT/$COORD_LEDGER" "$dir/$COORD_LEDGER"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  cp "$REPO_ROOT/$COORD_GATE" "$dir/$COORD_GATE"
  local lib
  for lib in ${GATE_LIBS[@]+"${GATE_LIBS[@]}"}; do
    cp "$REPO_ROOT/$lib" "$dir/$lib"
  done
  echo "$dir"
}

echo "FR-130 core boundary"
echo ""

# --- Case 1: the gate passes on the repository -------------------------------
echo "Case 1: the gate holds on the working tree"
if (cd "$REPO_ROOT" && ruby "$GATE" > "$WORK/case1.out" 2> "$WORK/case1.err"); then
  if grep -q "Core boundary: PASS" "$WORK/case1.out"; then
    pass "the core boundary gate passes on the repository"
  else
    fail "the gate exited 0 without printing its PASS summary"
  fi
else
  fail "the core boundary gate does not pass on the repository"
  cat "$WORK/case1.err" >&2
fi
echo ""

# --- Case 2: the emitted candidate is the reviewed ledger --------------------
# The recovery path and the compared value have to be the same thing. If they
# can differ, regenerating produces a ledger the gate then rejects, and the
# reviewer is told to fix a file the tool just wrote.
echo "Case 2: --emit-baseline reproduces the reviewed ledger byte for byte"
(cd "$REPO_ROOT" && ruby "$GATE" --emit-baseline) > "$WORK/emitted.json" 2>/dev/null || true
if cmp -s "$WORK/emitted.json" "$REPO_ROOT/$LEDGER"; then
  pass "the emitted candidate is byte-identical to the committed ledger"
else
  fail "--emit-baseline differs from the ledger the gate compares against"
# `sed -n` reads to end of input; `head` leaves early and kills the producer, and under
# `set -o pipefail` that status reaches `set -e` and ends the run with no summary line
# (FR-146). Measured: a 129 KB producer into `| head -1` dies 10 times out of 10.
  # The `|| true` went with it: it was there to survive `head`'s SIGPIPE, and it also
  # swallowed a real `diff` failure (FR-144's class).
  diff "$REPO_ROOT/$LEDGER" "$WORK/emitted.json" | sed -n '1,20p' >&2
  # Which interpreter produced that, because this comparison is byte-exact and
  # `ledger_json` is not version-stable. It collapses `{\n\n}` to `{}` but not
  # `{\n  }`, so an empty object renders differently across json gem versions and
  # a ledger regenerated on one Ruby fails byte-comparison on another. A diff
  # showing only empty-object rendering is that, not a boundary that moved — and
  # without these three lines the reader cannot tell those apart from the log.
  {
    echo "  interpreter: $(ruby -v 2>&1)"
    echo "  json gem:    $(ruby -rjson -e 'print(Gem.loaded_specs["json"]&.version || JSON::VERSION)' 2>&1)"
    echo "  empty hash:  $(ruby -rjson -e 'print JSON.pretty_generate({"k" => {}}).inspect' 2>&1)"
  } >&2
fi
echo ""

# --- Case 3: a widened module surface fails ----------------------------------
echo "Case 3: a new top-level pub mod in core fails the gate"
DIR="$(new_case pub-mod)"
printf '\npub mod fr130_probe;\n' >> "$DIR/core/src/lib.rs"
# Read from the ledger, not written as a literal. This case asserted
# "52 -> 53" until FR-130 Phase A moved the count to 50, at which point a gate
# whose whole subject is a number that changes had a fixture that could only
# work while it did not.
PUBMOD_BEFORE="$(ruby -rjson -e 'print JSON.parse(File.read(ARGV[0]))["coreSurface"]["pubMod"]' "$REPO_ROOT/$LEDGER")"
PUBMOD_AFTER=$((PUBMOD_BEFORE + 1))
set +e
(cd "$DIR" && ruby "$GATE" > "$WORK/case3.out" 2> "$WORK/case3.err")
STATUS=$?
set -e
if [[ "$STATUS" -ne 0 ]] && grep -q "coreSurface.pubMod $PUBMOD_BEFORE -> $PUBMOD_AFTER" "$WORK/case3.err"; then
  pass "a new pub mod fails and the report names the count it moved"
else
  fail "a new pub mod did not fail the gate with a named pubMod change (exit $STATUS)"
  cat "$WORK/case3.err" >&2
fi
echo ""

# --- Case 4: a new rusqlite touch point fails --------------------------------
echo "Case 4: a new rusqlite reference in core fails the gate"
DIR="$(new_case rusqlite-added)"
printf '\nuse rusqlite::Connection;\n' >> "$DIR/core/src/health.rs"
set +e
(cd "$DIR" && ruby "$GATE" > "$WORK/case4.out" 2> "$WORK/case4.err")
STATUS=$?
set -e
if [[ "$STATUS" -ne 0 ]] && grep -q "+ core/src/health.rs references rusqlite" "$WORK/case4.err"; then
  pass "a new rusqlite touch point fails and the report names the file"
else
  fail "a new rusqlite reference did not fail the gate naming the file (exit $STATUS)"
  cat "$WORK/case4.err" >&2
fi
echo ""

# --- Case 5: a removed rusqlite touch point also fails -----------------------
# This is the case FR-128 paid for. Under the monotonic ratchet FR-130 asked
# for, a decrease passes silently and the ledger goes on asserting debt the
# repository no longer carries — green, and false. Here a decrease is the goal,
# which is exactly why it has to be blessed rather than absorbed.
echo "Case 5: a removed rusqlite reference also fails, so the ledger cannot go stale"
DIR="$(new_case rusqlite-removed)"
# The stale state is constructed in the ledger rather than in the source, and it
# has to be: FR-141 took core to zero rusqlite references, so there is no longer
# any reference to strip. The earlier form read a target out of
# `rusqlite.files` and deleted its tokens; with the map empty that read returns
# nothing and the case wrote to a directory instead of failing. Claiming a file
# the repository does not reference exercises the same branch — the ledger
# over-claiming, `before.keys - after.keys` — and keeps working whatever the
# residual is, including at zero, which is the state this whole gate was built
# to reach.
REMOVAL_TARGET="core/src/db.rs"
# This case is the second of FR-143's nine, twice over. It first read its target
# out of `rusqlite.files.keys.min`, which FR-141 B4 emptied, so the read returned
# nothing and the case wrote to a directory. Before that it stripped tokens from
# a `db.rs` that had become a re-export shell, mutated nothing, and reported that
# the gate had failed to notice a removal — an accusation aimed at the gate for
# the fixture's own defect.
#
# Both are now impossible: the target must be a regular file, and the ledger must
# actually change. The abort keeps its words and becomes the diagnosis.
if fixture_mutate "case 5" "$DIR/$LEDGER" ruby -rjson -e '
path = ARGV[0]
ledger = JSON.parse(File.read(path))
target = ARGV[1]
abort "the repository still references rusqlite in #{target}; pick a target it does not" if
  ledger["rusqlite"]["files"].key?(target)
ledger["rusqlite"]["files"][target] = 3
ledger["rusqlite"]["total"] += 3
File.write(path, JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER" "$REMOVAL_TARGET"; then
  set +e
  (cd "$DIR" && ruby "$GATE" > "$WORK/case5.out" 2> "$WORK/case5.err")
  STATUS=$?
  set -e
  if [[ "$STATUS" -ne 0 ]] && grep -q "\- $REMOVAL_TARGET no longer references rusqlite" "$WORK/case5.err"; then
    pass "a ledger that over-claims a reference fails, and the report says so"
  else
    fail "an over-claiming ledger did not fail the gate (exit $STATUS)"
    cat "$WORK/case5.err" >&2
  fi
fi
echo ""

# --- Case 6: cfg(test) is out of scope, for both gates -----------------------
# Written as an assertion about the emitted baseline rather than about
# strip_test_modules. Testing the helper directly proves a function exists, not
# that the counting path calls it — the textual-presence-as-execution-fact error
# FR-134 documents. Both gates are checked with one probe because they share one
# scanner: if either grew a private copy that drifted, one baseline would move.
echo "Case 6: a cfg(test) module moves neither gate's baseline"
DIR="$(new_case scope-fidelity)"
PROBE="$DIR/core/src/prehook/mod.rs"
# Captured without `set -e`: a gate that cannot run at all must be reported as a
# failed case, not abort the run and take the remaining cases with it.
set +e
BOUNDARY_BEFORE="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2> "$WORK/case6-boundary.err")"
BOUNDARY_BEFORE_STATUS=$?
COORD_BEFORE="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2> "$WORK/case6-coord.err")"
COORD_BEFORE_STATUS=$?
set -e
# This case asserts the baselines do NOT move, so an inert mutation passes it
# vacuously — the strongest live instance of FR-143's second incident in this
# repository. `core/src/prehook/mod.rs` is named here and nowhere else; the day
# it moves, `File.readlines` raises, and before FR-143 that ended the run.
# Proving the insertion landed is what makes the equality below mean anything.
if fixture_mutate "case 6" "$PROBE" ruby -e '
path = ARGV[0]
lines = File.readlines(path)
probe = <<~RUST
  #[cfg(test)]
  mod fr130_scope_probe {
      use rusqlite::Connection;
      fn probe(_conn: &Connection) {
          let _ = "captures json_path";
          let _ = PipelineVariables::default();
      }
  }
RUST
lines.insert(lines.length / 2, probe)
File.write(path, lines.join)
' "$PROBE"; then
  set +e
  BOUNDARY_AFTER="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2>> "$WORK/case6-boundary.err")"
  BOUNDARY_AFTER_STATUS=$?
  COORD_AFTER="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2>> "$WORK/case6-coord.err")"
  COORD_AFTER_STATUS=$?
  set -e
  if [[ "$BOUNDARY_BEFORE_STATUS" -ne 0 || "$COORD_BEFORE_STATUS" -ne 0 ||
    "$BOUNDARY_AFTER_STATUS" -ne 0 || "$COORD_AFTER_STATUS" -ne 0 ]]; then
    fail "a gate could not emit a baseline, so the scope comparison proves nothing"
    cat "$WORK/case6-boundary.err" "$WORK/case6-coord.err" >&2
  elif [[ "$BOUNDARY_BEFORE" == "$BOUNDARY_AFTER" && "$COORD_BEFORE" == "$COORD_AFTER" ]]; then
    pass "test-only rusqlite, captures and PipelineVariables lines are excluded by both gates"
  else
    fail "a cfg(test) module changed an emitted baseline; the scan does not match its scope"
    diff <(echo "$BOUNDARY_BEFORE") <(echo "$BOUNDARY_AFTER") >&2 || true
    diff <(echo "$COORD_BEFORE") <(echo "$COORD_AFTER") >&2 || true
  fi
fi
echo ""

# --- Case 7: --write refuses under CI ----------------------------------------
echo "Case 7: --write refuses to run under CI"
DIR="$(new_case ci-write)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
(cd "$DIR" && CI=1 ruby "$GATE" --emit-baseline --write > "$WORK/case7.out" 2> "$WORK/case7.err")
STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$STATUS" -ne 0 && "$BEFORE" == "$AFTER" ]] &&
  grep -q "refusing --write under CI" "$WORK/case7.err"; then
  pass "--write refuses under CI (exit $STATUS) and leaves the ledger untouched"
else
  fail "--write did not refuse under CI or modified the ledger (exit $STATUS)"
  cat "$WORK/case7.err" >&2
fi

# `CI` is a GitHub and Travis convention, not a universal one. A self-hosted
# runner or a cron job that exports GITHUB_ACTIONS but not CI walked straight
# through the old guard and rewrote the reviewed ledger with no human present —
# the single barrier keeping the review gate from being decoration. FR-134
# widened the surface; this is the half of it that can regress silently, because
# the CI=1 case above keeps passing either way.
DIR="$(new_case ci-write-github-actions)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
(cd "$DIR" && env -u CI GITHUB_ACTIONS=true ruby "$GATE" --emit-baseline --write \
  > "$WORK/case7b.out" 2> "$WORK/case7b.err")
STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$STATUS" -ne 0 && "$BEFORE" == "$AFTER" ]] &&
  grep -q "refusing --write under GITHUB_ACTIONS" "$WORK/case7b.err"; then
  pass "--write refuses under GITHUB_ACTIONS with CI unset, and names why"
else
  fail "--write ran with only GITHUB_ACTIONS set (exit $STATUS), or modified the ledger"
  cat "$WORK/case7b.err" >&2
fi

# And the other direction: CI=false is how a developer says "treat this as
# interactive". A guard that only tests for presence would block it, and the
# recovery path this ledger depends on would be unusable on that machine.
#
# Every other indicator has to be cleared, not just CI. This case ran green
# locally and failed in CI on its first real run, because the runner also
# exports GITHUB_ACTIONS and the guard was right to keep refusing — the test was
# wrong, not the guard. That is the same environment-dependence this FR is
# about, in a test written to check it.
DIR="$(new_case ci-write-false)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
(cd "$DIR" && env -u CONTINUOUS_INTEGRATION -u GITHUB_ACTIONS -u GITLAB_CI \
    -u BUILDKITE -u CIRCLECI -u TEAMCITY_VERSION -u BUILD_NUMBER \
  CI=false ruby "$GATE" --emit-baseline --write > /dev/null 2> "$WORK/case7c.err")
STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$STATUS" -eq 0 && "$BEFORE" == "$AFTER" ]]; then
  pass "CI=false is treated as interactive, and a no-op write still changes nothing"
else
  fail "CI=false was treated as unattended (exit $STATUS) or the write was not a no-op"
  cat "$WORK/case7c.err" >&2
fi
echo ""

# --- Case 8: the schema snapshot rejects a schema that is not the reviewed one
# The snapshot is FR-130's pre-extraction baseline. A baseline that cannot fail
# is not a baseline, so the comparison is pointed at a copy with one table
# removed and required to name that table.
echo "Case 8: the schema snapshot rejects a schema that differs from the reviewed one"
DOCTORED="$WORK/doctored-schema.sql"
grep -v '^CREATE TABLE tasks ' "$REPO_ROOT/$SNAPSHOT" > "$DOCTORED"
if cmp -s "$DOCTORED" "$REPO_ROOT/$SNAPSHOT"; then
  fail "the doctored snapshot is identical to the reviewed one; the fixture is inert"
else
  set +e
  (cd "$REPO_ROOT" && SCHEMA_SNAPSHOT_PATH="$DOCTORED" \
    cargo test -p agent-orchestrator schema_snapshot > "$WORK/case8-bad.log" 2>&1)
  BAD_STATUS=$?
  (cd "$REPO_ROOT" && cargo test -p agent-orchestrator schema_snapshot \
    > "$WORK/case8-good.log" 2>&1)
  GOOD_STATUS=$?
  set -e
  if [[ "$BAD_STATUS" -ne 0 && "$GOOD_STATUS" -eq 0 ]] &&
    grep -q '+ CREATE TABLE tasks ' "$WORK/case8-bad.log"; then
    pass "a missing table fails the snapshot and is named in the diff; the real snapshot passes"
  else
    fail "the schema snapshot did not reject a doctored baseline (bad=$BAD_STATUS good=$GOOD_STATUS)"
    tail -20 "$WORK/case8-bad.log" >&2
  fi
fi
echo ""

# --- Case 9: the scanner is one implementation, not two ----------------------
# Case 6 shows the two gates agree today. This shows they agree because they are
# the same code: remove the shared library and neither can run. A gate that
# survived this would be carrying a private copy free to drift.
#
# The assertion is a controlled before-and-after inside one directory. "Both
# gates fail once the library is gone" is not enough on its own — a gate that was
# already broken for an unrelated reason satisfies it, and that is exactly how
# this case passed for the wrong reason against mutation M7 during the FR-130
# mutation run. Requiring both to pass first is what makes the failure
# attributable to the removal.
echo "Case 9: neither gate can run without the shared scanner"
DIR="$(new_case shared-scanner)"
set +e
(cd "$DIR" && ruby "$GATE" > /dev/null 2> "$WORK/case9-boundary-before.err")
BOUNDARY_BEFORE_STATUS=$?
(cd "$DIR" && ruby "$COORD_GATE" > /dev/null 2> "$WORK/case9-coord-before.err")
COORD_BEFORE_STATUS=$?
set -e
rm "$DIR/$GATE_LIB"
set +e
(cd "$DIR" && ruby "$GATE" > /dev/null 2> "$WORK/case9-boundary.err")
BOUNDARY_STATUS=$?
(cd "$DIR" && ruby "$COORD_GATE" > /dev/null 2> "$WORK/case9-coord.err")
COORD_STATUS=$?
set -e
if [[ "$BOUNDARY_BEFORE_STATUS" -ne 0 || "$COORD_BEFORE_STATUS" -ne 0 ]]; then
  fail "a gate already failed with the shared scanner present, so its later failure proves nothing" \
    "(boundary=$BOUNDARY_BEFORE_STATUS coordination=$COORD_BEFORE_STATUS)"
  cat "$WORK/case9-boundary-before.err" "$WORK/case9-coord-before.err" >&2
elif [[ "$BOUNDARY_STATUS" -ne 0 && "$COORD_STATUS" -ne 0 ]] &&
  grep -q "rust_source" "$WORK/case9-boundary.err" &&
  grep -q "rust_source" "$WORK/case9-coord.err"; then
  pass "both gates pass with scripts/lib/rust_source.rb and fail without it; neither holds a private copy"
else
  fail "a gate ran without the shared scanner (boundary=$BOUNDARY_STATUS coordination=$COORD_STATUS)"
  cat "$WORK/case9-boundary.err" "$WORK/case9-coord.err" >&2
fi
echo ""

# --- Case 10: a brace inside a literal does not hide the code after it -------
# FR-134 defects X and W. The scanner used to count `{` and `}` textually, so
# `.body("{")` inside a cfg(test) module left the depth counter above zero, the
# module's range ran to end of file, and every production line after it vanished
# from both ledgers. The counts do not move when that happens, and both ledgers
# now compare for exact equality, so the ratchet stays green over a growing
# blind spot.
#
# Two ledgers count different things, so the probe carries both: a production
# rusqlite reference for the boundary ledger and production captures /
# PipelineVariables lines for the coordination one. The assertion is that both
# baselines MOVE. Before the lexer fix they do not move at all, which is the
# defect stated as a test.
echo "Case 10: production code after a brace-unbalanced cfg(test) module is still counted"
DIR="$(new_case lexical-safety)"
PROBE="$DIR/core/src/prehook/mod.rs"
set +e
BOUNDARY_BEFORE="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2> "$WORK/case10-b.err")"
COORD_BEFORE="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2> "$WORK/case10-c.err")"
set -e
if fixture_mutate "case 10" "$PROBE" ruby -e '
path = ARGV[0]
lines = File.readlines(path)
# The unbalanced literals are the three shapes this repository actually
# contains: an interpolated format string, an escaped brace, and a lone brace
# passed as an argument. Each opens a brace the old counter never closed.
probe = <<~'"'"'RUST'"'"'
  #[cfg(test)]
  mod fr134_unbalanced_probe {
      #[test]
      fn braces_inside_literals() {
          let _ = format!("{err}", err = 1);
          let _ = "{{bad";
          let _ = String::from("{");
      }
  }

  pub fn fr134_production_after_probe(conn: &rusqlite::Connection) {
      let _ = conn;
      let _ = "captures json_path";
      let _ = PipelineVariables::default();
  }
RUST
lines.insert(lines.length / 2, probe)
File.write(path, lines.join)
' "$PROBE"; then
  set +e
  BOUNDARY_AFTER="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2>> "$WORK/case10-b.err")"
  COORD_AFTER="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2>> "$WORK/case10-c.err")"
  set -e
  if [[ -z "$BOUNDARY_BEFORE" || -z "$COORD_BEFORE" || -z "$BOUNDARY_AFTER" || -z "$COORD_AFTER" ]]; then
    fail "a gate could not emit a baseline, so the visibility comparison proves nothing"
    cat "$WORK/case10-b.err" "$WORK/case10-c.err" >&2
  elif [[ "$BOUNDARY_BEFORE" != "$BOUNDARY_AFTER" && "$COORD_BEFORE" != "$COORD_AFTER" ]]; then
    pass "production rusqlite, captures and PipelineVariables after an unbalanced literal reach both ledgers"
  else
    fail "a brace inside a literal hid the production code after it from a ledger"
    [[ "$BOUNDARY_BEFORE" == "$BOUNDARY_AFTER" ]] && echo "    core boundary baseline did not move" >&2
    [[ "$COORD_BEFORE" == "$COORD_AFTER" ]] && echo "    coordination baseline did not move" >&2
  fi
fi
echo ""

# --- Case 11: the fix does not close a module early --------------------------
# The other direction, and the reason the fix is a lexer rather than a regular
# expression. A per-line masker cannot see a raw string that spans lines: it
# reads the closing line's `}` as code, decides the module ended there, and
# hands the rest of the test module to the ledgers as production usage. The
# repository already contains this shape at item_generate.rs:199, so a fix that
# gets it wrong moves capturesOrJsonPath from 53 to 60.
echo "Case 11: a multi-line raw string does not end a cfg(test) module early"
DIR="$(new_case raw-string-safety)"
PROBE="$DIR/core/src/prehook/mod.rs"
set +e
BOUNDARY_BEFORE="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2> "$WORK/case11-b.err")"
COORD_BEFORE="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2> "$WORK/case11-c.err")"
set -e
# Case 11 also asserts the baselines do NOT move, so it is the second live
# instance an inert mutation would pass silently.
if fixture_mutate "case 11" "$PROBE" ruby -e '
path = ARGV[0]
lines = File.readlines(path)
probe = <<~'"'"'RUST'"'"'
  #[cfg(test)]
  mod fr134_raw_string_probe {
      #[test]
      fn raw_string_spans_lines() {
          let fixture = r#"{"items": [
              {"id": "a", "json_path": "$.items"}
          ]}"#;
          let _ = fixture;
          let _ = rusqlite::Connection::open_in_memory();
          let _ = "captures json_path";
          let _ = PipelineVariables::default();
      }
  }
RUST
lines.insert(lines.length / 2, probe)
File.write(path, lines.join)
' "$PROBE"; then
  set +e
  BOUNDARY_AFTER="$(cd "$DIR" && ruby "$GATE" --emit-baseline 2>> "$WORK/case11-b.err")"
  COORD_AFTER="$(cd "$DIR" && ruby "$COORD_GATE" --emit-baseline 2>> "$WORK/case11-c.err")"
  set -e
  if [[ -z "$BOUNDARY_BEFORE" || -z "$COORD_BEFORE" || -z "$BOUNDARY_AFTER" || -z "$COORD_AFTER" ]]; then
    fail "a gate could not emit a baseline, so the raw string comparison proves nothing"
    cat "$WORK/case11-b.err" "$WORK/case11-c.err" >&2
  elif [[ "$BOUNDARY_BEFORE" == "$BOUNDARY_AFTER" && "$COORD_BEFORE" == "$COORD_AFTER" ]]; then
    pass "a test module containing a multi-line raw string stays excluded from both ledgers"
  else
    fail "a multi-line raw string ended a cfg(test) module early and leaked test code into a ledger"
    diff <(echo "$BOUNDARY_BEFORE") <(echo "$BOUNDARY_AFTER") >&2 || true
    diff <(echo "$COORD_BEFORE") <(echo "$COORD_AFTER") >&2 || true
  fi
fi
echo ""

# --- Case 12: no cfg(test) module in the tree runs off the end ---------------
# Cases 10 and 11 prove the scanner handles the shapes. This proves the tree has
# none it cannot handle. A module whose depth never returns to zero excludes
# everything after it, and that is silent in the counts by construction — the
# hidden lines simply stop being counted, so no baseline moves to signal it.
echo "Case 12: no cfg(test) module in the scanned tree fails to close"
UNCLOSED="$(cd "$REPO_ROOT" && ruby -e '
$LOAD_PATH.unshift "scripts/lib"
require "rust_source"
require "pathname"
RustSource.unclosed_test_modules(Pathname.new(Dir.pwd)).each { |path, line| puts "#{path}:#{line}" }
' 2> "$WORK/case12.err")"
if [[ -n "$(cat "$WORK/case12.err")" ]]; then
  fail "the unclosed-module scan could not run"
  cat "$WORK/case12.err" >&2
elif [[ -z "$UNCLOSED" ]]; then
  pass "every cfg(test) module in core/src and crates/*/src closes"
else
  fail "a cfg(test) module never closes, hiding every line after it from both ledgers:"
  printf '    %s\n' $UNCLOSED >&2
fi
echo ""

echo "FR-130 core boundary: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
