#!/usr/bin/env bash
#
# FR-149 — the qa-doc-lint workflow-ID check and its lifecycle scope predicate.
#
# FR-149 gave that check an exemption: documents whose frontmatter says
# `lifecycle: superseded` are not cross-referenced against the fixture corpus,
# because a superseded document describes a mechanism that was removed and the
# fixtures it names were deleted with it.
#
# An exemption that has never been tripped is an exemption whose reach is being
# guessed at (§4.4 shape 8), and a scope predicate is an assertion that deserves
# the same attack as an assertion (§4.4 shape 9). So every property the
# exemption claims has a case here that fails without it:
#
#   1  the check is really wired into scripts/qa-doc-lint.sh — observed by
#      running the lint and reading what it printed, not by grepping for a call
#   2  a clean tree passes, and says which documents it exempted
#   3  an ACTIVE document naming an unknown workflow still fails
#   4  the same document, superseded, passes — and is named in the exempt list
#   5  superseding one document does not silence a different active one
#   6  when the scope cannot be derived at all, the exemption evaporates and the
#      previously-exempt document is checked again — fail closed, not fail open
#
# Case 6 is the one that matters most and the one an author is least likely to
# write: it mutates a *different* file from the one it asserts about, so a
# cached or defaulted exemption would still pass cases 1-5 and fail only here.
#
# Every fixture derives its target from the repository rather than naming one.
# Nine recorded times a fixture's named target moved and eight stayed green
# (§4.4 shape 7); a fixture that says "the first active QA document carrying a
# --workflow line" cannot go stale that way.
#
# Safety: every mutation happens inside a temporary copy under $TMPDIR. The
# working tree is never written, no daemon is started, no database is touched,
# no provider is invoked, nothing reaches the network.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

command -v ruby >/dev/null 2>&1 || { echo "missing required command: ruby" >&2; exit 1; }
command -v rg >/dev/null 2>&1 || { echo "missing required command: rg" >&2; exit 1; }

LIB="scripts/lib/qa_doc_workflow_ids.sh"
GATE="scripts/qa/doc-lifecycle.rb"
# doc-lifecycle.rb requires the shared serialiser. A case repo is assembled by
# copying, so a working-tree-only dependency has to be named here or the gate
# under test dies on a missing require before asserting anything.
GATE_LIBS="scripts/lib/rust_source.rb scripts/lib/rust_lexer.rb scripts/lib/ci_env.rb"

# An id no bundle can plausibly define. Long and self-describing so that if it
# ever appears in a real log, its origin is obvious.
BOGUS_ID="fr149-scope-fixture-no-such-workflow"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr149-doc-lint-scope.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

# A case copies only what the check reads: the three document roots
# doc-lifecycle.rb scans, the fixture corpus, the check itself and the gate it
# derives its scope from.
new_case() {
  local dir="$WORK/$1"
  mkdir -p "$dir/scripts/qa" "$dir/scripts/lib" "$dir/docs" "$dir/fixtures/manifests"
  cp -R "$REPO_ROOT/docs/design_doc" "$dir/docs/design_doc"
  cp -R "$REPO_ROOT/docs/qa" "$dir/docs/qa"
  cp -R "$REPO_ROOT/docs/security" "$dir/docs/security"
  cp -R "$REPO_ROOT/fixtures/manifests/bundles" "$dir/fixtures/manifests/bundles"
  cp "$REPO_ROOT/$LIB" "$dir/$LIB"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  local lib
  for lib in $GATE_LIBS; do
    cp "$REPO_ROOT/$lib" "$dir/$lib"
  done
  echo "$dir"
}

# Runs the check inside a case directory, capturing stdout+stderr and the true
# exit status. Not piped anywhere: a pipe would hand back the reader's status.
run_check() {
  local dir="$1" out="$2"
  ( cd "$dir" && . "./$LIB" && qa_doc_workflow_ids_check "[scope]" ) >"$out" 2>&1
}

# These two derive a target; they never write. Both are shaped as assignments
# because that is what separates reading a value out of a fixture tree from
# rewriting one in place — `scripts/qa/fixture-target-drift.rb` draws exactly
# that line, and it flagged the earlier subshell form of both.
#
# The ruby always exits 0 and prints nothing when it finds nothing, so a
# non-zero status can only mean the derivation itself broke. Collapsing the two
# into "empty string" is the shape that reports "no such document" when what
# actually happened was a crash — the wrong diagnosis, on the fixture's own
# premise.

# The first active QA document under docs/qa/orchestrator that already carries a
# `--workflow <id>` line, derived from the case repo. Two calls with different
# skip counts give two distinct documents.
pick_doc() {
  local dir="$1" skip="${2:-0}" out=""
  out=$(cd "$dir" && ruby -e '
      skip = ARGV[0].to_i
      seen = 0
      Dir.glob("docs/qa/orchestrator/*.md").sort.each do |path|
        next if File.basename(path) == "README.md"
        body = File.read(path)
        next unless body =~ /^---\n(.*?)\n---\n/m
        next unless Regexp.last_match(1).include?("lifecycle: active")
        next unless body.include?("--workflow ")
        if seen == skip
          puts path
          break
        end
        seen += 1
      end
    ' "$skip") || {
    fail "pick_doc: the derivation itself failed in $dir; the fixture has no target"
    return 2
  }
  [[ -n "$out" ]] || {
    fail "pick_doc: no active QA document under docs/qa/orchestrator carries a --workflow line (skip=$skip)"
    return 1
  }
  printf '%s\n' "$out"
}

# An existing active document to point `superseded_by` at. Derived, because a
# named successor is one more thing that can move.
pick_successor() {
  local dir="$1" avoid="$2" out=""
  out=$(cd "$dir" && ruby -e '
      avoid = ARGV[0]
      Dir.glob("docs/qa/orchestrator/*.md").sort.each do |path|
        next if path == avoid || File.basename(path) == "README.md"
        body = File.read(path)
        next unless body =~ /^---\n(.*?)\n---\n/m
        next unless Regexp.last_match(1).include?("lifecycle: active")
        puts path
        break
      end
    ' "$avoid") || {
    fail "pick_successor: the derivation itself failed in $dir; the fixture has no successor"
    return 2
  }
  [[ -n "$out" ]] || {
    fail "pick_successor: no active QA document is available as a successor (avoiding $avoid)"
    return 1
  }
  printf '%s\n' "$out"
}

add_bogus_workflow() {
  local file="$1" id="$2"
  fixture_mutate "append an unknown --workflow id to ${file##*/}" "$file" ruby -e '
    file, id = ARGV
    File.open(file, "a") do |f|
      f.puts
      f.puts "```bash"
      f.puts "orchestrator task create --project fr149-scope --workflow #{id}"
      f.puts "```"
    end
  ' "$file" "$id"
}

mark_superseded() {
  local file="$1" successor="$2"
  fixture_mutate "mark ${file##*/} superseded" "$file" ruby -e '
    file, successor = ARGV
    lines = File.readlines(file)
    abort "no frontmatter in #{file}" unless lines[0].chomp == "---"
    closing = (1...lines.length).find { |i| lines[i].chomp == "---" }
    abort "unterminated frontmatter in #{file}" if closing.nil?
    replaced = false
    (1...closing).each do |i|
      next unless lines[i].start_with?("lifecycle:")
      lines[i] = "lifecycle: superseded\n"
      replaced = true
    end
    abort "no lifecycle key in #{file}" unless replaced
    lines.insert(closing, "superseded_by: #{successor}\n")
    File.write(file, lines.join)
  ' "$file" "$successor"
}

# Removes a document's frontmatter entirely. doc-lifecycle.rb refuses to emit an
# index built from documents that fail validation, so this is how the scope
# derivation is made to fail without touching the check itself.
break_frontmatter() {
  local file="$1"
  fixture_mutate "remove the frontmatter from ${file##*/}" "$file" ruby -e '
    file = ARGV[0]
    lines = File.readlines(file)
    abort "no frontmatter in #{file}" unless lines[0].chomp == "---"
    closing = (1...lines.length).find { |i| lines[i].chomp == "---" }
    abort "unterminated frontmatter in #{file}" if closing.nil?
    File.write(file, lines[(closing + 1)..-1].join)
  ' "$file"
}

echo "FR-149 qa-doc-lint workflow-ID scope"
echo ""

# --- Case 1: the check is wired into the lint -------------------------------
# Observed by running the real lint and reading what it printed at run time. A
# grep for the function name in the file would be satisfied by a commented-out
# call (§4.4 shape 1). The lint's own verdict is deliberately not asserted here:
# this case is about whether the section executes, and coupling it to the whole
# lint's colour would make an unrelated failure look like a wiring failure.
echo "Case 1: scripts/qa-doc-lint.sh actually runs the check"
LINT_OUT="$WORK/lint.out"
( cd "$REPO_ROOT" && bash scripts/qa-doc-lint.sh ) >"$LINT_OUT" 2>&1 || true
if rg -q 'exempt \(lifecycle: superseded\)' "$LINT_OUT"; then
  pass "the lint printed the check's exempt line, so the check ran"
else
  fail "scripts/qa-doc-lint.sh produced no output from the workflow-ID check"
fi
echo ""

# --- Case 2: a clean tree passes and reports its exempt set -----------------
echo "Case 2: a clean tree passes, and names what it exempted"
CASE2="$(new_case case2)"
if run_check "$CASE2" "$WORK/case2.out"; then
  if rg -q 'exempt \(lifecycle: superseded\)' "$WORK/case2.out"; then
    pass "the check passes on an unmutated tree and prints its exempt set"
  else
    fail "the check passed but did not report which documents it exempted"
  fi
else
  fail "the check does not pass on an unmutated tree"
  cat "$WORK/case2.out" >&2
fi
echo ""

# --- Case 3: an active document naming an unknown workflow fails ------------
echo "Case 3: an ACTIVE document naming an unknown workflow fails"
CASE3="$(new_case case3)"
DOC3="$(pick_doc "$CASE3" 0)" || DOC3=""
if [[ -z "$DOC3" ]]; then
  fail "no active QA document carrying a --workflow line; the fixture has no target"
elif add_bogus_workflow "$CASE3/$DOC3" "$BOGUS_ID"; then
  if run_check "$CASE3" "$WORK/case3.out"; then
    fail "an active document naming '$BOGUS_ID' passed the check"
  elif rg -q "Unknown workflow ID '$BOGUS_ID' at $DOC3" "$WORK/case3.out"; then
    pass "the check fails and names the document and the id: $DOC3"
  else
    fail "the check failed, but not on the unknown id it was given"
    cat "$WORK/case3.out" >&2
  fi
fi
echo ""

# --- Case 4: the same document, superseded, is exempt and says so -----------
# The mutation is applied on top of case 3's, so the *only* difference between a
# red run and a green one is the lifecycle field. Asserting the exempt line as
# well as the exit code, because an exit code cannot say which branch produced
# it (§4.4 shape 7).
echo "Case 4: the same document, superseded, is exempt — and is named as exempt"
CASE4="$(new_case case4)"
DOC4="$(pick_doc "$CASE4" 0)" || DOC4=""
SUCC4="$(pick_successor "$CASE4" "$DOC4")" || SUCC4=""
if [[ -z "$DOC4" || -z "$SUCC4" ]]; then
  fail "could not derive a document and a successor for the supersession fixture"
elif add_bogus_workflow "$CASE4/$DOC4" "$BOGUS_ID" && mark_superseded "$CASE4/$DOC4" "$SUCC4"; then
  if ! run_check "$CASE4" "$WORK/case4.out"; then
    fail "a superseded document was still cross-referenced against the corpus"
    cat "$WORK/case4.out" >&2
  elif rg -q "exempt \(lifecycle: superseded\):" "$WORK/case4.out" && rg -q "  $DOC4\$" "$WORK/case4.out"; then
    pass "the superseded document is exempt and is printed in the exempt set"
  else
    fail "the check passed but never reported $DOC4 as exempt — a silent exemption"
    cat "$WORK/case4.out" >&2
  fi
fi
echo ""

# --- Case 5: superseding one document does not silence another --------------
echo "Case 5: superseding one document does not silence a different active one"
CASE5="$(new_case case5)"
DOC5A="$(pick_doc "$CASE5" 0)" || DOC5A=""
DOC5B="$(pick_doc "$CASE5" 1)" || DOC5B=""
SUCC5="$(pick_successor "$CASE5" "$DOC5A")" || SUCC5=""
if [[ -z "$DOC5A" || -z "$DOC5B" || -z "$SUCC5" ]]; then
  fail "could not derive two distinct documents for the blast-radius fixture"
elif add_bogus_workflow "$CASE5/$DOC5A" "$BOGUS_ID" \
  && mark_superseded "$CASE5/$DOC5A" "$SUCC5" \
  && add_bogus_workflow "$CASE5/$DOC5B" "$BOGUS_ID"; then
  if run_check "$CASE5" "$WORK/case5.out"; then
    fail "an active document's unknown id was silenced by superseding a different document"
  elif rg -q "Unknown workflow ID '$BOGUS_ID' at $DOC5B" "$WORK/case5.out" \
    && ! rg -q "Unknown workflow ID '$BOGUS_ID' at $DOC5A" "$WORK/case5.out"; then
    pass "the active document is reported and the superseded one is not"
  else
    fail "the check failed, but not with exactly one of the two documents named"
    cat "$WORK/case5.out" >&2
  fi
fi
echo ""

# --- Case 6: an underivable scope fails closed ------------------------------
# The frontmatter is broken on a *third* document, so the exemption granted in
# case 4 has to evaporate for a reason that has nothing to do with the exempt
# document itself. A defaulted, cached or hard-coded exemption passes cases 1-5
# and fails only here.
echo "Case 6: when the scope cannot be derived, the exemption evaporates"
CASE6="$(new_case case6)"
DOC6="$(pick_doc "$CASE6" 0)" || DOC6=""
SUCC6="$(pick_successor "$CASE6" "$DOC6")" || SUCC6=""
VICTIM6="$(pick_doc "$CASE6" 1)" || VICTIM6=""
if [[ -z "$DOC6" || -z "$SUCC6" || -z "$VICTIM6" ]]; then
  fail "could not derive the three documents the fail-closed fixture needs"
elif add_bogus_workflow "$CASE6/$DOC6" "$BOGUS_ID" \
  && mark_superseded "$CASE6/$DOC6" "$SUCC6" \
  && break_frontmatter "$CASE6/$VICTIM6"; then
  if run_check "$CASE6" "$WORK/case6.out"; then
    fail "the scope derivation failed and the check passed anyway — it fails open"
    cat "$WORK/case6.out" >&2
  elif rg -q 'doc-lifecycle.rb --emit-index failed' "$WORK/case6.out" \
    && rg -q "Unknown workflow ID '$BOGUS_ID' at $DOC6" "$WORK/case6.out"; then
    pass "the derivation failure is reported and the exempt document is checked again"
  else
    fail "the check failed, but not by reporting the broken scope and re-checking $DOC6"
    cat "$WORK/case6.out" >&2
  fi
fi
echo ""

echo "FR-149 qa-doc-lint workflow-ID scope: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
