#!/usr/bin/env bash
#
# FR-141 persistence API capability boundary — QA gate.
#
# Verifies that scripts/qa/persistence-api-boundary.rb holds the boundary rather
# than reporting on it. A gate observed only passing has not been observed doing
# anything, so most cases below apply a mutation it must reject — but four of
# them apply mutations it must NOT reject, because a check that fails on every
# edit is a ratchet, not a boundary.
#
# The cases that carry the argument, each named for the shape it defeats:
#
#   Case 4  a driver type in a DOC COMMENT does not fail. This is what a
#           `grep rusqlite` gate reports as a leak, and FR-134 spent its length
#           removing text-presence checks that could not tell the two apart.
#   Case 5  a driver type inside a STRING LITERAL does not fail, and the literal
#           carries `){` so an unmasked scanner would mis-terminate the signature
#           and swallow the item after it. RustLexer exists for this.
#   Case 6  `use rusqlite::Connection as Db;` and a signature naming `Db` DOES
#           fail. A gate matching the token `Connection` passes this mutation,
#           which is why the alias is read from the file's own use statements.
#   Case 7  a signature split across lines DOES fail. A per-line matcher sees
#           `conn:` and `&rusqlite::Connection` on different lines and finds
#           neither.
#   Case 8  a `pub fn` inside a PRIVATELY declared module is not public API and
#           is not reported. crates/orchestrator-persistence/src/task_repository
#           declares `mod items;` and `mod write_ops;` privately and re-exports
#           four of the seventeen public functions they define. A file-level
#           heuristic reports all seventeen and invents thirteen items for a
#           migration to move.
#   Case 9  the same function, re-exported by name, IS reported. Without it,
#           case 8's green is indistinguishable from "the gate skipped the file".
#   Case 10 a NEW connection-yielding function is looked for at call sites on the
#           same run that discovers it. Fact 3's scanned names are derived from
#           fact 1, not listed, so the coverage cannot lag the code by one audit.
#   Case 11 a cfg(test) module acquiring a connection does not count. The
#           boundary is about production code; counting tests would make the
#           ledger move whenever a test is added.
#   Case 12 an item added behind the `test-support` feature IS reported. The
#           obvious implementation skips a feature-gated module, and then
#           anything may be added there unobserved — §4.4's "a gate that
#           certifies enforcement it cannot observe is worse than no gate."
#   Case 13 enabling that feature from `[dependencies]` DOES fail. This is the
#           condition that makes case 12's bucket harmless, so it is asserted.
#   Case 14 the same feature under `[dev-dependencies]` does NOT fail. Without
#           it, case 13 is satisfied by a gate rejecting any manifest edit, and
#           the distinction the door rests on — which table — goes untested.
#   Case 15 a `pub(crate)` field of driver type is NOT public API. The field
#           regex accepted `pub(…)` where the item regex rejected it, so
#           `struct Migration` kept reporting as a leak after its `up` field
#           went crate-private. Found by disbelieving a count, not by review.
#   Case 16 a bare `pub` field of driver type IS reported, so case 15's green
#           cannot mean the struct body was never read.
#
# Safety: every mutation happens inside a temporary copy under $TMPDIR. The
# working tree is never written, nothing is compiled, no database is opened, and
# no provider is invoked.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/persistence-api-boundary.rb"
LEDGER="config/governance/persistence-api-boundary-ledger.json"
# Every shared library the gate requires. A library missing here makes the gate
# die on its require, and a case expecting failure then passes for the wrong
# reason.
GATE_LIBS=(
  scripts/lib/rust_source.rb
  scripts/lib/rust_lexer.rb
  scripts/lib/ci_env.rb
)

# The publicly reachable module every "add a leak" mutation lands in, and the
# privately declared one cases 8 and 9 use. Both are read from the repository
# rather than created, so a case exercises the real module graph.
PUBLIC_MODULE="crates/orchestrator-persistence/src/db_maintenance.rs"
PRIVATE_MODULE="crates/orchestrator-persistence/src/task_repository/write_ops.rs"
PRIVATE_PARENT="crates/orchestrator-persistence/src/task_repository/mod.rs"
CONSUMER="crates/cli/src/cli.rs"
# The feature-gated module. Read from the repository, so a case exercises the
# real cfg attribute on the real `mod` declaration rather than a synthetic one.
TEST_ONLY_MODULE="crates/orchestrator-persistence/src/test_support.rs"

for command in ruby; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr141-api-boundary.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# A case copies what the gate reads: the root manifest (its member list is the
# discovery source), core/src, every member's sources and build script, the
# ledger, the gate and its libraries. Copying the repository wholesale would
# drag in target/ and cost gigabytes per case.
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
  for lib in ${GATE_LIBS[@]+"${GATE_LIBS[@]}"}; do
    cp "$REPO_ROOT/$lib" "$dir/$lib"
  done
  echo "$dir"
}

# Appends text to a file inside a case directory. The text arrives as a single
# argument with \n escapes, and is written by ruby rather than by a heredoc:
# scripts/qa/bash32-compat.rb tracks heredoc state per line, and FR-138 recorded
# that an unterminated one silently truncates the scan of the rest of this file.
append() {
  local file="$1"
  # The target is checked here rather than trusted. `File.open(…, "a")` CREATES
  # a missing file, so a case whose target the tree has moved would mutate a
  # brand-new file the gate never reads, and then report the gate as having
  # missed the leak — pointing the auditor at the gate instead of at the
  # fixture. FR-143 catalogues nine such cases; that misdirection was the worst
  # of them. Returning 1 rather than exiting keeps `set -e` from turning a stale
  # fixture into a harness crash.
  if [[ ! -f "$file" ]]; then
    fail "the mutation target $file does not exist; this case has been asserting nothing"
    return 1
  fi
  ruby -e 'File.open(ARGV[0], "a") { |handle| handle.write(ARGV[1].gsub("\\n", "\n")) }' "$file" "$2"
}

# Proves a mutation landed. A case whose edit silently failed to apply reports
# the gate as green and blames the gate for it.
require_present() {
  local file="$1" needle="$2" label="$3"
  if ! grep -qF "$needle" "$file"; then
    fail "$label: the mutation did not land in $file"
    return 1
  fi
  return 0
}

run_gate() {
  local dir="$1" tag="$2"
  shift 2
  set +e
  (cd "$dir" && ruby "$GATE" "$@" > "$WORK/$tag.out" 2> "$WORK/$tag.err")
  STATUS=$?
  set -e
}

echo "FR-141 persistence API capability boundary"
echo ""

# --- Case 1: the gate passes on the repository -------------------------------
echo "Case 1: the gate holds on the working tree"
run_gate "$REPO_ROOT" case1
if [[ "$STATUS" -eq 0 ]] && grep -q "Persistence API boundary: PASS" "$WORK/case1.out"; then
  pass "the persistence API boundary gate passes on the repository"
else
  fail "the persistence API boundary gate does not pass on the repository (exit $STATUS)"
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
  diff "$REPO_ROOT/$LEDGER" "$WORK/emitted.json" | sed -n '1,20p' >&2
fi
echo ""

# --- Case 3: a new public item returning a connection fails ------------------
echo "Case 3: a public item whose return type names a connection fails"
DIR="$(new_case yields-return)"
if append "$DIR/$PUBLIC_MODULE" '\n/// Hands out the write connection.\npub fn fr141_borrow(db: &rusqlite::Connection) -> &rusqlite::Connection {\n    db\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "fr141_borrow(db: &rusqlite::Connection)" "case 3"; then
  run_gate "$DIR" case3
  if [[ "$STATUS" -ne 0 ]] && grep -q "fn fr141_borrow" "$WORK/case3.err"; then
    pass "a public fn returning a connection fails, and the report names the item"
  else
    fail "a public fn returning a connection did not fail the gate (exit $STATUS)"
    cat "$WORK/case3.err" >&2
  fi
fi
echo ""

# --- Case 4: a driver type in a doc comment does NOT fail --------------------
# The mutation a text-presence gate cannot survive. It is written as a comment
# rather than deleted-and-restored because a comment is the case the author of a
# grep did not have in mind.
echo "Case 4: a driver type mentioned only in a doc comment does NOT fail"
DIR="$(new_case doc-comment-only)"
if append "$DIR/$PUBLIC_MODULE" '\n/// Callers used to pass a rusqlite::Connection here; see tokio_rusqlite::Connection.\n/// pub fn fr141_commented(conn: &rusqlite::Connection) -> &rusqlite::Connection { conn }\npub fn fr141_documented() -> u64 {\n    0\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "pass a rusqlite::Connection here" "case 4"; then
  run_gate "$DIR" case4
  if [[ "$STATUS" -eq 0 ]]; then
    pass "a driver type in a doc comment is not a leak, and the gate says so"
  else
    fail "a doc comment mentioning the driver failed the gate (exit $STATUS)"
    cat "$WORK/case4.err" >&2
  fi
fi
echo ""

# --- Case 5: a driver type inside a string literal does NOT fail -------------
# The literal carries `){` so a scanner that did not mask it would end the
# signature in the wrong place and read the following item's text as part of
# this one.
echo "Case 5: a driver type inside a string literal does NOT fail"
DIR="$(new_case string-literal-only)"
if append "$DIR/$PUBLIC_MODULE" '\npub fn fr141_literal() -> String {\n    String::from("){ -> &rusqlite::Connection tokio_rusqlite::Connection")\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "){ -> &rusqlite::Connection" "case 5"; then
  run_gate "$DIR" case5
  if [[ "$STATUS" -eq 0 ]]; then
    pass "a driver type inside a string literal is not a leak, and the literal did not derail the scan"
  else
    fail "a string literal naming the driver failed the gate (exit $STATUS)"
    cat "$WORK/case5.err" >&2
  fi
fi
echo ""

# --- Case 6: a renamed import in a signature fails ---------------------------
echo "Case 6: a driver type imported under another name DOES fail"
DIR="$(new_case renamed-import)"
if append "$DIR/$PUBLIC_MODULE" '\nuse rusqlite::Connection as Fr141Db;\n\n/// Takes the driver connection under a local name.\npub fn fr141_renamed(handle: &Fr141Db) -> u64 {\n    let _ = handle;\n    0\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "use rusqlite::Connection as Fr141Db;" "case 6"; then
  run_gate "$DIR" case6
  if [[ "$STATUS" -ne 0 ]] && grep -q "fn fr141_renamed" "$WORK/case6.err"; then
    pass "an aliased driver import in a signature fails, so the check reads the use statement and not the token"
  else
    fail "an aliased driver import did not fail the gate (exit $STATUS)"
    cat "$WORK/case6.err" >&2
  fi
fi
echo ""

# --- Case 7: a signature split across lines fails ----------------------------
echo "Case 7: a driver type on its own line in a split signature DOES fail"
DIR="$(new_case multiline-signature)"
if append "$DIR/$PUBLIC_MODULE" '\n/// Signature deliberately spread over several lines.\npub fn fr141_spread(\n    conn:\n        &rusqlite::Connection,\n    label: &str,\n) -> u64 {\n    let _ = (conn, label);\n    0\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "        &rusqlite::Connection," "case 7"; then
  run_gate "$DIR" case7
  if [[ "$STATUS" -ne 0 ]] && grep -q "fn fr141_spread" "$WORK/case7.err"; then
    pass "a multi-line signature fails, so the check matches by bracket and not by line"
  else
    fail "a multi-line signature did not fail the gate (exit $STATUS)"
    cat "$WORK/case7.err" >&2
  fi
fi
echo ""

# --- Case 8: a pub fn in a privately declared module is not public API -------
echo "Case 8: a pub fn in a privately declared module does NOT fail"
DIR="$(new_case private-module)"
if append "$DIR/$PRIVATE_MODULE" '\n/// Not reachable from outside the crate: the module is declared `mod`.\npub fn fr141_private(conn: &Connection) -> u64 {\n    let _ = conn;\n    0\n}\n' &&
  require_present "$DIR/$PRIVATE_MODULE" "fr141_private(conn: &Connection)" "case 8"; then
  run_gate "$DIR" case8
  if [[ "$STATUS" -eq 0 ]]; then
    pass "a pub fn behind a private module is not public API, and the gate does not report it"
  else
    fail "a pub fn behind a private module was reported as public API (exit $STATUS)"
    cat "$WORK/case8.err" >&2
  fi
fi
echo ""

# --- Case 9: the same function, re-exported, IS public API -------------------
# Pairs with case 8. Without this, case 8's green would be equally consistent
# with the gate never opening the file.
echo "Case 9: the same function re-exported by name DOES fail"
DIR="$(new_case private-module-reexported)"
if append "$DIR/$PRIVATE_MODULE" '\n/// Re-exported below, which makes it crate-external.\npub fn fr141_private(conn: &Connection) -> u64 {\n    let _ = conn;\n    0\n}\n' &&
  append "$DIR/$PRIVATE_PARENT" '\npub use write_ops::fr141_private;\n' &&
  require_present "$DIR/$PRIVATE_MODULE" "fr141_private(conn: &Connection)" "case 9" &&
  require_present "$DIR/$PRIVATE_PARENT" "pub use write_ops::fr141_private;" "case 9"; then
  run_gate "$DIR" case9
  if [[ "$STATUS" -ne 0 ]] && grep -q "fn fr141_private" "$WORK/case9.err"; then
    pass "a re-exported function from a private module is public API, so case 8 is about visibility and not about a skipped file"
  else
    fail "a re-exported function from a private module was not reported (exit $STATUS)"
    cat "$WORK/case9.err" >&2
  fi
fi
echo ""

# --- Case 10: call sites are derived from the yields set, not listed ---------
# The enumeration shape, closed. A new way to obtain a connection is looked for
# at call sites on the same run that discovers it, so the covered set cannot lag
# the code by one audit round.
echo "Case 10: a newly discovered connection-yielding name is scanned for at call sites"
DIR="$(new_case derived-coverage)"
if append "$DIR/$PUBLIC_MODULE" '\n/// A second way to obtain the connection, unknown to any list.\npub fn fr141_lend(db: &rusqlite::Connection) -> &rusqlite::Connection {\n    db\n}\n' &&
  append "$DIR/$CONSUMER" '\nfn fr141_consumer(db: &orchestrator_persistence::db_maintenance::Holder) {\n    let _ = fr141_lend(db);\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "fr141_lend(db: &rusqlite::Connection)" "case 10" &&
  require_present "$DIR/$CONSUMER" "fr141_lend(db)" "case 10"; then
  run_gate "$DIR" case10
  if [[ "$STATUS" -ne 0 ]] &&
    grep -q "fn fr141_lend" "$WORK/case10.err" &&
    grep -q "crates/cli/src/cli.rs" "$WORK/case10.err"; then
    pass "the new yielding name is reported, and its call site in a forbidden crate is reported with it"
  else
    fail "a newly discovered yielding name was not scanned for at call sites (exit $STATUS)"
    cat "$WORK/case10.err" >&2
  fi
fi
echo ""

# --- Case 11: a cfg(test) acquisition does not count -------------------------
echo "Case 11: a connection acquired inside a cfg(test) module does NOT fail"
DIR="$(new_case test-module-acquisition)"
if append "$DIR/$CONSUMER" '\n#[cfg(test)]\nmod fr141_tests {\n    #[test]\n    fn borrows_a_connection() {\n        let database = super::fake_database();\n        let _ = database.writer();\n        let _ = database.reader();\n    }\n}\n' &&
  require_present "$DIR/$CONSUMER" "database.writer()" "case 11"; then
  run_gate "$DIR" case11
  if [[ "$STATUS" -eq 0 ]]; then
    pass "a cfg(test) module acquiring a connection is outside the production scan"
  else
    fail "a cfg(test) acquisition moved the ledger (exit $STATUS)"
    cat "$WORK/case11.err" >&2
  fi
fi
echo ""

# --- Case 12: the test-only door is inventoried, not skipped -----------------
# The failure this case exists for is the one §4.4 names directly: a gate that
# declines to look at an exempted region certifies an exemption it cannot
# observe. `test_support` is gated on a feature, so the obvious implementation
# skips it — and then anything at all could be added there, including a second
# `open_conn` with no gate and no ledger entry. The mutation adds an item the
# module does not have and expects the gate to name it.
echo "Case 12: a new item behind the test-only feature is reported, not skipped"
DIR="$(new_case test-only-inventoried)"
if append "$DIR/$TEST_ONLY_MODULE" '\n/// Hands out a second connection.\npub fn fr141_backdoor(path: &std::path::Path) -> anyhow::Result<rusqlite::Connection> {\n    open_conn(path)\n}\n' &&
  require_present "$DIR/$TEST_ONLY_MODULE" "fr141_backdoor(path: &std::path::Path) -> anyhow::Result<rusqlite::Connection>" "case 12"; then
  run_gate "$DIR" case12
  if [[ "$STATUS" -ne 0 ]] && grep -q "test-only public items yielding a driver type" "$WORK/case12.err" &&
    grep -q "fn fr141_backdoor" "$WORK/case12.err"; then
    pass "an item added behind the feature is reported under the test-only inventory"
  else
    fail "an item added behind the test-only feature was skipped rather than reported (exit $STATUS)"
    cat "$WORK/case12.err" >&2
  fi
fi
echo ""

# --- Case 13: enabling the feature from [dependencies] fails -----------------
# This is the condition that makes case 12's bucket harmless, so it is asserted
# rather than assumed. The mutation is the one the implementation is least
# likely to catch: not a new dependency, but the SAME crate that already
# declares orchestrator-persistence gaining the feature on its production edge —
# a gate searching the manifest for the feature's name finds it either way,
# because crates/daemon lists it under [dev-dependencies] on the line below.
echo "Case 13: enabling the test-only feature from [dependencies] fails"
DIR="$(new_case feature-on-production-edge)"
if append "$DIR/crates/cli/Cargo.toml" '\n[dependencies.orchestrator-persistence]\npath = "../orchestrator-persistence"\nfeatures = ["test-support"]\n' &&
  require_present "$DIR/crates/cli/Cargo.toml" 'features = ["test-support"]' "case 13"; then
  run_gate "$DIR" case13
  if [[ "$STATUS" -ne 0 ]] && grep -q "enables the test-only feature" "$WORK/case13.err" &&
    grep -q "not \[dev-dependencies\]" "$WORK/case13.err"; then
    pass "a production edge enabling the test-only feature fails, and the report names the file"
  else
    fail "the test-only feature on a production dependency edge did not fail (exit $STATUS)"
    cat "$WORK/case13.err" >&2
  fi
fi
echo ""

# --- Case 14: the same declaration under [dev-dependencies] does NOT fail ----
# Case 13's red is worth nothing without this. Without it the case is satisfied
# by a gate that rejects any manifest edit, or any mention of the feature
# anywhere, and the distinction the door depends on — which table the edge is
# in — would be untested.
echo "Case 14: the same feature under [dev-dependencies] does NOT fail"
DIR="$(new_case feature-on-dev-edge)"
if append "$DIR/crates/cli/Cargo.toml" '\n[dev-dependencies.orchestrator-persistence]\npath = "../orchestrator-persistence"\nfeatures = ["test-support"]\n' &&
  require_present "$DIR/crates/cli/Cargo.toml" 'features = ["test-support"]' "case 14"; then
  run_gate "$DIR" case14
  if [[ "$STATUS" -eq 0 ]]; then
    pass "the sanctioned [dev-dependencies] edge passes, so case 13 tests the table and not the name"
  else
    fail "a [dev-dependencies] edge enabling the test-only feature was rejected (exit $STATUS)"
    cat "$WORK/case14.err" >&2
  fi
fi
echo ""

# --- Case 15: a pub(crate) field is not public API ---------------------------
# The mutation the implementation was least likely to catch, because it did not:
# the field regex accepted `pub(…)` where the item regex rejected it, so
# `struct Migration` kept reporting as yielding a connection after FR-141 B5a
# made its `up` field crate-private. Paired with case 16 so a green here cannot
# mean the struct body was never read.
echo "Case 15: a pub(crate) field of driver type is NOT public API"
DIR="$(new_case crate-private-field)"
if append "$DIR/$PUBLIC_MODULE" '\n/// A handle whose connection is crate-private.\npub struct Fr141Handle {\n    pub(crate) conn: rusqlite::Connection,\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "pub(crate) conn: rusqlite::Connection," "case 15"; then
  run_gate "$DIR" case15
  if [[ "$STATUS" -eq 0 ]]; then
    pass "a pub(crate) field of driver type does not count as public API"
  else
    fail "a pub(crate) field was reported as public API (exit $STATUS)"
    cat "$WORK/case15.err" >&2
  fi
fi
echo ""

# --- Case 16: a bare pub field of driver type IS public API ------------------
echo "Case 16: a bare pub field of driver type IS reported"
DIR="$(new_case public-field)"
if append "$DIR/$PUBLIC_MODULE" '\n/// A handle that hands its connection out.\npub struct Fr141Handle {\n    pub conn: rusqlite::Connection,\n}\n' &&
  require_present "$DIR/$PUBLIC_MODULE" "pub conn: rusqlite::Connection," "case 16"; then
  run_gate "$DIR" case16
  if [[ "$STATUS" -ne 0 ]] && grep -q "struct Fr141Handle" "$WORK/case16.err"; then
    pass "a bare pub field of driver type is reported, so case 15 tests the visibility"
  else
    fail "a public field of driver type was not reported (exit $STATUS)"
    cat "$WORK/case16.err" >&2
  fi
fi
echo ""

echo "=== persistence API boundary: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]]
