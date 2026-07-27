#!/usr/bin/env bash
#
# FR-136 persistence dependency chokepoint — QA gate.
#
# Verifies that scripts/qa/persistence-dependency.rb actually holds the decision
# rather than reporting on it. A gate observed only passing has not been observed
# doing anything, so every case below applies a mutation it must reject — except
# cases 4 and 14, which apply ones it must NOT reject, because a rule that fails
# on every change is a ratchet, not a policy.
#
# The cases that carry the argument:
#
#   Case 4  an exempt crate adds a driver declaration and the gate stays green.
#           Without it, "the gate fails when I touch a manifest" is indistinguishable
#           from "the gate enforces a per-crate chokepoint".
#   Case 7  a forbidden crate gains a SQL statement and no rusqlite token at all.
#           This is the state a manifest-only gate reports as clean, and it is not
#           hypothetical: crates/orchestrator-security/src/secret_store_crypto.rs
#           runs four production SQL statements with zero driver references today.
#   Case 14 log prose must NOT be counted as SQL, asserted in the same file a real
#           statement then proves is scanned — so the green is about the prose and
#           not about the location. Cases 12 and 13 make the match stricter; this
#           is what stops the repair from becoming a relaxation.
#   Case 15 a forbidden crate's build script runs SQL. Condition 1 already treats
#           [build-dependencies] as production; until FR-139 condition 2 never
#           opened the file that would use it.
#
# Cases 12 to 17 are FR-139's. FR-136 shipped this suite with an assertion no
# input could fail (see case 16) and a scan narrower than its own scope prose
# (case 17).
#
# Safety: every mutation happens inside a temporary copy under $TMPDIR. The
# working tree is never written, no daemon is started, no database is touched,
# and no provider is invoked.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/persistence-dependency.rb"
LEDGER="config/governance/persistence-dependency-ledger.json"
# Every shared library the gate requires. A library missing here makes the gate
# die on its require, and a case expecting failure then passes for the wrong
# reason — the shape that let FR-130's case 9 pass against mutation M7.
GATE_LIBS=(
  scripts/lib/rust_source.rb
  scripts/lib/rust_lexer.rb
  scripts/lib/ci_env.rb
)

for command in ruby; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr136-persistence.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

digest() { ruby -rdigest -e 'print Digest::SHA256.file(ARGV[0]).hexdigest' "$1"; }

# A case copies what the gate reads: the root manifest (its member list is the
# discovery source), core/src, every member's manifest, sources and build
# script, the ledger, the gate and its libraries. Copying the repository
# wholesale would drag in target/ and cost gigabytes per case.
#
# The build script is copied because FR-139 widened the scan to include it. Case
# 15 asserts a build script is read; without the copy that case would exercise a
# root the gate reports as missing, and would pass for the wrong reason.
new_case() {
  local dir crate name
  dir="$WORK/$1"
  mkdir -p "$dir/config/governance" "$dir/scripts/qa" "$dir/scripts/lib" "$dir/crates"
  cp "$REPO_ROOT/Cargo.toml" "$dir/Cargo.toml"
  mkdir -p "$dir/core"
  cp "$REPO_ROOT/core/Cargo.toml" "$dir/core/Cargo.toml"
  cp -R "$REPO_ROOT/core/src" "$dir/core/src"
  for crate in "$REPO_ROOT"/crates/*/; do
    name="$(basename "$crate")"
    mkdir -p "$dir/crates/$name"
    if [[ -f "$crate/Cargo.toml" ]]; then cp "$crate/Cargo.toml" "$dir/crates/$name/Cargo.toml"; fi
    if [[ -d "$crate/src" ]]; then cp -R "$crate/src" "$dir/crates/$name/src"; fi
    if [[ -f "$crate/build.rs" ]]; then cp "$crate/build.rs" "$dir/crates/$name/build.rs"; fi
  done
  cp "$REPO_ROOT/$LEDGER" "$dir/$LEDGER"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  local lib
  for lib in "${GATE_LIBS[@]}"; do
    cp "$REPO_ROOT/$lib" "$dir/$lib"
  done
  echo "$dir"
}

# Runs the gate in a case directory and records the exit status without letting
# `set -e` abort the whole run. A gate that cannot start must be a failed case,
# not a truncated log.
run_gate() {
  local dir="$1" tag="$2"
  shift 2
  set +e
  (cd "$dir" && ruby "$GATE" "$@" > "$WORK/$tag.out" 2> "$WORK/$tag.err")
  STATUS=$?
  set -e
}

echo "FR-136 persistence dependency chokepoint"
echo ""

# --- Case 1: the gate passes on the repository -------------------------------
echo "Case 1: the gate holds on the working tree"
run_gate "$REPO_ROOT" case1
if [[ "$STATUS" -eq 0 ]] && grep -q "Persistence dependency: PASS" "$WORK/case1.out"; then
  pass "the persistence dependency gate passes on the repository"
else
  fail "the persistence dependency gate does not pass on the repository (exit $STATUS)"
  cat "$WORK/case1.err" >&2
fi
echo ""

# --- Case 2: the emitted candidate is the reviewed ledger --------------------
# The recovery path and the compared value have to be the same thing. If they can
# differ, regenerating produces a ledger the gate then rejects, and the reviewer
# is told to fix a file the tool just wrote.
echo "Case 2: --emit-baseline reproduces the reviewed ledger byte for byte"
(cd "$REPO_ROOT" && ruby "$GATE" --emit-baseline) > "$WORK/emitted.json" 2>/dev/null || true
if cmp -s "$WORK/emitted.json" "$REPO_ROOT/$LEDGER"; then
  pass "the emitted candidate is byte-identical to the committed ledger"
else
  fail "--emit-baseline differs from the ledger the gate compares against"
  diff "$REPO_ROOT/$LEDGER" "$WORK/emitted.json" | head -20 >&2 || true
fi
echo ""

# --- Case 3: a crate with no persistence role may not reach for the driver ----
# The thing the chokepoint exists to stop: a new consumer. crates/cli is `none`,
# so this is not a residual being paid down but an edge being created.
echo "Case 3: a crate with role 'none' declaring the driver fails"
DIR="$(new_case none-declares)"
ruby -e '
path = ARGV[0]
source = File.read(path)
source.sub!(/^\[dependencies\]$/, "[dependencies]\nrusqlite = { version = \"0.31\", features = [\"bundled\"] }")
File.write(path, source)
' "$DIR/crates/cli/Cargo.toml"
run_gate "$DIR" case3
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "crates/cli is none and must not name the SQLite driver" "$WORK/case3.err"; then
  pass "a role-'none' crate declaring rusqlite fails, and the report names the crate and its role"
else
  fail "a role-'none' crate declaring rusqlite did not fail the gate (exit $STATUS)"
  cat "$WORK/case3.err" >&2
fi
echo ""

# --- Case 4: the rule discriminates, it does not simply forbid change --------
# This is the case that separates a policy from a ratchet. orchestrator-security
# is exempt with a written reason — it sits below core and cannot route upward —
# so adding the async wrapper beside the driver it already declares must NOT
# fail. A gate that fails here is enforcing "nothing may change", which would
# have exactly the same green record on the repository as the real rule.
echo "Case 4: an exempt crate adding a driver declaration does NOT fail"
DIR="$(new_case exempt-declares)"
ruby -e '
path = ARGV[0]
source = File.read(path)
source.sub!(/^(rusqlite = .*)$/, "\\1\ntokio-rusqlite = \"0.5\"")
File.write(path, source)
' "$DIR/crates/orchestrator-security/Cargo.toml"
if ! grep -q '^tokio-rusqlite' "$DIR/crates/orchestrator-security/Cargo.toml"; then
  fail "the fixture did not add a declaration, so the case proves nothing"
else
  run_gate "$DIR" case4
  if [[ "$STATUS" -eq 0 ]]; then
    pass "an exempt crate may add a driver declaration; the rule reads roles, not diffs"
  else
    fail "the gate rejected a declaration its own ledger permits (exit $STATUS)"
    cat "$WORK/case4.err" >&2
  fi
fi
echo ""

# --- Case 5: [dev-dependencies] and [dependencies] are different facts -------
# core-boundary.rb's whole-file `match?` cannot tell them apart, which is why
# crates/integration-tests sits in its frozen list beside four production crates
# as though it were the same kind of fact. Moving the line between sections
# changes nothing textually that a whole-file match would notice.
echo "Case 5: promoting the test-only crate's driver to a production dependency fails"
DIR="$(new_case dev-to-prod)"
ruby -e '
path = ARGV[0]
lines = File.readlines(path)
moved = lines.reject { |line| line =~ /^rusqlite\s*=/ }
index = moved.index { |line| line.strip == "[dependencies]" }
abort "no [dependencies] section in the fixture" if index.nil?
driver = lines.find { |line| line =~ /^rusqlite\s*=/ }
abort "no rusqlite declaration in the fixture" if driver.nil?
moved.insert(index + 1, driver)
File.write(path, moved.join)
' "$DIR/crates/integration-tests/Cargo.toml"
run_gate "$DIR" case5
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "crates/integration-tests is test-only and may name the driver only under" "$WORK/case5.err"; then
  pass "a dev-dependency promoted to a production dependency fails, naming the section rule"
else
  fail "moving the driver out of [dev-dependencies] did not fail the gate (exit $STATUS)"
  cat "$WORK/case5.err" >&2
fi
echo ""

# --- Case 6: coverage comes from the member list, not from a glob ------------
# core-boundary.rb discovers dependents with Dir["crates/*/Cargo.toml"] plus
# core. A member declared anywhere else is invisible to it. The fixture puts the
# new member outside crates/ deliberately: a member under crates/ would be found
# by the defective method too, and the case would pass without proving anything.
echo "Case 6: a new workspace member outside crates/ is discovered"
DIR="$(new_case new-member)"
mkdir -p "$DIR/tools/probe/src"
cat > "$DIR/tools/probe/Cargo.toml" <<'TOML'
[package]
name = "fr136-probe"
version = "0.1.0"
edition = "2024"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
TOML
cat > "$DIR/tools/probe/src/lib.rs" <<'RUST'
pub fn probe(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tasks", [])?;
    Ok(())
}
RUST
ruby -e '
path = ARGV[0]
source = File.read(path)
source.sub!(/^(\s*)"crates\/slack-gateway",$/, "\\1\"crates/slack-gateway\",\n\\1\"tools/probe\",")
File.write(path, source)
' "$DIR/Cargo.toml"
if ! grep -q 'tools/probe' "$DIR/Cargo.toml"; then
  fail "the fixture did not add the member, so the discovery case proves nothing"
else
  run_gate "$DIR" case6
  # Both conditions must see it. Discovering the manifest and not the source
  # would leave the new member's SQL unread, which is the half of this defect
  # that produces no diagnostic at all.
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "tools/probe is a workspace member with no reviewed role" "$WORK/case6.err" &&
    grep -q "+ tools/probe/src/lib.rs has" "$WORK/case6.err"; then
    pass "a member outside crates/ is discovered from the member list, manifest and source both"
  else
    fail "a new workspace member outside crates/ was not fully discovered (exit $STATUS)"
    cat "$WORK/case6.err" >&2
  fi
fi
echo ""

# --- Case 7: SQL with no driver token at all ---------------------------------
# The case a manifest-only gate reports as clean. crates/daemon already declares
# the driver, so condition 1 has nothing to say; the added statement names no
# rusqlite path, so a driver-token inventory does not see it either. Only the
# per-file residual does.
#
# The mutation is chosen to be the one the implementation is least likely to
# catch: `conn.execute(sql, [])` is how this repository writes a statement when
# it has no parameters, and it needs no `rusqlite::` anywhere. Deleting a line or
# adding `use rusqlite::params` would both be caught by the check the author was
# already thinking about.
echo "Case 7: a SQL statement added with no driver token fails"
DIR="$(new_case sql-without-token)"
PROBE="$DIR/crates/daemon/src/protection.rs"
if grep -q 'rusqlite' "$PROBE"; then
  fail "the probe file already names the driver; the fixture would not isolate condition 2"
else
  # Written exactly the way the repository writes a parameterless statement, and
  # deliberately containing no `rusqlite` substring anywhere — not even inside
  # `tokio_rusqlite`, which would have handed condition 1 the token the case
  # exists to withhold.
  cat >> "$PROBE" <<'RUST'

pub async fn fr136_probe(state: &InnerState, id: String) -> anyhow::Result<()> {
    state
        .async_database
        .writer()
        .call(move |conn| {
            conn.execute("DELETE FROM tasks WHERE id = ?1", [&id])?;
            Ok(())
        })
        .await?;
    Ok(())
}
RUST
  if grep -q 'rusqlite' "$PROBE"; then
    fail "the fixture introduced a driver token; it no longer isolates condition 2"
  fi
  run_gate "$DIR" case7
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "+ crates/daemon/src/protection.rs has .* SQL statement" "$WORK/case7.err"; then
    pass "a SQL statement in a forbidden crate fails even with no rusqlite token in the file"
  else
    fail "a SQL statement with no driver token did not fail the gate (exit $STATUS)"
    cat "$WORK/case7.err" >&2
  fi
fi
echo ""

# --- Case 8: a decrease fails too --------------------------------------------
# The FR-128 case. Under a monotonic ratchet a decrease passes silently and the
# ledger goes on asserting debt the repository no longer carries — green, and
# false. Here a decrease is the goal, which is precisely why it has to be blessed
# rather than absorbed. The mutation removes rather than adds, because removal is
# the direction an author writing a ratchet does not have in mind.
echo "Case 8: a removed SQL statement also fails, so the ledger cannot go stale"
DIR="$(new_case sql-removed)"
# The statement this case neutralises used to sit in
# crates/orchestrator-scheduler/src/scheduler/task_state.rs. FR-141 B3 moved it
# into the layer, and the fixture pointed at a path that no longer had it — the
# case aborted with "no statement to neutralise" rather than failing loudly,
# which is a fixture naming its target rather than deriving it. The assertion is
# unchanged: exact equality holds in the decreasing direction too, whatever the
# file's role. Only the address moved.
TARGET="crates/orchestrator-persistence/src/scheduler_state.rs"
BEFORE_SQL=$(grep -c '"SELECT\|"INSERT\|"UPDATE\|"DELETE' "$DIR/$TARGET" || true)
ruby -e '
path = ARGV[0]
source = File.read(path)
# Neutralise one statement without touching the rusqlite token beside it, so the
# case moves the SQL count alone and the diagnostic is unambiguous.
abort "no statement to neutralise" unless source.sub!(/"SELECT COUNT\(\*\) FROM command_runs[^"]*"/m, "\"fr136 neutralised\"")
File.write(path, source)
' "$DIR/$TARGET"
AFTER_SQL=$(grep -c '"SELECT\|"INSERT\|"UPDATE\|"DELETE' "$DIR/$TARGET" || true)
if [[ "$BEFORE_SQL" -eq "$AFTER_SQL" ]]; then
  fail "the fixture did not remove a statement ($BEFORE_SQL -> $AFTER_SQL); the case is inert"
else
  run_gate "$DIR" case8
  TARGET_SQL=$(ruby -rjson -e 'print JSON.parse(File.read(ARGV[0]))["references"][ARGV[1]]["sql"]' "$DIR/$LEDGER" "$TARGET")
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "~ $TARGET sql $TARGET_SQL -> $((TARGET_SQL - 1))" "$WORK/case8.err"; then
    pass "a decrease fails too, and the report names the file and the direction it moved"
  else
    fail "removing a SQL statement did not fail the gate (exit $STATUS)"
    cat "$WORK/case8.err" >&2
  fi
fi
echo ""

# --- Case 9: --write refuses under CI ----------------------------------------
# The ledger's reviewed half is a decision. An unattended rewrite would turn the
# review into decoration, and this is the only barrier there is.
echo "Case 9: --write refuses to run unattended"
DIR="$(new_case ci-write)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
(cd "$DIR" && CI=1 ruby "$GATE" --emit-baseline --write > "$WORK/case9.out" 2> "$WORK/case9.err")
STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$STATUS" -ne 0 && "$BEFORE" == "$AFTER" ]] &&
  grep -q "refusing --write under CI" "$WORK/case9.err"; then
  pass "--write refuses under CI (exit $STATUS) and leaves the ledger untouched"
else
  fail "--write did not refuse under CI or modified the ledger (exit $STATUS)"
  cat "$WORK/case9.err" >&2
fi

# The other direction, so the guard is not simply "refuse always": CI=false is
# how a developer says "treat this as interactive", and the recovery path the
# ledger depends on has to remain usable. Every other indicator is cleared too —
# FR-134's version of this case ran green locally and failed in CI because the
# runner also exports GITHUB_ACTIONS and the guard was right to keep refusing.
DIR="$(new_case ci-write-false)"
BEFORE="$(digest "$DIR/$LEDGER")"
set +e
(cd "$DIR" && env -u CONTINUOUS_INTEGRATION -u GITHUB_ACTIONS -u GITLAB_CI \
    -u BUILDKITE -u CIRCLECI -u TEAMCITY_VERSION -u BUILD_NUMBER \
  CI=false ruby "$GATE" --emit-baseline --write > /dev/null 2> "$WORK/case9b.err")
STATUS=$?
set -e
AFTER="$(digest "$DIR/$LEDGER")"
if [[ "$STATUS" -eq 0 && "$BEFORE" == "$AFTER" ]]; then
  pass "CI=false is treated as interactive, and a no-op write still changes nothing"
else
  fail "CI=false was treated as unattended (exit $STATUS) or the write was not a no-op"
  cat "$WORK/case9b.err" >&2
fi
echo ""

# --- Case 10: the scanner is shared, not copied ------------------------------
# The ledger's numbers only mean what the scope says if this gate counts the tree
# the way core-boundary.rb does. A private copy free to drift would produce two
# reviewed states that both look correct — the reason scripts/lib/rust_source.rb
# exists. Requiring a pass first is what makes the later failure attributable to
# the removal rather than to a gate that was already broken.
echo "Case 10: the gate cannot run without the shared scanner"
DIR="$(new_case shared-scanner)"
run_gate "$DIR" case10-before
BEFORE_STATUS=$STATUS
rm "$DIR/scripts/lib/rust_source.rb"
run_gate "$DIR" case10
if [[ "$BEFORE_STATUS" -ne 0 ]]; then
  fail "the gate already failed with the shared scanner present, so its later failure proves nothing"
  cat "$WORK/case10-before.err" >&2
elif [[ "$STATUS" -ne 0 ]] && grep -q "rust_source" "$WORK/case10.err"; then
  pass "the gate passes with scripts/lib/rust_source.rb and fails without it; it holds no private copy"
else
  fail "the gate ran without the shared scanner (exit $STATUS)"
  cat "$WORK/case10.err" >&2
fi
echo ""

# --- Case 11: the ledger cannot outlive the scan it describes ----------------
# The scope prose and the implemented scan are two statements that can drift
# apart silently: the ledger would go on describing a measurement nobody makes.
echo "Case 11: a ledger whose scope prose does not match the scan fails"
DIR="$(new_case scope-drift)"
ruby -rjson -e '
path = ARGV[0]
ledger = JSON.parse(File.read(path))
ledger["scope"] = "every workspace member, scanned somehow"
File.write(path, JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER"
run_gate "$DIR" case11
if [[ "$STATUS" -ne 0 ]] && grep -q "ledger scope prose does not match" "$WORK/case11.err"; then
  pass "a ledger describing a scan the gate does not implement fails"
else
  fail "a drifted scope prose did not fail the gate (exit $STATUS)"
  cat "$WORK/case11.err" >&2
fi
echo ""

# --- Cases 12 to 17: FR-139 ---------------------------------------------------
# The scan's ruler and its reach. Cases 12 to 14 are about what counts as a SQL
# statement, 15 and 17 about what the scan reads, 16 about the one classification
# assertion that survived FR-139.
#
# Cases 12 to 14 all mutate a file that is ALREADY in the ledger with a known
# count. A new file would trip reference_errors and the unclassified branch at
# once, and the case could not say which assertion it exercised. Mutating a
# ledgered file leaves exactly one diagnostic: `~ <file> sql N -> N+1`.
# FR-141 moved every statement out of the forbidden crates, so the previous probe
# — crates/daemon/src/server/attention.rs — left the ledger entirely and these
# three cases mutated a file the gate reported as new rather than as changed.
# The count is now read from the ledger rather than restated here, so the next
# move relocates the probe without silently inverting what it proves.
PROBE_FILE="crates/orchestrator-persistence/src/audit_links.rs"
PROBE_SQL_BEFORE=$(ruby -rjson -e 'print JSON.parse(File.read(ARGV[0]))["references"][ARGV[1]]["sql"]' "$LEDGER" "$PROBE_FILE")

# --- Case 12: PRAGMA is a SQL statement --------------------------------------
# The verb FR-139 added. It matters because PRAGMA is how a crate configures the
# connection it was handed, which is condition 2's whole subject: before this,
# crates/orchestrator-security/src/lib.rs ran `PRAGMA foreign_keys = ON` on the
# orchestrator database and the ledger recorded that file as running one
# statement when it runs two.
echo "Case 12: a PRAGMA statement is counted"
DIR="$(new_case pragma-counted)"
cat >> "$DIR/$PROBE_FILE" <<'RUST'

pub fn fr139_pragma_probe(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}
RUST
run_gate "$DIR" case12
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "~ $PROBE_FILE sql $PROBE_SQL_BEFORE -> $((PROBE_SQL_BEFORE + 1))" "$WORK/case12.err"; then
  pass "a PRAGMA statement moves the per-file count and fails the gate"
else
  fail "a PRAGMA statement was not counted (exit $STATUS)"
  cat "$WORK/case12.err" >&2
fi
echo ""

# --- Case 13: a literal opening with an escape sequence ----------------------
# `"\n            SELECT …"` is `"`, backslash, `n` in the source text. An anchor
# of `"\s*` cannot step over the backslash, so this shape was a free bypass:
# reformat a statement across lines and it stops being counted. There are zero
# such literals on this tree, so the fixture is the only place the shape exists —
# which is the point of writing it before one appears rather than after.
echo "Case 13: a SQL literal opening with an escaped newline is counted"
DIR="$(new_case escaped-newline)"
cat >> "$DIR/$PROBE_FILE" <<'RUST'

pub fn fr139_escape_probe() -> &'static str {
    "\n            SELECT id FROM tasks WHERE id = ?1"
}
RUST
run_gate "$DIR" case13
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "~ $PROBE_FILE sql $PROBE_SQL_BEFORE -> $((PROBE_SQL_BEFORE + 1))" "$WORK/case13.err"; then
  pass "a statement hidden behind a leading escape sequence is counted"
else
  fail "an escaped-newline SQL literal was not counted (exit $STATUS)"
  cat "$WORK/case13.err" >&2
fi
echo ""

# --- Case 14: prose is not SQL, and the green is not vacuous -----------------
# The tempting repair for cases 12 and 13 is to relax the match — drop the
# uppercase requirement, or add VACUUM/BEGIN/COMMIT. Measured on this tree, a
# case-insensitive match reads 20 help strings in crates/cli/src/commands/guide.rs
# as SQL, and every VACUUM hit outside core is a log message. So "must not be
# counted" is asserted at the same strength as "must be counted".
#
# The two halves are one case on purpose. "The gate stayed green after I added
# prose" is also satisfied by the file never being read at all, which is a state
# this suite would consider broken. The second half appends one real statement to
# the SAME file and requires the count to move by exactly one — so the green in
# the first half is green about the prose, not about the location.
echo "Case 14: log prose is not counted, and the file it sits in is really scanned"
DIR="$(new_case prose-not-sql)"
# The last line is not prose in a string — it is an in-set verb, uppercase, in a
# comment with no quote before it. That covers the other way this could break:
# the two fixtures above answer "was the verb set widened", this one answers
# "was the opening-quote anchor dropped", and neither implies the other.
cat >> "$DIR/$PROBE_FILE" <<'RUST'

pub fn fr139_prose_probe(count: u64) -> Vec<String> {
    vec![
        format!("VACUUM complete: {count}"),
        "BEGIN the migration once the operator confirms".to_string(),
        "update the task and create a workspace, then delete the draft".to_string(),
        "Created index for the guide; DROPPED support is not implied".to_string(),
    ]
}

// SELECT, INSERT and DELETE name statements; PRAGMA configures a connection.
RUST
run_gate "$DIR" case14a
if [[ "$STATUS" -eq 0 ]]; then
  cat >> "$DIR/$PROBE_FILE" <<'RUST'

pub fn fr139_prose_control(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    Ok(())
}
RUST
  run_gate "$DIR" case14b
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "~ $PROBE_FILE sql $PROBE_SQL_BEFORE -> $((PROBE_SQL_BEFORE + 1))" "$WORK/case14b.err"; then
    pass "four prose strings count as zero statements, in a file one real statement proves is scanned"
  else
    fail "the control statement did not move the count, so case 14's green proved nothing (exit $STATUS)"
    cat "$WORK/case14b.err" >&2
  fi
else
  fail "log prose was read as SQL (exit $STATUS); the match has been relaxed"
  cat "$WORK/case14a.err" >&2
fi
echo ""

# --- Case 15: a build script is production source ----------------------------
# Condition 1 counts [build-dependencies] as a production declaration
# (persistence-dependency.rb, driver_declarations). Until FR-139 condition 2
# read only <member>/src, so the gate governed a build-time driver declaration
# whose only possible consumer it refused to open. Five members ship a build
# script and two of them — daemon and orchestrator-scheduler — are `forbidden`.
#
# The mutation withholds the driver token deliberately, the same choice case 7
# makes: `conn.execute(sql, [])` needs no rusqlite path, so a token inventory
# sees nothing and only the per-file SQL residual does.
echo "Case 15: SQL in a forbidden crate's build script fails"
DIR="$(new_case build-script)"
BUILD="$DIR/crates/daemon/build.rs"
if [[ ! -f "$BUILD" ]]; then
  fail "the fixture has no build script to mutate; new_case did not copy it"
elif grep -q 'rusqlite' "$BUILD"; then
  fail "the build script already names the driver; the fixture would not isolate condition 2"
else
  cat >> "$BUILD" <<'RUST'

fn fr139_build_probe(conn: &Connection) {
    let _ = conn.execute("DELETE FROM tasks", []);
}
RUST
  if grep -q 'rusqlite' "$BUILD"; then
    fail "the fixture introduced a driver token; it no longer isolates condition 2"
  fi
  run_gate "$DIR" case15
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "+ crates/daemon/build.rs has 0 driver reference(s) and 1 SQL statement(s)" "$WORK/case15.err"; then
    pass "a build script is scanned as production source, with no driver token in it"
  else
    fail "SQL in a forbidden crate's build script did not fail the gate (exit $STATUS)"
    cat "$WORK/case15.err" >&2
  fi
fi
echo ""

# --- Case 16: the classification assertion can fail --------------------------
# classification_errors used to have a second branch summing the categorised
# references and requiring the total to equal the scan. Both sides were the same
# reduction over the same hash, so no input could make it fail; FR-139 deleted
# it and this case is what the surviving branch owes in its place.
#
# The mutation deletes the `category` key rather than setting it to
# "unclassified". Setting the sentinel is the case the author had in mind; the
# absent key is the one a real edit produces, and it reaches the branch through
# the `|| "unclassified"` default instead of through the literal. The counts are
# left untouched so reference_errors stays silent and the diagnostic is
# unambiguously this branch's.
echo "Case 16: a scanned file with no reviewed category fails"
DIR="$(new_case no-category)"
ruby -rjson -e '
path = ARGV[0]
ledger = JSON.parse(File.read(path))
target = ARGV[1]
abort "the fixture target is not in the ledger" unless ledger["references"].key?(target)
ledger["references"][target].delete("category")
File.write(path, JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER" "crates/orchestrator-persistence/src/audit_links.rs"
run_gate "$DIR" case16
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "1 file(s) touch persistence with no reviewed category" "$WORK/case16.err" &&
  grep -q "crates/orchestrator-persistence/src/audit_links.rs" "$WORK/case16.err" &&
  ! grep -q "persistence touch points differ" "$WORK/case16.err"; then
  pass "a file whose category was dropped fails on the classification branch alone"
else
  fail "a scanned file with no reviewed category did not fail as expected (exit $STATUS)"
  cat "$WORK/case16.err" >&2
fi
echo ""

# --- Case 17: the reviewed root list is frozen -------------------------------
# The scope check compares the ledger's prose to the gate's constant — prose to
# prose. It agreed for the whole of FR-136 while the constant said "its non-test
# Rust source" and the walk read only <member>/src. scanRoots is the counterpart
# with a subject: the roots the walk just visited, frozen and compared both ways,
# so narrowing the scan produces a diff a reviewer reads rather than a smaller
# number nobody sees.
echo "Case 17: the reviewed scan-root list is frozen in both directions"
DIR="$(new_case scan-roots)"
ruby -rjson -e '
path = ARGV[0]
ledger = JSON.parse(File.read(path))
root = ARGV[1]
abort "the fixture root is not in scanRoots" unless (ledger["scanRoots"] || []).include?(root)
ledger["scanRoots"] -= [root]
File.write(path, JSON.pretty_generate(ledger) + "\n")
' "$DIR/$LEDGER" "crates/daemon/build.rs"
run_gate "$DIR" case17
if [[ "$STATUS" -ne 0 ]] &&
  grep -q "the roots this gate reads differ from the reviewed ledger" "$WORK/case17.err" &&
  grep -q "+ crates/daemon/build.rs is scanned and is not in the reviewed root list" "$WORK/case17.err"; then
  pass "a scan root missing from the reviewed list fails, naming the root"
else
  fail "a drifted scan-root list did not fail the gate (exit $STATUS)"
  cat "$WORK/case17.err" >&2
fi
echo ""

# --- Case 18: the build-script path is read from [package], not from anywhere --
# FR-139 read the `build` key with a whole-file regex, so any `build = "..."` in
# any table redirected the scan away from the real script — a dependency named
# `build`, a `[package.metadata.*]` table a tool defined for itself. scanRoots
# caught every form of it, which is what an outer freeze is for, but the reading
# itself was the mistake driver_declarations exists to avoid.
#
# Both directions, because a fix that simply stopped honouring `build` would pass
# the negative half and silently drop renamed scripts from the scan.
echo "Case 18: the build key is honoured in [package] and ignored elsewhere"
DIR="$(new_case build-key)"
DECOY_PKG="$DIR/crates/cli/Cargo.toml"
cat >> "$DECOY_PKG" <<'TOML'

[package.metadata.fr139-probe]
build = "nowhere.rs"
TOML
run_gate "$DIR" case18a
if [[ "$STATUS" -eq 0 ]]; then
  pass "a build key outside [package] does not redirect the scan"
else
  fail "a decoy build key outside [package] moved the scan (exit $STATUS)"
  cat "$WORK/case18a.err" >&2
fi

# The positive half. The script is renamed and [package] says so, so the scan has
# to follow it — and scanRoots has to show both ends of the move.
DIR="$(new_case build-key-rename)"
if [[ ! -f "$DIR/crates/daemon/build.rs" ]]; then
  fail "the fixture has no build script to rename; new_case did not copy it"
else
  mv "$DIR/crates/daemon/build.rs" "$DIR/crates/daemon/renamed_build.rs"
  ruby -e '
    path = ARGV[0]
    text = File.read(path)
    abort "the fixture manifest has no [package] table" unless text.include?("[package]")
    File.write(path, text.sub("[package]\n", "[package]\nbuild = \"renamed_build.rs\"\n"))
  ' "$DIR/crates/daemon/Cargo.toml"
  run_gate "$DIR" case18b
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "+ crates/daemon/renamed_build.rs is scanned and is not in the reviewed root list" "$WORK/case18b.err" &&
    grep -q "\- crates/daemon/build.rs is in the reviewed root list and is no longer scanned" "$WORK/case18b.err"; then
    pass "a renamed build script is followed, and both ends of the move are named"
  else
    fail "a renamed build script was not followed (exit $STATUS)"
    cat "$WORK/case18b.err" >&2
  fi
fi
echo ""

echo "FR-136 persistence dependency chokepoint: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
