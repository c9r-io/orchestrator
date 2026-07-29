#!/usr/bin/env bash
#
# FR-145 — QA gate for scripts/qa/pipefail-short-circuit.rb, and for the property
# the scanner exists to protect.
#
# The rule has zero violations in this repository once the rewrite has landed.
# That is what a guard looks like, and it is also the problem: a rule nobody has
# tried to trip is a rule nobody knows can fire, and a rule that fires on correct
# input gets switched off long before it catches anything. So every "must fire"
# case below is paired with a "must not fire" one on the same probe.
#
# Two of the silent cases exist because FR-145 itself got them wrong: it counted
# 42 sites where there were 35, because `grep -c` counted four comment lines
# *describing* the pattern — one of them the comment written to explain the first
# fix. A scanner that repeats that error reports findings on its own
# documentation and is switched off in a week. Cases 7 and 8 are that error,
# turned into assertions.
#
# The FR's third claim — that two tracked files without `set -o pipefail` were
# immune — was wrong in the other direction, and case 9b is the demonstration
# that settles it: shell options are dynamic, `run-cli-probes.sh` sources every
# scenario into a shell that sets `pipefail`, and a scenario sourced that way
# reports a present pattern as absent. So case 9 asserts that the rule *does*
# fire there.
#
# Case 12 is a false positive this gate produced during development, kept because
# it is the one a reader would not think to write: `[[ "$(… | grep -c .)" -eq 3 ]]`
# was flagged as a quiet grep, because `-eq` is a short-flag cluster containing
# `q` and the flag scan had run past the end of the command.
#
# Case 16 asserts the mechanism itself, and does it deterministically. A test
# that runs the buffer race 400 times and asserts "at least one failure" is a
# coin flip on someone else's runner: measured here, a 90 KB producer fires 2-3%
# of the time while a 1 MB one fired 0/200. Remove the race instead of racing it
# — a producer that is still writing *by construction* when the reader leaves
# reproduces the property every time, on any machine, with any grep.
#
# The mutations go through scripts/lib/gate_fixture.sh, so a fixture whose target
# has moved fails loudly instead of proving nothing (FR-143).
#
# Safety: read-only against the working tree. Every case builds a scratch git
# checkout under $TMPDIR. No daemon starts, no database is touched, no provider
# is invoked, nothing reaches the network.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/pipefail-short-circuit.rb"

for command in ruby git mktemp shasum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr145-pipefail-short-circuit.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

CASE="$WORK/case"
PROBE="probe.sh"

# The probe in its correct form: everything a governed gate legitimately does,
# written the way the rule requires. Every "must fire" case mutates one line of
# this and nothing else, so a case that fires is firing on its own mutation.
#
# The mutation direction is deliberate. These cases rewrite a working here-string
# *back* into a pipe rather than deleting a line, because reintroduction is what
# actually happens: someone reaches for the spelling they already know. A fixture
# that deletes proves only that the gate notices absence.
write_probe() {
  cat > "$CASE/$PROBE" <<'PROBE'
#!/usr/bin/env bash
set -euo pipefail

# A comment describing the hazard: printf '%s' "$x" | grep -q described
rows="$(cat data.txt)"

grep -q "wanted" <<< "$rows" || echo missing
grep -q "a|b" data.txt || echo no-alternation
grep -q needle data.txt || echo not-a-pipeline
cat data.txt | grep -x whole-line-no-quiet
cat data.txt | grep -F -- -q literal-dash-q
[[ "$(cat data.txt | grep -c .)" -eq 3 ]] || echo wrong-count
rg --quiet other <<< "$rows" || echo missing-other
grep -qxF "$entry" <<< "$(printf '%s\n' $list)" || echo not-listed
sed -n '1,5p' <<< "$rows"
cat data.txt | sed -n '1,5p'
cat data.txt | awk 'NR<=5'
head -5 data.txt

cat <<'INNER'
printf '%s' "$x" | grep -q inside-a-heredoc-body
INNER
PROBE
}

reset_case() {
  rm -rf "$CASE"
  mkdir -p "$CASE"
  write_probe
  printf 'one\ntwo\nthree\n' > "$CASE/data.txt"
  git -C "$CASE" init -q .
  git -C "$CASE" add -A >/dev/null
}

# The exit status is read through `if`, never from `$?` after an assignment:
# the assignment would consume the status first (FR-144).
GATE_OUT=""
GATE_STATUS=0
run_gate() {
  local root="${1:-$CASE}"
  if GATE_OUT="$(ruby "$REPO_ROOT/$GATE" --repo-root "$root" 2>&1)"; then
    GATE_STATUS=0
  else
    GATE_STATUS=$?
  fi
}

# A rule must fire *and* the gate must exit non-zero, and it must name the line
# the case mutated. A gate that prints findings and returns 0 blocks nothing, and
# an exit code cannot distinguish the branch a gate failed through from any other
# (§4.4 shape 7) — so the line is asserted, not just the rule tag.
#
# The expected line is *derived* from the mutated probe by locating the marker,
# never written down: a fixture that restates a number stops working the moment
# the probe gains a line, and it stops working by passing on the wrong finding.
fires_on() {
  local label="$1" rule="$2" marker="$3" line
  # `grep -n … | head -1` is itself the shape this gate forbids once FR-146 lands, and a
  # harness exempt from its own rule is the FR-134 defect. `grep` reads to EOF here, and the
  # first line comes off by expansion instead of through a short-circuiting reader.
  local hits
  hits="$(grep -nF "$marker" "$CASE/$PROBE" || true)"
  line="${hits%%$'\n'*}"; line="${line%%:*}"
  if [[ -z "$line" ]]; then
    fail "$label: the marker '$marker' is not in the mutated probe, so nothing was asserted"
    return
  fi
  fires_at "$label" "$rule" "$line"
}

fires_at() {
  local label="$1" rule="$2" line="$3"
  run_gate
  if [[ "$GATE_STATUS" -eq 0 ]]; then
    fail "$label: the gate exited 0"
    return
  fi
  if grep -q "^$PROBE:$line: \[$rule\]" <<< "$GATE_OUT"; then
    pass "$label"
  else
    fail "$label: expected $PROBE:$line [$rule], got: $(grep -oE "^$PROBE:[0-9]+: \[[a-z-]+\]" <<< "$GATE_OUT" | tr '\n' ' ')"
  fi
}

# Silence is asserted on the summary line rather than on the exit code, because a
# scanner that died before reading anything can also leave a zero status behind.
silent() {
  local label="$1"
  run_gate
  if [[ "$GATE_STATUS" -eq 0 ]] && grep -q '^pipefail short-circuit: PASS' <<< "$GATE_OUT"; then
    pass "$label"
  else
    fail "$label: $(head -3 <<< "$GATE_OUT" | tr '\n' ' ')"
  fi
}

# from -> to on a whole line, routed through fixture_mutate so a pattern that no
# longer matches is a failed assertion rather than a silent no-op. Anchored to
# the whole line on purpose: FR-133 lost a case to an unanchored `sub` that
# rewrote a header comment instead of the setting, changed the digest, satisfied
# fixture_mutate, and left the subject untouched.
sub_line() {
  local label="$1" from="$2" to="$3"
  fixture_mutate "$label" "$CASE/$PROBE" \
    ruby -e 'path, from, to = ARGV
             text = File.read(path)
             raise "no line matched: #{from}" unless text =~ /^#{Regexp.escape(from)}$/
             File.write(path, text.sub(/^#{Regexp.escape(from)}$/, to))' \
    "$CASE/$PROBE" "$from" "$to" || return 1
  git -C "$CASE" add -A >/dev/null
}

echo "=== FR-145: pipefail short-circuit ==="
echo ""

# 1. Control. The probe as written exercises every silent shape at once: a
#    comment, a here-document body, a quoted alternation, a bare grep, a
#    non-quiet grep, a `--` terminated option list, a counting grep, two
#    here-strings and a word-split list. If any of these fired, every "must fire"
#    case below would be meaningless — they would be firing on the probe.
reset_case
silent "control: the correct probe produces no finding"

# 2. The canonical shape.
reset_case
if sub_line "reintroduce the pipe" \
  'grep -q "wanted" <<< "$rows" || echo missing' \
  'printf '"'"'%s'"'"' "$rows" | grep -q "wanted" || echo missing'; then
  fires_on "a here-string rewritten back into a pipe fires" short-circuit-under-pipefail \
    '| grep -q "wanted"'
fi

# 3. The long form. `--quiet` is the spelling a short-flag regex misses.
reset_case
if sub_line "long-form quiet" \
  'rg --quiet other <<< "$rows" || echo missing-other' \
  'cat data.txt | rg --quiet other || echo missing-other'; then
  fires_on "rg --quiet as a downstream stage fires" short-circuit-under-pipefail \
    '| rg --quiet other'
fi

# 4. A flag cluster. `-qxF` is one token and `q` is in the middle of it.
reset_case
if sub_line "clustered quiet flag" \
  'cat data.txt | grep -x whole-line-no-quiet' \
  'cat data.txt | grep -qxF whole-line'; then
  fires_on "a clustered -qxF fires" short-circuit-under-pipefail \
    '| grep -qxF whole-line'
fi

# 5. Inside a command substitution, inside double quotes. This is the case a
#    naive quote tracker loses: `"$( … )"` opens a fresh quoting context, so the
#    `|` is not quoted at all. The gate missed it on its first run.
reset_case
if sub_line "quiet grep inside a substitution" \
  'rows="$(cat data.txt)"' \
  'rows="$(cat data.txt | grep -q seed && echo ok)"'; then
  fires_on "a quiet grep inside \$( ) fires" short-circuit-under-pipefail \
    '| grep -q seed'
fi

# 6. A pipeline broken after the `|`. The reader is the first word on its line
#    and is still a downstream stage.
reset_case
if sub_line "pipeline continued on the next line" \
  'grep -q needle data.txt || echo not-a-pipeline' \
  'cat data.txt |
  grep -q needle || echo continued'; then
  fires_on "a reader on the line after a trailing pipe fires" short-circuit-under-pipefail \
    'grep -q needle || echo continued'
fi

# 7. A comment is not a hazard. This is the error FR-145 made about itself.
reset_case
if sub_line "hazard in a comment" \
  '# A comment describing the hazard: printf '"'"'%s'"'"' "$x" | grep -q described' \
  '# printf "%s" "$x" | grep -q still-a-comment | grep -q twice'; then
  silent "the shape written in a comment does not fire"
fi

# 8. A here-document body is data to the enclosing script.
reset_case
if sub_line "hazard in a here-document body" \
  'printf '"'"'%s'"'"' "$x" | grep -q inside-a-heredoc-body' \
  'cat other.txt | grep -q still-inside | rg --quiet twice'; then
  silent "the shape written inside a here-document body does not fire"
fi

# 9. A file that does not enable pipefail is NOT exempt, because shell options
#    are dynamic. This case asserted the opposite until the closure self-check
#    asked what the rule would still pass on: `scripts/regression/run-cli-probes.sh`
#    sets `-euo pipefail` and sources every file under `scenarios/`, and two of
#    those files were reported as immune while running under the runner's
#    options. Case 9b is that demonstration; this is the rule that follows from
#    it.
reset_case
if sub_line "drop pipefail, keep the shape" \
  'set -euo pipefail' \
  'set -eu
cat data.txt | grep -q unguarded'; then
  fires_on "the shape fires in a file that does not enable pipefail itself" \
    short-circuit-under-pipefail '| grep -q unguarded'
fi

# 9b. The reason, executed rather than asserted. A scenario sourced into a
#     pipefail runner reports a pattern that is present as absent — so "this file
#     does not set pipefail" is not a statement about the shell its pipelines run
#     in, and a scanner cannot prove nothing sources a file.
cat > "$WORK/dynamic-runner.sh" <<'RUNNER'
#!/usr/bin/env bash
set -euo pipefail
export VERDICT
source "$1"
RUNNER
cat > "$WORK/dynamic-scenario.sh" <<'SCENARIO'
#!/usr/bin/env bash
# No `set` at all: this file inherits whatever the caller chose.
if { printf 'MATCHME\n'; sleep 0.2; printf 'tail\n'; } | grep -q MATCHME; then
  echo matched > "$VERDICT"
else
  echo unmatched > "$VERDICT"
fi
SCENARIO
# The verdict goes to a file, not to stdout. The first version of this case read
# combined output and compared it for equality, which passed on macOS bash 3.2
# and failed on the Linux runner, where bash prints
# `printf: write error: Broken pipe` to stderr before the branch is taken. The
# diagnostic is the mechanism working; reading it as part of the answer was the
# assertion's mistake, and it is the fourth time in this FR that a proxy for a
# fact turned out to be the wrong subject.
rm -f "$WORK/verdict"
VERDICT="$WORK/verdict" bash "$WORK/dynamic-runner.sh" "$WORK/dynamic-scenario.sh" >/dev/null 2>&1 || true
if [[ ! -f "$WORK/verdict" ]]; then
  fail "the dynamic-scope demonstration left no verdict, so nothing was asserted"
elif [[ "$(cat "$WORK/verdict")" == "unmatched" ]]; then
  pass "a file that sets no options reports a present pattern as absent when sourced into a pipefail shell"
else
  fail "the dynamic-scope demonstration did not reproduce: verdict was '$(cat "$WORK/verdict")'"
fi

# 10. A `|` inside a double-quoted pattern is not a pipe. Already in the probe;
#     asserted directly so a regression names itself rather than showing up as
#     one extra finding somewhere in case 1.
reset_case
if sub_line "quoted alternation only" \
  'grep -q "a|b" data.txt || echo no-alternation' \
  'grep -q "a|b|c|d" data.txt || echo no-alternation'; then
  silent "a quoted alternation is not a pipeline"
fi

# 11. `--` ends the option list, so a pattern that looks like a flag is a
#     pattern. Asserted on its own for the same reason as case 10.
reset_case
if sub_line "pattern after --" \
  'cat data.txt | grep -F -- -q literal-dash-q' \
  'cat data.txt | grep -F -- -q -quiet --silent'; then
  silent "a -q pattern after -- is not a quiet flag"
fi

# 12. `grep -c` counts and therefore reads to end of input. The gate flagged
#     three of these on its first run over this repository, because `-eq` in the
#     enclosing `[[ ]]` matched "a short-flag cluster containing q".
reset_case
if sub_line "counting grep in an arithmetic test" \
  '[[ "$(cat data.txt | grep -c .)" -eq 3 ]] || echo wrong-count' \
  '[[ "$(cat data.txt | grep -c .)" -eq 3 && "$(cat data.txt | grep -c x)" -eq 0 ]] || echo wrong'; then
  silent "a counting grep inside an arithmetic test does not fire"
fi

# 13. The scanned set follows git, not a list. A gate registered tomorrow is
#     governed tomorrow, and this is asserted by adding one rather than by
#     reading the scanner's source (§4.4 shape 2, and FR-143's precedent).
reset_case
if before_files="$(ruby "$REPO_ROOT/$GATE" --repo-root "$CASE" --list-files | wc -l | tr -d ' ')"; then
  cat > "$CASE/freshly-tracked.sh" <<'FRESH'
#!/usr/bin/env bash
set -euo pipefail
echo tracked-after-the-scanner-was-written
FRESH
  git -C "$CASE" add -A >/dev/null
  after_files="$(ruby "$REPO_ROOT/$GATE" --repo-root "$CASE" --list-files | wc -l | tr -d ' ')"
  listed="$(ruby "$REPO_ROOT/$GATE" --repo-root "$CASE" --list-files)"
  if [[ "$after_files" -eq $((before_files + 1)) ]] &&
     grep -qxF "freshly-tracked.sh" <<< "$listed"; then
    pass "a file tracked after the scanner was written is governed without editing it"
  else
    fail "the governed set did not follow git ($before_files -> $after_files)"
  fi
else
  fail "the scanner could not list the governed set"
fi

# 14. The summary names how many files were read, so "0 findings" and "read
#     nothing" are different sentences. The count is derived from the scratch
#     tree rather than restated: two files are added, so the number must be the
#     probe plus those two.
reset_case
cat > "$CASE/plain.sh" <<'PLAIN'
#!/usr/bin/env bash
set -eu
grep -q ungoverned <<< "$(cat data.txt)"
PLAIN
cat > "$CASE/another.sh" <<'ANOTHER'
#!/usr/bin/env bash
set -euo pipefail
grep -q wanted <<< "$(cat data.txt)"
ANOTHER
git -C "$CASE" add -A >/dev/null
tracked="$(git -C "$CASE" ls-files '*.sh' | wc -l | tr -d ' ')"
run_gate
if [[ "$GATE_STATUS" -eq 0 ]] &&
   grep -q "^pipefail short-circuit: PASS ($tracked tracked shell file(s) scanned, 0 finding(s))" <<< "$GATE_OUT"; then
  pass "the summary reports how many files it read, and the count follows git ($tracked)"
else
  fail "the summary does not report the scanned count: $(tail -1 <<< "$GATE_OUT")"
fi

# 15. A file that ends inside a here-document was never fully read, so a clean
#     result over it is not evidence of anything. The backstop that does not
#     depend on the lexer being right.
reset_case
if sub_line "unterminated here-document" \
  'INNER' \
  'NOT-THE-TERMINATOR'; then
  run_gate
  if [[ "$GATE_STATUS" -ne 0 ]] && grep -q "\[unclosed-heredoc\]" <<< "$GATE_OUT"; then
    pass "a file ending inside a here-document is reported, not silently truncated"
  else
    fail "an unterminated here-document did not produce a finding"
  fi
fi

# 16. The mechanism, deterministically.
#
#     The producer is still writing when the reader leaves *by construction* —
#     it sleeps after emitting the match — so this reproduces on any machine,
#     under any load, with any grep, without depending on the pipe buffer. The
#     probabilistic form belongs in QA-195 as the field observation that found
#     the defect; it does not belong in a gate.
#
#     Written into a here-document and executed, rather than inline, so this
#     file does not itself contain the shape it forbids.
cat > "$WORK/mechanism.sh" <<'MECHANISM'
#!/usr/bin/env bash
set -uo pipefail
piped=0
hered=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  { printf 'MATCHME\n'; sleep 0.2; printf 'tail\n'; } | grep -q MATCHME || piped=$((piped + 1))
  out="$( { printf 'MATCHME\n'; sleep 0.2; printf 'tail\n'; } )"
  grep -q MATCHME <<< "$out" || hered=$((hered + 1))
done
echo "$piped $hered"
MECHANISM
if MECH="$(bash "$WORK/mechanism.sh")"; then
  MECH_PIPED="${MECH%% *}"
  MECH_HERED="${MECH##* }"
  if [[ "$MECH_PIPED" -eq 10 ]]; then
    pass "pipefail reports a matched pattern as unmatched when the producer outlives the reader ($MECH_PIPED/10)"
  else
    fail "the mechanism did not reproduce: $MECH_PIPED/10 piped failures, expected 10"
  fi
  if [[ "$MECH_HERED" -eq 0 ]]; then
    pass "the here-string form reports the match every time ($((10 - MECH_HERED))/10)"
  else
    fail "the here-string form failed $MECH_HERED/10 times"
  fi
else
  fail "the mechanism probe did not run"
fi

# 18. `head` always short-circuits — there is no flag that makes it read to the end —
#     so it is flagged with no flag test at all. FR-146 measured it at 10/10 deaths on a
#     129 KB producer where `grep -q` on 90 KB managed 8-13 in 400.
reset_case
if sub_line "head as a downstream stage" \
  'head -5 data.txt' \
  'cat data.txt | head -5'; then
  fires_on "a downstream head fires with no flag needed" short-circuit-under-pipefail \
    '| head -5'
fi

# 19. And inside a command substitution, where the assignment carries the status to `set -e`.
reset_case
if sub_line "head inside a substitution" \
  'rows="$(cat data.txt)"' \
  'rows="$(cat data.txt | head -1)"'; then
  fires_on "a head inside \$( ) fires" short-circuit-under-pipefail '| head -1'
fi

# 20. The remedies must not fire. All three read to end of input, or use no pipe at all, and
#     all three were measured against a 1.3 MB producer. Without this the rule would be
#     unusable: every fix anyone applied would light it up again.
reset_case
if sub_line "more of the remedies" \
  "cat data.txt | awk 'NR<=5'" \
  "cat data.txt | awk 'NR<=5'
cat data.txt | sed -n '2,7p'
printf '%s' \"\${rows%%\$'\\n'*}\""; then
  silent "sed -n, awk and first-line expansion are not short-circuits"
fi

# 21. `head` reading a *file* is the first stage of nothing. There is no producer upstream to
#     kill, so it must stay silent — otherwise the rule would forbid the one spelling that is
#     unambiguously safe.
reset_case
silent "head as the first stage, reading a file, does not fire"

# 22. The mechanism for `head`, deterministically, and the part that makes it worse than the
#     `-q` family: the run does not merely report wrongly, it *stops*. The probe records
#     whether the line after the pipeline ever executed. Written to a file, for the reason
#     case 9b was rewritten.
cat > "$WORK/head-runner.sh" <<'RUNNER'
#!/usr/bin/env bash
set -euo pipefail
export VERDICT
printf 'reached=no\n' > "$VERDICT"
{ printf 'line1\n'; sleep 0.2; printf 'line2\n'; } | head -1 >/dev/null
printf 'reached=yes\n' > "$VERDICT"
RUNNER
rm -f "$WORK/head-verdict"
VERDICT="$WORK/head-verdict" bash "$WORK/head-runner.sh" >/dev/null 2>&1 && HEAD_STATUS=0 || HEAD_STATUS=$?
HEAD_REACHED="$(cat "$WORK/head-verdict" 2>/dev/null || echo 'reached=<none>')"
# The subject is the **truncation**, so that is what is asserted: a non-zero status and a line
# after the pipeline that never executed. The exact code is a platform detail and asserting it
# was a real failure here — this case first read `-eq 141`, which holds on bash 3.2.57 and 5.3.9
# on macOS and is **1** on the Linux runner, where the producer reports EPIPE rather than dying
# of SIGPIPE. Same defect, same consequence, different number. The observed status is printed
# either way so the difference stays visible instead of being smoothed over.
if [[ "$HEAD_STATUS" -ne 0 && "$HEAD_REACHED" == "reached=no" ]]; then
  pass "a diagnostic head ends the run (status=$HEAD_STATUS) and the line after it never executes"
else
  fail "the head truncation did not reproduce: status=$HEAD_STATUS $HEAD_REACHED"
fi

# 17. And the gate holds on the repository it governs. Case 1 proves the rule can
#     stay quiet on a probe someone wrote to be quiet; this proves it stays quiet
#     on the tree, which is the claim the CI step actually makes.
run_gate "$REPO_ROOT"
if [[ "$GATE_STATUS" -eq 0 ]] && grep -q '^pipefail short-circuit: PASS' <<< "$GATE_OUT"; then
  pass "the repository as it stands has no short-circuit under pipefail"
else
  fail "the repository does not pass its own scanner: $(tail -3 <<< "$GATE_OUT" | tr '\n' ' ')"
fi

echo ""
echo "=== Pipefail short-circuit: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
