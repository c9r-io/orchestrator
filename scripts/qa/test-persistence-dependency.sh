#!/usr/bin/env bash
#
# FR-136 persistence dependency chokepoint — QA gate.
#
# Verifies that scripts/qa/persistence-dependency.rb actually holds the decision
# rather than reporting on it. A gate observed only passing has not been observed
# doing anything, so every case below applies a mutation it must reject — and
# case 4 applies one it must NOT reject, because a rule that fails on every
# change is a ratchet, not a policy.
#
# The two cases that carry the argument:
#
#   Case 4  an exempt crate adds a driver declaration and the gate stays green.
#           Without it, "the gate fails when I touch a manifest" is indistinguishable
#           from "the gate enforces a per-crate chokepoint".
#   Case 7  a forbidden crate gains a SQL statement and no rusqlite token at all.
#           This is the state a manifest-only gate reports as clean, and it is not
#           hypothetical: crates/orchestrator-security/src/secret_store_crypto.rs
#           runs four production SQL statements with zero driver references today.
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
# discovery source), core/src, every member's manifest and sources, the ledger,
# the gate and its libraries. Copying the repository wholesale would drag in
# target/ and cost gigabytes per case.
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
    [[ -f "$crate/Cargo.toml" ]] && cp "$crate/Cargo.toml" "$dir/crates/$name/Cargo.toml"
    [[ -d "$crate/src" ]] && cp -R "$crate/src" "$dir/crates/$name/src"
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
TARGET="crates/orchestrator-scheduler/src/scheduler/task_state.rs"
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
  if [[ "$STATUS" -ne 0 ]] && grep -q "~ $TARGET sql 8 -> 7" "$WORK/case8.err"; then
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

echo "FR-136 persistence dependency chokepoint: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
