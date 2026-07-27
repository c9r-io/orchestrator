#!/usr/bin/env bash
#
# FR-138 bash 3.2 scanner lexical state — QA gate.
#
# `bash32-compat.rb` used to decide quoting per line. A `<< WORD` lookalike
# inside a region opened on an earlier line read as a here-document opener, and
# every remaining line of the file left the scan with no diagnostic. Two live
# instances at the time FR-138 was filed:
#
#   * `test-qa-gate-surface.sh` — a `perl -e` replacement string containing
#     `<<EOF`. 360 lines dropped, and they were hiding a real finding.
#   * `test-bash32-compat.sh` — `hosting << job_name` inside the `ruby -e` block
#     whose whole purpose is to prove CI runs that gate on a bash 3.2 host. The
#     line proving the gate has a macOS host is the line that hid the gate's own
#     last 16 lines from it.
#
# The second one is why this file exists separately from `test-bash32-compat.sh`.
# FR-138 diagnosed both escapes as "cross-line quoting is not tracked", and that
# diagnosis is incomplete: carrying `in_single`/`in_double` across lines fixes
# the first and leaves the second exactly as broken, because the `'` that opens
# the ruby program sits inside `"$( ... )"` and quoting *resets* inside a command
# substitution. Case 2 below is that shape specifically.
#
# Safety: read-only against the working tree. Fixtures are scratch git
# repositories under $TMPDIR; no daemon, no database, no provider.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="$REPO_ROOT/scripts/qa/bash32-compat.rb"

for command_name in ruby git awk; do
  command -v "$command_name" >/dev/null 2>&1 || { echo "missing required command: $command_name" >&2; exit 1; }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr138-lexer.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

new_repo() {
  local dir="$WORK/$1"
  mkdir -p "$dir"
  git -C "$dir" init --quiet
  echo "$dir"
}

run_gate() {
  ruby "$GATE" --repo-root "$1" 2>&1
}

# Every fixture asserts three things, not one:
#   * the gate rejects the tree,
#   * it rejects it under the *named* rule at the *named* line, and
#   * no other rule fires.
# The third is the FR-127 isolation convention. Without it a fixture that trips
# some unrelated rule reports success for the wrong reason, and the rule it
# claims to cover stays unverified.
expect_only() {
  local label="$1" dir="$2" rule="$3" locator="$4"
  local output other

  if output="$(run_gate "$dir")"; then
    echo "$output" >&2
    fail "$label: the gate accepted the tree"
    return
  fi

  if ! grep -q "\[$rule\]" <<<"$output"; then
    echo "$output" >&2
    fail "$label: rejected, but not under [$rule]"
    return
  fi

  if ! grep -q "$locator" <<<"$output"; then
    echo "$output" >&2
    fail "$label: reported under [$rule] but not at $locator"
    return
  fi

  other="$(grep -oE '\[[a-z-]+\]' <<<"$output" | sort -u | grep -v "^\[$rule\]$" || true)"
  if [[ -n "$other" ]]; then
    echo "$output" >&2
    fail "$label: other rules also fired ($(tr '\n' ' ' <<<"$other")); the fixture is not isolated"
    return
  fi

  pass "$label"
}

echo "== case 1: a here-document lookalike inside a cross-line single-quoted region =="
# The hazard sits on the LAST line of the file on purpose. Put it in the middle
# and a partial fix that recovers only some of the swallowed tail still passes —
# the assertion has to be positioned where only full recovery reaches it.
REPO1="$(new_repo case1)"
cat > "$REPO1/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
perl -pi -e '
  s{alpha}{beta\n  cat > /dev/null <<EOF\n  payload\n  EOF};
' /dev/null
echo still-code
mapfile -t tail_of_file < /dev/null
OUTER
git -C "$REPO1" add -A
expect_only "a hazard after a quoted <<EOF lookalike is still scanned" "$REPO1" mapfile "subject.sh:7"

echo "== case 2: the same lookalike inside \$( ... ' ... ' ) =="
# The shape that survives the obvious fix. `X="$(ruby -e '` opens a double-quoted
# region, so a flat two-boolean tracker reads the `'` as a literal character and
# never enters the single-quoted state at all. Quoting resets inside `$( )`; a
# lexer that does not model that reports this tree clean.
REPO2="$(new_repo case2)"
cat > "$REPO2/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
JOBS="$(ruby -e '
  hosting = []
  hosting << job_name
  puts hosting.join(" ")
' )"
echo "$JOBS"
declare -A tail_of_file=()
OUTER
git -C "$REPO2" add -A
expect_only "quoting resets inside a command substitution" "$REPO2" associative-array "subject.sh:9"

echo "== case 3: an apostrophe inside a double-quoted string is not a quote opener =="
# Unlike the cases around it, this one's mutation target is not the original
# defect — the old per-line scanner could not have this bug, because it had no
# state to corrupt. It targets the mistake the *replacement* is most likely to
# make, and the one the first draft of this lexer did make: treating `'` as an
# opener even inside `"`. That leaves the file in a single-quoted region that
# never closes, blanks every line after it, and reports a clean tree.
#
# The apostrophe has to be the only one in the file. Give it a partner anywhere
# later and the bogus region closes, the hazard comes back into view, and the
# fixture passes against the very bug it is here to catch.
REPO3="$(new_repo case3)"
cat > "$REPO3/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
echo "one file's worth of output"
wait -n
OUTER
git -C "$REPO3" add -A
expect_only "an apostrophe inside double quotes does not swallow the file" "$REPO3" wait-n "subject.sh:4"

echo "== case 4: a file that ends inside a here-document is a finding =="
# The backstop that does not depend on the lexer being right about anything. Any
# escape, by any mechanism, leaves the file ending mid-here-document; that alone
# is reportable. Asserted by rule name: a combined fixture would exit non-zero
# whichever rule fired.
REPO4="$(new_repo case4)"
cat > "$REPO4/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
cat > /dev/null <<NEVER_CLOSED
this body runs to end of file
OUTER
git -C "$REPO4" add -A
expect_only "an unclosed here-document is reported at its opener" "$REPO4" unclosed-heredoc "subject.sh:3"

CASE4_OUTPUT="$(run_gate "$REPO4" || true)"
if grep -q "NEVER_CLOSED" <<<"$CASE4_OUTPUT"; then
  pass "the diagnostic names the terminator it is waiting for"
else
  echo "$CASE4_OUTPUT" >&2
  fail "the diagnostic does not name the unclosed terminator; a reader cannot locate the cause"
fi

echo "== case 5: negation is a command position =="
# `COMMAND_POSITION` listed `not`, which is not a bash keyword, and omitted `!`,
# which is. The candidate that could never match read as though negation were
# covered. Both halves are asserted: the invocation is caught, and the mentions
# that `3b5f9eb4` introduced the rule to suppress are still not.
REPO5="$(new_repo case5)"
cat > "$REPO5/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
if ! mapfile -t xs < /dev/null; then :; fi
OUTER
git -C "$REPO5" add -A
# The label spells the construct as `if ! map<>file` rather than literally. This
# file is scanned by the gate it tests, double-quoted text is live code to the
# scanner, and a builtin name in command position inside an ordinary string is a
# finding against this file — which is why the fixtures themselves are written as
# here-document bodies. Same convention as `test-bash32-compat.sh`.
expect_only "\`if ! map<>file\` is an invocation, not a mention" "$REPO5" mapfile "subject.sh:3"

cat > "$REPO5/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
CLASSES="empty-array-expansion mapfile case-conversion"
echo "$WORK/hazard/mapfile.sh"    # mapfile in a path and in a comment
entries=()
for index in ${entries[@]+"${!entries[@]}"}; do echo "$index"; done
[[ ! -f mapfile ]] || true
echo "$CLASSES"
OUTER
git -C "$REPO5" add -A
if run_gate "$REPO5" >/dev/null 2>&1; then
  pass "mentions of a builtin in paths, word lists, comments and \`[[ ! -f ]]\` are still not findings"
else
  run_gate "$REPO5" >&2 || true
  fail "adding \`!\` to the command-position set reintroduced the mention false positives"
fi

echo "== case 6: an array emptied in one file and expanded in another =="
# Emptiness used to be inferred per file, so an array zeroed in a sourced library
# and expanded in its caller was invisible. FR-138 removed the inference rather
# than extending it across the source graph: every unguarded value expansion is a
# finding now, which closes this direction and the over-reporting one together.
# The fixture stays because it is what pins the cross-file direction shut if
# anyone reintroduces inference.
REPO6="$(new_repo case6)"
mkdir -p "$REPO6/lib"
cat > "$REPO6/lib/shared.sh" <<'OUTER'
#!/usr/bin/env bash
shared_args=()
OUTER
cat > "$REPO6/consumer.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
. "$(dirname "$0")/lib/shared.sh"
printf '%s\n' "${shared_args[@]}"
OUTER
git -C "$REPO6" add -A
expect_only "an array emptied in a sourced library is caught in its caller" \
  "$REPO6" empty-array-expansion "consumer.sh:4"

echo "== case 7: the guarded form is still accepted =="
# A gate with no accepting state passes every fixture above and is useless.
REPO7="$(new_repo case7)"
cat > "$REPO7/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
args=()
printf '%s\n' ${args[@]+"${args[@]}"}
echo "${#args[@]}"
cat > /dev/null <<'INNER'
mapfile -t inside_a_heredoc < /dev/null
INNER
OUTER
git -C "$REPO7" add -A
if run_gate "$REPO7" >/dev/null 2>&1; then
  pass "the guarded form, length expansion and here-document bodies are accepted"
else
  run_gate "$REPO7" >&2 || true
  fail "the gate has no accepting state"
fi

echo "== case 8: every line of every tracked file is accounted for =="
# The census, and the reason this case is not "the gate passes". The FR-138
# defect happened *while the gate was green* — a truncated scan reports a clean
# tree, so the exit code is satisfied by exactly the state being tested for.
# Line accounting is not: a file whose scan stops early has lines that are
# neither scanned nor inside a here-document body, and the sum breaks.
CENSUS="$WORK/census.txt"
ruby "$GATE" --coverage-census > "$CENSUS"

CENSUS_FILES="$(awk 'END { print NR }' "$CENSUS")"
TRACKED_FILES="$(ruby "$GATE" --list-files | awk 'END { print NR }')"
if [[ "$CENSUS_FILES" -eq "$TRACKED_FILES" && "$TRACKED_FILES" -gt 0 ]]; then
  pass "the census covers all $TRACKED_FILES tracked shell files"
else
  fail "the census covers $CENSUS_FILES file(s) but git tracks $TRACKED_FILES"
fi

UNACCOUNTED="$(awk '$3 + $4 != $2 { printf "%s (total %s, scanned %s, heredoc %s)\n", $1, $2, $3, $4 }' "$CENSUS")"
if [[ -z "$UNACCOUNTED" ]]; then
  pass "in every file, scanned lines + here-document lines = total lines"
else
  echo "$UNACCOUNTED" >&2
  fail "some files have lines that are neither scanned nor here-document bodies"
fi

# The two files FR-138 named, asserted by name and by the number that actually
# moved. Both end in code, so a scan that reaches end of file has `last == total`.
# Under the defect these read 993/1353 and 369/385.
#
# Not asserted as "heredoc == 0": both files are QA wrappers that write their
# fixtures from here-documents, so dropping lines is correct behaviour here. The
# question is whether the scan came back out again.
for named in scripts/qa/test-qa-gate-surface.sh scripts/qa/test-bash32-compat.sh; do
  record="$(awk -v f="$named" '$1 == f { print; exit }' "$CENSUS")"
  if [[ -z "$record" ]]; then
    fail "census has no record for $named"
    continue
  fi
  # shellcheck disable=SC2086
  set -- $record
  if [[ "$5" -eq "$2" ]]; then
    pass "${named##*/}: the scan reaches line $5 of $2"
  else
    fail "${named##*/}: the scan stops at line $5 of $2 — $(($2 - $5)) lines never entered it"
  fi
done

echo
echo "FR-138 bash 3.2 scanner lexical state: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
