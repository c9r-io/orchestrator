#!/usr/bin/env bash
#
# FR-132 document lifecycle governance — QA gate.
#
# Verifies that scripts/qa/doc-lifecycle.rb actually holds the lifecycle metadata
# on docs/design_doc and docs/qa, and actually keeps
# config/governance/doc-lifecycle-index.json in step with them. A gate observed
# only passing has not been observed doing anything, so every case below is
# paired with a defect it must reject.
#
# Safety: every mutation happens inside a temporary copy under $TMPDIR. The
# working tree is never written, no daemon is started, no database is touched,
# and no provider is invoked.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/doc-lifecycle.rb"
# The gate reuses the shared ledger serialiser. A case repo is assembled by
# copying, so a working-tree-only dependency has to be named here or the gate
# under test dies on a missing require before asserting anything — which is how
# the FR-130 wrapper first failed.
GATE_LIBS=(
  scripts/lib/rust_source.rb
  scripts/lib/rust_lexer.rb
  scripts/lib/ci_env.rb
)
INDEX="config/governance/doc-lifecycle-index.json"

command -v ruby >/dev/null 2>&1 || { echo "missing required command: ruby" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr132-doc-lifecycle.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

digest() { ruby -rdigest -e 'print Digest::SHA256.file(ARGV[0]).hexdigest' "$1"; }

# A case copies only what the gate scans: the two document roots, the index, the
# gate and its shared library.
new_case() {
  local dir
  dir="$WORK/$1"
  mkdir -p "$dir/config/governance" "$dir/scripts/qa" "$dir/scripts/lib" "$dir/docs"
  cp -R "$REPO_ROOT/docs/design_doc" "$dir/docs/design_doc"
  cp -R "$REPO_ROOT/docs/qa" "$dir/docs/qa"
  cp "$REPO_ROOT/$INDEX" "$dir/$INDEX"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  local lib
  for lib in ${GATE_LIBS[@]+"${GATE_LIBS[@]}"}; do
    cp "$REPO_ROOT/$lib" "$dir/$lib"
  done
  echo "$dir"
}

# Rewrites one frontmatter key in place, or appends it when absent. Used by the
# negative cases so each mutation is a single, named edit.
set_field() {
  local file="$1" key="$2" value="$3"
  ruby -e '
    file, key, value = ARGV
    lines = File.readlines(file)
    abort "no frontmatter in #{file}" unless lines[0].chomp == "---"
    closing = (1...lines.length).find { |i| lines[i].chomp == "---" }
    abort "unterminated frontmatter in #{file}" if closing.nil?
    replaced = false
    (1...closing).each do |i|
      next unless lines[i].start_with?("#{key}:")
      lines[i] = "#{key}: #{value}\n"
      replaced = true
    end
    lines.insert(closing, "#{key}: #{value}\n") unless replaced
    File.write(file, lines.join)
  ' "$file" "$key" "$value"
}

drop_field() {
  local file="$1" key="$2"
  ruby -e '
    file, key = ARGV
    lines = File.readlines(file)
    closing = (1...lines.length).find { |i| lines[i].chomp == "---" }
    kept = lines[0..closing].reject { |l| l.start_with?("#{key}:") }
    File.write(file, (kept + lines[(closing + 1)..-1]).join)
  ' "$file" "$key"
}

echo "FR-132 doc lifecycle"
echo ""

SAMPLE="docs/design_doc/orchestrator/142-core-boundary-freeze.md"
OTHER="docs/qa/orchestrator/180-core-boundary-freeze.md"
SUPERSEDED="docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md"

# --- Case 1: the gate passes on the repository -------------------------------
echo "Case 1: the gate holds on the working tree"
if (cd "$REPO_ROOT" && ruby "$GATE" > "$WORK/case1.out" 2> "$WORK/case1.err"); then
  if grep -q "Doc lifecycle: PASS" "$WORK/case1.out"; then
    pass "the doc lifecycle gate passes on the repository"
  else
    fail "the gate exited 0 without printing its PASS summary"
  fi
else
  fail "the doc lifecycle gate does not pass on the repository"
  cat "$WORK/case1.err" >&2
fi

# --- Case 2: the emitted index is byte-identical to the committed one ---------
echo "Case 2: --emit-index reproduces the committed index byte for byte"
(cd "$REPO_ROOT" && ruby "$GATE" --emit-index > "$WORK/case2.json" 2> "$WORK/case2.err") || true
if [[ -s "$WORK/case2.json" ]] && [[ "$(digest "$WORK/case2.json")" == "$(digest "$REPO_ROOT/$INDEX")" ]]; then
  pass "the regeneration path reproduces the committed index exactly"
else
  fail "--emit-index differs from the committed index; regeneration is not reviewable"
fi

# --- Case 3: a document with no frontmatter is rejected -----------------------
echo "Case 3: a document whose frontmatter was removed fails, naming the file"
DIR="$(new_case case3)"
ruby -e '
  file = ARGV[0]
  lines = File.readlines(file)
  closing = (1...lines.length).find { |i| lines[i].chomp == "---" }
  File.write(file, lines[(closing + 1)..-1].join.sub(/\A\n+/, ""))
' "$DIR/$SAMPLE"
if (cd "$DIR" && ruby "$GATE" > "$WORK/case3.out" 2> "$WORK/case3.err"); then
  fail "a document with no frontmatter passed the gate"
else
  if grep -q "$SAMPLE" "$WORK/case3.err" && grep -q "no frontmatter block" "$WORK/case3.err"; then
    pass "an unclassified document fails and the diagnostic names it"
  else
    fail "the gate failed but did not name the unclassified document"
  fi
fi

# --- Case 4: superseded without a successor is rejected -----------------------
echo "Case 4: lifecycle: superseded with no superseded_by fails"
DIR="$(new_case case4)"
drop_field "$DIR/$SUPERSEDED" superseded_by
if (cd "$DIR" && ruby "$GATE" > "$WORK/case4.out" 2> "$WORK/case4.err"); then
  fail "a superseded document with no successor passed the gate"
else
  if grep -q "no \`superseded_by\` names the successor" "$WORK/case4.err"; then
    pass "a supersession with no successor is rejected"
  else
    fail "the gate failed for some other reason than the missing successor"
  fi
fi

# --- Case 5: a dangling superseded_by is rejected -----------------------------
echo "Case 5: superseded_by naming a file that does not exist fails"
DIR="$(new_case case5)"
set_field "$DIR/$SUPERSEDED" superseded_by "docs/design_doc/orchestrator/999-does-not-exist.md"
if (cd "$DIR" && ruby "$GATE" > "$WORK/case5.out" 2> "$WORK/case5.err"); then
  fail "a dangling superseded_by passed the gate"
else
  if grep -q "does not resolve to a file" "$WORK/case5.err"; then
    pass "a superseded_by pointing at nothing is rejected"
  else
    fail "the gate failed for some other reason than the dangling pointer"
  fi
fi

# --- Case 6: a self-referential superseded_by is rejected ---------------------
# The target exists, so an existence check alone would pass this. The document
# still tells a reader it was replaced by itself.
echo "Case 6: superseded_by pointing at the document itself fails"
DIR="$(new_case case6)"
set_field "$DIR/$SUPERSEDED" superseded_by "$SUPERSEDED"
if (cd "$DIR" && ruby "$GATE" > "$WORK/case6.out" 2> "$WORK/case6.err"); then
  fail "a self-referential superseded_by passed the gate"
else
  if grep -q "points at the document itself" "$WORK/case6.err"; then
    pass "a document superseded by itself is rejected"
  else
    fail "the gate failed for some other reason than the self-reference"
  fi
fi

# --- Case 7: a supersession cycle is rejected ---------------------------------
# Every pointer in a cycle resolves to a real file, so existence and
# self-reference checks both pass. Only following the chain finds it.
echo "Case 7: a two-document superseded_by cycle fails"
DIR="$(new_case case7)"
set_field "$DIR/$SAMPLE" lifecycle superseded
set_field "$DIR/$SAMPLE" superseded_by "$OTHER"
set_field "$DIR/$OTHER" lifecycle superseded
set_field "$DIR/$OTHER" superseded_by "$SAMPLE"
if (cd "$DIR" && ruby "$GATE" > "$WORK/case7.out" 2> "$WORK/case7.err"); then
  fail "a supersession cycle passed the gate"
else
  if grep -q "forms a cycle" "$WORK/case7.err"; then
    pass "a supersession cycle is rejected"
  else
    fail "the gate failed for some other reason than the cycle"
  fi
fi

# --- Case 8: the lifecycle value is validated, not merely present -------------
echo "Case 8: an out-of-vocabulary lifecycle value fails"
DIR="$(new_case case8)"
set_field "$DIR/$SAMPLE" lifecycle banana
if (cd "$DIR" && ruby "$GATE" > "$WORK/case8.out" 2> "$WORK/case8.err"); then
  fail "lifecycle: banana passed the gate"
else
  if grep -q "is not one of active, superseded" "$WORK/case8.err"; then
    pass "the lifecycle vocabulary is enforced, not just the key's presence"
  else
    fail "the gate failed but not on the illegal lifecycle value"
  fi
fi

# --- Case 9: related_fr format is enforced; absence is legal ------------------
echo "Case 9: a malformed related_fr fails while an absent one passes"
DIR="$(new_case case9)"
set_field "$DIR/$SAMPLE" related_fr "the streaming pivot"
# The exit code alone does not attribute the failure. Editing related_fr also
# moves the index, so a gate with its format check removed still exits non-zero
# — on index drift. Requiring the diagnostic is what makes this case fail when
# the format check is gone, which it did not during the FR-132 mutation run.
if ! (cd "$DIR" && ruby "$GATE" > "$WORK/case9a.out" 2> "$WORK/case9a.err") \
  && ! grep -q "is not FR-<number>" "$WORK/case9a.err"; then
  fail "a free-text related_fr was not rejected on its format; the gate failed for another reason"
elif (cd "$DIR" && ruby "$GATE" > /dev/null 2>&1); then
  fail "a free-text related_fr passed the gate"
else
  DIR2="$(new_case case9b)"
  drop_field "$DIR2/$OTHER" related_fr
  # Dropping related_fr changes the index, so the index has to be regenerated for
  # this half; what is under test is that absence is *legal*, not that it is a
  # no-op for the index.
  (cd "$DIR2" && ruby "$GATE" --emit-index > "$DIR2/$INDEX.new" 2> "$WORK/case9b.emit.err") \
    && mv "$DIR2/$INDEX.new" "$DIR2/$INDEX"
  if (cd "$DIR2" && ruby "$GATE" > "$WORK/case9b.out" 2> "$WORK/case9b.err"); then
    pass "related_fr is format-checked when present and optional when absent"
  else
    fail "a document with no related_fr was rejected; the field is meant to be optional"
    cat "$WORK/case9b.err" >&2
  fi
fi

# --- Case 10: coverage is derived from the tree, not from a list --------------
# The mutation lands in a directory that did not exist when the gate was written.
# An enumerated roster of scanned paths would not see it; a filesystem walk does.
echo "Case 10: a document in a brand-new subdirectory is still governed"
DIR="$(new_case case10)"
mkdir -p "$DIR/docs/design_doc/newly-invented-module"
printf '# Untagged\n\nbody\n' > "$DIR/docs/design_doc/newly-invented-module/01-untagged.md"
if (cd "$DIR" && ruby "$GATE" > "$WORK/case10.out" 2> "$WORK/case10.err"); then
  fail "a document in a new subdirectory escaped the gate; coverage is enumerated, not derived"
else
  if grep -q "newly-invented-module/01-untagged.md" "$WORK/case10.err"; then
    pass "coverage follows the filesystem, so a new subdirectory is governed on arrival"
  else
    fail "the gate failed but not because of the document in the new subdirectory"
  fi
fi

# --- Case 11: --write refuses under CI ---------------------------------------
echo "Case 11: CI=1 --emit-index --write refuses and leaves the index untouched"
DIR="$(new_case case11)"
BEFORE="$(digest "$DIR/$INDEX")"
set_field "$DIR/$SAMPLE" related_fr "FR-999"
if (cd "$DIR" && CI=1 ruby "$GATE" --emit-index --write > "$WORK/case11.out" 2> "$WORK/case11.err"); then
  fail "--write succeeded under CI; the review gate would be decoration"
else
  if grep -q "refusing --write under CI" "$WORK/case11.err" \
    && [[ "$(digest "$DIR/$INDEX")" == "$BEFORE" ]]; then
    pass "an automatic ledger rewrite in CI is refused and the index is unchanged"
  else
    fail "--write under CI did not refuse cleanly, or it wrote the index anyway"
  fi
fi

# --- Case 12: the index tracks frontmatter, in both directions ----------------
# Requirement 5's consistency assertion. Changing a document's related_fr must
# both move the emitted index and turn the committed index red.
echo "Case 12: changing related_fr moves the emitted index and fails the committed one"
DIR="$(new_case case12)"
(cd "$DIR" && ruby "$GATE" --emit-index > "$WORK/case12.before.json" 2> "$WORK/case12.err") || true
set_field "$DIR/$SAMPLE" related_fr "FR-901"
(cd "$DIR" && ruby "$GATE" --emit-index > "$WORK/case12.after.json" 2>> "$WORK/case12.err") || true
if [[ -s "$WORK/case12.before.json" ]] \
  && [[ "$(digest "$WORK/case12.before.json")" != "$(digest "$WORK/case12.after.json")" ]] \
  && grep -q '"FR-901"' "$WORK/case12.after.json"; then
  if (cd "$DIR" && ruby "$GATE" > "$WORK/case12.out" 2> "$WORK/case12.verify.err"); then
    fail "the committed index still passed after a document's related_fr changed"
  else
    pass "the index follows the frontmatter and a stale index fails the gate"
  fi
else
  fail "changing related_fr did not change the emitted index"
fi

echo ""
echo "FR-132 doc lifecycle: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
