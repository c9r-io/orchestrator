#!/usr/bin/env bash
#
# FR-143 fixture target drift — QA gate for scripts/lib/gate_fixture.sh and
# scripts/qa/fixture-target-drift.rb.
#
# The subject is a fixture that reports without proving. Nine of those are on
# record, eight of them green, and the worst did not merely go quiet: it
# announced that the gate under test had failed to notice a removal nobody had
# made, and sent the auditor to read the gate.
#
# Two things are asserted, and they are different questions:
#
#   the library   a stale premise costs one failed assertion and the run
#                 continues; a mutation that does not land is a failure and
#                 says so; a mutation that does land is not.
#   the scanner   the shapes cannot come back, and it is a parse rather than a
#                 grep — the design record, this file and the QA document all
#                 have to write the forbidden shapes down.
#
# Every rule the scanner defines has a case here, and every "must fire" case is
# paired with a "must not fire" one. A rule that flags correct code gets
# switched off long before it catches anything, and a scanner that fires on
# everything has the same green record as one that detects drift.
#
# Safety: read-only against the working tree. Every case builds a scratch tree
# under $TMPDIR. No daemon starts, no database is touched, no provider is
# invoked, and nothing reaches the network.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCANNER="scripts/qa/fixture-target-drift.rb"
LIBRARY="scripts/lib/gate_fixture.sh"
SURFACE="config/governance/qa-gate-surface.json"

for command in ruby shasum mktemp awk; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr143-fixture-drift.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/$LIBRARY"

echo "=== FR-143: fixture target drift ==="
echo ""

# ── Part 1: the library ─────────────────────────────────────────────────────
#
# Driven through a child bash rather than by sourcing here, because half of what
# is under test is what happens to the *run*: an abort takes the summary line
# with it, and a run that stopped early is indistinguishable from one that
# finished. That is only observable from outside.

harness() {
  local name="$1" body="$2"
  local dir="$WORK/$name"
  mkdir -p "$dir"
  cp "$REPO_ROOT/$LIBRARY" "$dir/gate_fixture.sh"
  {
    echo '#!/usr/bin/env bash'
    echo 'set -euo pipefail'
    # The bodies below write to $PWD. Without this the child runs in the
    # repository root and the cases leave db.rs, agent.yaml and target.txt in
    # the working tree — a fixture doing something other than what its own
    # safety paragraph says, which is this gate's whole subject. Caught by
    # `git status` after the first run, not by reading the script.
    echo "cd \"$dir\""
    echo 'PASS=0; FAIL=0'
    echo 'pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }'
    echo 'fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }'
    echo ". \"$dir/gate_fixture.sh\""
    echo "$body"
    echo 'echo "harness: $PASS passed, $FAIL failed"'
    echo '[[ "$FAIL" -eq 0 ]]'
  } > "$dir/run.sh"
  set +e
  bash "$dir/run.sh" > "$dir/log" 2>&1
  HARNESS_STATUS=$?
  set -e
  HARNESS_LOG="$dir/log"
}

# 1. A premise that no longer holds is one failed assertion, and the run
#    survives it. This is incidents 4, 8 and 9: `abort "no statement to
#    neutralise"`, a category stripped from a file that had left the ledger, and
#    an empty `git log --grep` counted as a pass.
harness premise-stale '
printf "hello\n" > "$PWD/target.txt"
if fixture_premise "case A" ruby -e "abort %q(the anchor this fixture expects is gone)"; then
  pass "case A: ran"
else
  echo "  (case A skipped its assertion, as it should)"
fi
pass "the case after it still ran"
'
if [[ "$HARNESS_STATUS" -ne 0 ]] &&
  grep -q "the fixture's premise no longer holds" "$HARNESS_LOG" &&
  grep -q "the anchor this fixture expects is gone" "$HARNESS_LOG" &&
  grep -q "the case after it still ran" "$HARNESS_LOG" &&
  grep -q "harness: 1 passed, 1 failed" "$HARNESS_LOG"; then
  pass "a stale premise fails the case, quotes the premise's own words, and the run continues to its summary"
else
  fail "a stale premise did not fail cleanly (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi

# 2. A premise that holds costs nothing. Without this, "fails on every premise"
#    and "detects a stale premise" have the same green record.
harness premise-holds '
if fixture_premise "case B" ruby -e "exit 0"; then
  pass "case B: the premise holds"
fi
'
if [[ "$HARNESS_STATUS" -eq 0 ]] && grep -q "harness: 1 passed, 0 failed" "$HARNESS_LOG"; then
  pass "a premise that still holds does not fail the case"
else
  fail "a holding premise was reported as broken (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi

# 3. THE RECORDED INCIDENT. A substitution that matches nothing.
#
#    This is the second of the nine and the worst: core/src/db.rs had become a
#    re-export shell whose only rusqlite token sat inside `mod tests`, where the
#    scanner does not count it. The mutation mutated nothing, the gate correctly
#    reported no change, and the fixture reported that the gate had failed to
#    notice a removal.
#
#    The mutation is a substitution whose pattern no longer matches, not a
#    deleted file. Deletion is the case the author has in mind and it already
#    fails loudly; a substitution matching nothing is the one that reports.
harness mutation-inert '
printf "pub use crate::persistence::db::*;\n" > "$PWD/db.rs"
if fixture_mutate "case 5" "$PWD/db.rs" ruby -e "
  path = ARGV[0]
  File.write(path, File.read(path).gsub(/rusqlite::Connection/, %q()))
" "$PWD/db.rs"; then
  fail "case 5: the gate did not notice the removal"
fi
'
if [[ "$HARNESS_STATUS" -ne 0 ]] &&
  grep -q "the mutation did not apply to" "$HARNESS_LOG" &&
  grep -q "db.rs" "$HARNESS_LOG" &&
  grep -q "the fixture proves nothing" "$HARNESS_LOG" &&
  ! grep -q "the gate did not notice the removal" "$HARNESS_LOG"; then
  pass "a substitution that matches nothing fails naming the file, and the accusation against the gate never prints"
else
  fail "an inert mutation was not caught (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi

# 4. The other direction: a mutation that lands is not a finding.
harness mutation-lands '
printf "maxTurns: 6\n" > "$PWD/agent.yaml"
if fixture_mutate "case 6" "$PWD/agent.yaml" ruby -e "
  path = ARGV[0]
  File.write(path, File.read(path).sub(%q(maxTurns: 6), %q(maxTurns: 9)))
" "$PWD/agent.yaml"; then
  pass "case 6: the mutation landed"
fi
'
if [[ "$HARNESS_STATUS" -eq 0 ]] && grep -q "harness: 1 passed, 0 failed" "$HARNESS_LOG"; then
  pass "a mutation that lands is not reported, so the check is about the change and not about the edit"
else
  fail "a landing mutation was reported as inert (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi

# 5. A target that is not a regular file. Incident 3: core-boundary case 5 read
#    its removal target from a ledger map FR-141 B4 had emptied, the read
#    returned the empty string, and the case wrote to a directory.
harness mutation-directory '
mkdir -p "$PWD/somewhere"
if fixture_mutate "case 7" "$PWD/somewhere" ruby -e "File.write(ARGV[0], %q(x))" "$PWD/somewhere"; then
  fail "case 7: the gate accepted it"
fi
'
if [[ "$HARNESS_STATUS" -ne 0 ]] &&
  grep -q "is not a regular file" "$HARNESS_LOG" &&
  grep -q "the mutation had nowhere to land" "$HARNESS_LOG"; then
  pass "a target that is a directory fails before the mutation runs, which is what an emptied ledger read produces"
else
  fail "a directory target was not caught (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi

# 6. A producer that leaves nothing behind. Zero bytes and a correct derivation
#    are indistinguishable from the exit code alone — the FR-144 lesson, one
#    layer over.
harness produce-empty '
if fixture_produce "case 8" "$PWD/derived.yaml" ruby -e "File.write(ARGV[0], %q())" "$PWD/derived.yaml"; then
  fail "case 8: accepted an empty derivation"
fi
if fixture_produce "case 8b" "$PWD/real.yaml" ruby -e "File.write(ARGV[0], %q(kind: Workflow))" "$PWD/real.yaml"; then
  pass "case 8b: a non-empty derivation is accepted"
fi
'
if [[ "$HARNESS_STATUS" -ne 0 ]] &&
  grep -q "the producer left nothing at" "$HARNESS_LOG" &&
  grep -q "examines nothing" "$HARNESS_LOG" &&
  grep -q "harness: 1 passed, 1 failed" "$HARNESS_LOG"; then
  pass "an empty derivation fails and a non-empty one passes, so the rule is about the content and not the command"
else
  fail "the producer contract did not hold (exit $HARNESS_STATUS)"
  cat "$HARNESS_LOG" >&2
fi
echo ""

# ── Part 2: the scanner ─────────────────────────────────────────────────────

CASE="$WORK/scan"
mkdir -p "$CASE/scripts/qa" "$CASE/scripts/lib" "$CASE/config/governance"
cp "$REPO_ROOT/$SCANNER" "$CASE/$SCANNER"
cp "$REPO_ROOT/scripts/lib/shell_lexer.rb" "$CASE/scripts/lib/shell_lexer.rb"

# The scanner exits non-zero exactly when it finds something, so `scan | grep -q`
# would invert under `set -o pipefail`: a successful match reads as failure.
# Captured instead.
scan() { (cd "$CASE" && ruby "$SCANNER" 2>&1) || true; }

# Routed through the library this gate exists to enforce. That is not decoration:
# this file is registered ci-required, so the scanner reads it, and it reported
# this very function on its first run. A gate exempt from its own rule is the
# shape FR-134 named — enforcement that exists and does not apply to itself.
register() {
  fixture_mutate "register ${1##*/}" "$CASE/$SURFACE" ruby -rjson -e '
    path, script = ARGV
    doc = JSON.parse(File.read(path))
    doc["scripts"] << { "path" => script, "enforcement" => "ci-required" }
    File.write(path, JSON.pretty_generate(doc) + "\n")
  ' "$CASE/$SURFACE" "$1"
}

printf '{\n  "scripts": []\n}\n' > "$CASE/$SURFACE"

probe() {
  local name="$1"
  cat > "$CASE/scripts/qa/$name"
  chmod +x "$CASE/scripts/qa/$name"
  register "scripts/qa/$name"
}

# 7. Positive control — a surface with one clean gate, deliberately not an empty
#    one. An empty surface used to be this control, and the closure self-check
#    found that it made the control pass on a scanner that examined nothing:
#    §4.4 shape 5, in the gate written directly after the FR about that shape.
#    Case 7b is the assertion that closed it.
probe test-clean.sh <<'SH'
#!/usr/bin/env bash
DIR="$(new_case control)"
if fixture_mutate "case 1" "$DIR/config/ledger.json" ruby -e 'File.write(ARGV[0], "{}")' "$DIR/config/ledger.json"; then
  run_gate "$DIR" c
  if [[ "$STATUS" -ne 0 ]] && grep -q "the ledger is empty" "$WORK/c.err"; then
    pass "case 1"
  fi
fi
cat > "$DIR/notes.txt" <<'DOC'
a here-document that closes
DOC
SH
OUT="$(scan)"
if [[ -z "$(grep '\[' <<< "$OUT")" ]]; then
  pass "positive control: a correctly written gate produces no findings"
else
  fail "the scanner reported findings against a correctly written gate"
  echo "$OUT" >&2
fi

# 7b. And an empty scan is a failure, not a clean run. Zero gates scanned and
#     twenty-eight gates scanned clean are the same exit code otherwise.
EMPTY="$WORK/empty"
mkdir -p "$EMPTY/scripts/qa" "$EMPTY/scripts/lib" "$EMPTY/config/governance"
cp "$REPO_ROOT/$SCANNER" "$EMPTY/$SCANNER"
cp "$REPO_ROOT/scripts/lib/shell_lexer.rb" "$EMPTY/scripts/lib/shell_lexer.rb"
printf '{\n  "scripts": []\n}\n' > "$EMPTY/$SURFACE"
EMPTY_OUT="$( (cd "$EMPTY" && ruby "$SCANNER" 2>&1) || true)"
(cd "$EMPTY" && ruby "$SCANNER" >/dev/null 2>&1) && EMPTY_STATUS=0 || EMPTY_STATUS=$?
if [[ "$EMPTY_STATUS" -ne 0 ]] && grep -q "yielded no ci-required shell gates" <<< "$EMPTY_OUT" &&
  grep -q "examined nothing" <<< "$EMPTY_OUT"; then
  pass "a manifest that yields no gates fails rather than reporting a clean scan of nothing"
else
  fail "the scanner reported PASS having examined nothing (exit $EMPTY_STATUS)"
  echo "$EMPTY_OUT" >&2
fi

# 8. unproven-mutation, both directions in one probe: the unwrapped rewrite is a
#    finding at its own line, and the wrapped one beside it is not.
probe test-unproven.sh <<'SH'
#!/usr/bin/env bash
DIR="$(new_case alpha)"
ruby -e 'File.write(ARGV[0], "x")' "$DIR/config/ledger.json"
if fixture_mutate "case 2" "$DIR/config/other.json" ruby -e 'File.write(ARGV[0], "y")' "$DIR/config/other.json"; then
  pass "case 2"
fi
SH
OUT="$(scan)"
if grep -q "test-unproven.sh:3: \[unproven-mutation\]" <<< "$OUT" &&
  ! grep -q "test-unproven.sh:4:" <<< "$OUT"; then
  pass "an unwrapped in-place rewrite is a finding naming its line, and a wrapped one on the next line is not"
else
  fail "unproven-mutation did not discriminate wrapped from unwrapped"
  echo "$OUT" >&2
fi

# 9. Running the gate under test is not mutating the tree.
#
#    `(cd "$DIR" && ruby "$GATE")` is how every one of these gates invokes its
#    subject. Without the `-e` requirement the rule matches it, and reports 96
#    findings on this repository where there are 43. A scanner reporting defects
#    that are not there is worse than the silence it replaces.
probe test-invocation.sh <<'SH'
#!/usr/bin/env bash
DIR="$(new_case beta)"
(cd "$DIR" && ruby "$GATE" > "$WORK/out" 2> "$WORK/err")
BEFORE="$(digest "$DIR/config/ledger.json")"
COUNT=$(ruby -rjson -e 'print JSON.parse(File.read(ARGV[0]))["n"]' "$DIR/config/ledger.json")
SH
OUT="$(scan)"
if ! grep -q "test-invocation.sh" <<< "$OUT"; then
  pass "running the gate against a fixture tree, and reading a value out of one, are not mutations"
else
  fail "the scanner read an invocation or a value capture as a mutation"
  echo "$OUT" >&2
fi

# 10. aborting-premise, and what separates it from a grep for the word.
#
#     The word appears four times in this probe and only one of them is the
#     defect: a shell comment, a here-document body, a Ruby comment inside the
#     block, and the real premise. DD-155, the QA document and this file all
#     have to write the forbidden shape down, and a grep would flag every one of
#     them — after which the natural way to silence it is to stop writing the
#     rule down.
#
#     The finding is anchored to the line the block opens on, not to the line
#     the abort sits on, because that is where the wrapper has to go. The abort
#     line is named in the message.
probe test-aborting.sh <<'SH'
#!/usr/bin/env bash
# A fixture must not abort "the premise is gone" — it must fail the case.
DIR="$(new_case gamma)"
cat > "$DIR/notes.txt" <<'DOC'
abort "this is prose inside a here-document, not code"
DOC
ruby -e '
  path = ARGV[0]
  text = File.read(path)
  # abort "this line is a Ruby comment and is not the premise"
  abort "the fixture anchor is missing" unless text.include?("anchor")
  File.write(path, text.sub("anchor", "moved"))
' "$DIR/notes.txt"
SH
OUT="$(scan)"
# Counted per rule, not per file: line 7 is legitimately two findings, because
# the same statement is also an unwrapped in-place rewrite. Counting per file
# would have made this case fail on a scanner that was right.
if grep -q "test-aborting.sh:7: \[aborting-premise\]" <<< "$OUT" &&
  grep -q "premise at line 11 aborts" <<< "$OUT" &&
  [[ "$(grep 'test-aborting.sh' <<< "$OUT" | grep -c 'aborting-premise')" -eq 1 ]]; then
  pass "the one uncaught premise of four occurrences is the finding, anchored where the wrapper goes and naming the abort's line"
else
  fail "aborting-premise did not separate the premise from the three occurrences that are prose"
  echo "$OUT" >&2
fi

# 11. The same block wrapped is not a finding — the abort is then the diagnosis
#     rather than the defect, which is the whole design.
probe test-aborting-wrapped.sh <<'SH'
#!/usr/bin/env bash
DIR="$(new_case delta)"
if fixture_mutate "case 3" "$DIR/notes.txt" ruby -e '
  path = ARGV[0]
  text = File.read(path)
  abort "the fixture anchor is missing" unless text.include?("anchor")
  File.write(path, text.sub("anchor", "moved"))
' "$DIR/notes.txt"; then
  pass "case 3"
fi
SH
OUT="$(scan)"
if ! grep -q "test-aborting-wrapped.sh" <<< "$OUT"; then
  pass "a wrapped premise keeps its abort, because something now catches it"
else
  fail "a correctly wrapped premise was flagged"
  echo "$OUT" >&2
fi

# 12. exit-code-only, all three directions. §4.4 stated mechanically: a proxy may
#     be an additional condition, never the only one.
probe test-exitcode.sh <<'SH'
#!/usr/bin/env bash
run_gate "$DIR" a
if [[ "$STATUS" -ne 0 ]]; then
  pass "the gate rejected it"
else
  fail "no"
fi
run_gate "$DIR" b
if [[ "$STATUS" -ne 0 ]] && grep -q "+ crates/cli names the driver" "$WORK/b.err"; then
  pass "the gate rejected it and said which branch"
fi
run_gate "$DIR" c-before
BEFORE_STATUS=$STATUS
run_gate "$DIR" c
if [[ "$STATUS" -ne 0 ]]; then
  pass "it failed only after the removal, since BEFORE_STATUS was 0"
fi
SH
OUT="$(scan)"
if grep -q "test-exitcode.sh:3: \[exit-code-only\]" <<< "$OUT" &&
  ! grep -q "test-exitcode.sh:10:" <<< "$OUT" &&
  ! grep -q "test-exitcode.sh:16:" <<< "$OUT"; then
  pass "an exit code alone is a finding; a diagnostic match is not, and neither is a recorded before-run"
else
  fail "exit-code-only did not discriminate the two escapes"
  echo "$OUT" >&2
fi

# 13. restated-expectation. Incident 1 wrote `pubMod 52 -> 53` and incident 8
#     wrote `sql 8 -> 7`; both are ledger values a gate exists to let move.
probe test-restated.sh <<'SH'
#!/usr/bin/env bash
if [[ "$STATUS" -ne 0 ]] && grep -q "~ $TARGET sql 8 -> 7" "$WORK/a.err"; then
  pass "restated"
fi
if [[ "$STATUS" -ne 0 ]] && grep -q "~ $TARGET sql $N -> $((N - 1))" "$WORK/b.err"; then
  pass "derived"
fi
SH
OUT="$(scan)"
if grep -q "test-restated.sh:2: \[restated-expectation\]" <<< "$OUT" &&
  ! grep -q "test-restated.sh:5:" <<< "$OUT"; then
  pass "a literal N -> M in an expected diagnostic is a finding; the same expectation read from the ledger is not"
else
  fail "restated-expectation did not discriminate a literal from a derivation"
  echo "$OUT" >&2
fi

# 14. unclosed-heredoc. The backstop that does not depend on the other four
#     being right: a clean result over a file the scanner only half-read is an
#     artefact of how much was read. FR-138 is exactly that failure in the bash
#     3.2 scanner, so it is asserted here rather than inherited on trust.
probe test-unclosed.sh <<'SH'
#!/usr/bin/env bash
cat > "$DIR/thing.txt" <<'NEVERCLOSED'
DIR="$(new_case eps)"
ruby -e 'File.write(ARGV[0], "x")' "$DIR/config/ledger.json"
SH
OUT="$(scan)"
# Both directions. The counterpart is the control probe from case 7, which
# contains a here-document that closes: without asserting that one is silent,
# "reports every here-document" and "reports unterminated ones" have the same
# green record here.
if grep -q "test-unclosed.sh:2: \[unclosed-heredoc\]" <<< "$OUT" &&
  ! grep -q "test-clean.sh.*unclosed-heredoc" <<< "$OUT"; then
  pass "a file ending inside a here-document is reported; one that closes is not"
else
  fail "unclosed-heredoc did not separate a terminated here-document from an unterminated one"
  echo "$OUT" >&2
fi

# 15. The scratch trees are discovered, not listed.
#
#     This case is the finding my own measurement prototype missed. A hand-listed
#     roster of variable names — DIR, d, BASE, PROBE — found 27 sites; deriving
#     the roots from the assignments found 28, the extra one being a scratch
#     variable named nothing like the others. §4.4 shape 2 inside the tool built
#     to catch §4.4 shape 2.
probe test-discovery.sh <<'SH'
#!/usr/bin/env bash
QA_ROOT="$WORK/an-unusual-name"
TOOL_FIXTURE="$QA_ROOT/coordination-tools-only.yaml"
ruby -ryaml -e 'File.write(ARGV[0], "kind: Workflow\n")' "$TOOL_FIXTURE"
SH
OUT="$(scan)"
if grep -q "test-discovery.sh:4: \[unproven-mutation\]" <<< "$OUT"; then
  pass "a scratch tree named nothing like the others is still followed, because the roots are derived"
else
  fail "the scratch-root discovery is a hand list after all"
  echo "$OUT" >&2
fi

# 16. Coverage follows the manifest.
BEFORE_FILES="$( (cd "$CASE" && ruby "$SCANNER" --list-files) | wc -l | tr -d ' ')"
probe test-freshly-registered.sh <<'SH'
#!/usr/bin/env bash
exit 0
SH
AFTER_LIST="$(cd "$CASE" && ruby "$SCANNER" --list-files)"
AFTER_FILES="$(wc -l <<< "$AFTER_LIST" | tr -d ' ')"
if [[ "$AFTER_FILES" -eq $((BEFORE_FILES + 1)) ]] &&
  grep -q "test-freshly-registered.sh" <<< "$AFTER_LIST"; then
  pass "registering a ci-required shell gate grows the scanned set by exactly one, with no edit to the scanner"
else
  fail "the scanned set did not follow the manifest ($BEFORE_FILES -> $AFTER_FILES)"
fi

# 17. And the gate holds on the repository it governs.
set +e
(cd "$REPO_ROOT" && ruby "$SCANNER" > "$WORK/repo.out" 2> "$WORK/repo.err")
REPO_STATUS=$?
set -e
if [[ "$REPO_STATUS" -eq 0 ]] && grep -q "Fixture target drift: PASS" "$WORK/repo.out"; then
  pass "the scanner passes on this repository"
else
  fail "the scanner does not pass on this repository (exit $REPO_STATUS)"
  cat "$WORK/repo.err" >&2
fi

echo ""
echo "=== FR-143 fixture target drift: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]]
