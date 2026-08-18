#!/usr/bin/env bash
#
# FR-140: fixtures for the governance execution cost ledger and its budget.
#
# The ledger exists because "add one more gate" had been a zero-cost decision
# for fourteen FRs. Every case below is a way it could go on looking complete
# while the thing it measures moves underneath it — a gate whose step nobody
# recorded, a ceiling with no reason, a total compared against a measurement
# that is missing steps.
#
# Case 6 is the one that matters most. A ledger that only checks coverage — every
# step has a number — passes on a pipeline of any length whatsoever, and would
# have reported PASS on all 80 minutes that prompted this FR. So one case lowers
# the ceiling below the recorded total and requires the failure, which is the
# only assertion here that observes the budget doing arithmetic on real seconds
# rather than existing.
#
# Case 8 is a differential, not a fixture: FR-140 rewrote RustLexer.mask_literals
# for speed, and the claim is that it is byte-identical to what it replaced. The
# suites that use it passing is a weaker statement — it would also hold for a
# lexer that masks slightly differently in a region no current fixture inspects —
# so the equivalence is asserted directly against the implementation in git
# history, over every tracked Rust file and over the constructs the corpus may
# not contain.
#
# Safety: cases run against throwaway repositories under $TMPDIR. The working
# tree is read but never written, no daemon starts, no provider is invoked, and
# nothing here contacts GitHub — verification is offline by construction and
# only --refresh, which these cases never call, talks to the API.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/ci-cost.rb"
LEDGER="config/governance/ci-step-cost.json"
SURFACE="config/governance/qa-gate-surface.json"

for required in jq ruby git; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "missing required command: $required" >&2
    exit 1
  }
done

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr140-ci-cost.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# A case is a small self-contained repository: one workflow that runs one gate,
# an enforcement surface declaring it, a cost ledger, and the libraries the gate
# requires. Small on purpose — the provenance rule reads git history, so each
# case needs a history of its own.
new_case() {
  local dir="$WORK/$1"
  mkdir -p "$dir/.github/workflows" "$dir/config/governance" "$dir/scripts/qa" "$dir/scripts/lib"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  cp "$REPO_ROOT/scripts/lib/workflow_model.rb" "$dir/scripts/lib/workflow_model.rb"
  cp "$REPO_ROOT/scripts/lib/ci_env.rb" "$dir/scripts/lib/ci_env.rb"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$dir/scripts/qa/test-alpha.sh"
  chmod 755 "$dir/scripts/qa/test-alpha.sh"

  cat > "$dir/.github/workflows/ci.yml" <<'YAML'
name: CI
on:
  push:
    branches: [main]
jobs:
  governance:
    name: Governance
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Alpha gate
        id: alpha
        run: ./scripts/qa/test-alpha.sh
  parity:
    name: Parity
    runs-on: ubuntu-latest
    steps:
      - name: Compare
        run: echo compare
YAML

  cat > "$dir/$SURFACE" <<'JSON'
{
  "scripts": [
    {
      "path": "scripts/qa/test-alpha.sh",
      "enforcement": "ci-required",
      "workflow": ".github/workflows/ci.yml",
      "job": "governance"
    }
  ]
}
JSON

  git -C "$dir" init -q
  git -C "$dir" config user.email qa@local
  git -C "$dir" config user.name qa
  git -C "$dir" add -A
  git -C "$dir" commit -qm "workflow"
  echo "$dir"
}

# A ledger recording both jobs, every declared step measured, at the current
# HEAD. Whatever the caller mutates afterwards is the defect under test.
seed_ledger() {
  local dir="$1" sha
  sha="$(git -C "$dir" rev-parse HEAD)"
  cat > "$dir/$LEDGER" <<JSON
{
  "version": 1,
  "description": "fixture",
  "workflow": ".github/workflows/ci.yml",
  "budget": {
    "jobs": ["governance", "parity"],
    "seconds": 600,
    "decidedBy": "docs/design_doc/orchestrator/153-governance-execution-cost.md",
    "reason": "fixture ceiling",
    "reviewWhen": "fixture review condition"
  },
  "pendingMeasurement": {},
  "measurement": { "runId": "1", "headSha": "$sha" },
  "criticalPath": {
    "description": "fixture",
    "full": { "seconds": 300, "chain": ["governance"] },
    "deferred": { "seconds": 300, "chain": ["governance"] },
    "tieredSteps": 0,
    "tieredSeconds": 0
  },
  "jobs": {
    "governance": {
      "seconds": 300,
      "unattributed": 290,
      "steps": { "Checkout": 2, "Alpha gate": 8 }
    },
    "parity": {
      "seconds": 100,
      "unattributed": 99,
      "steps": { "Compare": 1 }
    }
  }
}
JSON
}

edit_ledger() {
  local dir="$1" filter="$2"
  jq "$filter" "$dir/$LEDGER" > "$dir/$LEDGER.tmp"
  mv "$dir/$LEDGER.tmp" "$dir/$LEDGER"
}

run_gate() { (cd "$1" && ruby "$GATE" > "$WORK/out.log" 2>&1); }

echo "=== FR-140: governance execution cost (negative fixtures) ==="
echo ""

# Positive control. Without it every case below could be passing because the
# gate is broken rather than because the defect is detected.
d="$(new_case control)"
seed_ledger "$d"
if run_gate "$d"; then
  pass "positive control: a complete, in-budget ledger passes"
else
  fail "positive control: an intact ledger did not pass"
  cat "$WORK/out.log" >&2
fi

# 1. A ci-required gate whose step nobody recorded.
#
#    The mutation is an ADDED step, not a deleted record. Deletion is the case
#    the author has in mind and a hand-maintained list survives it; a new gate
#    landing outside the list is how this decays in practice, and it is the exact
#    shape FR-140 was filed about — the surface grew from 45 entries to 65 while
#    nothing was obliged to notice.
d="$(new_case unrecorded-step)"
seed_ledger "$d"
printf '#!/usr/bin/env bash\nexit 0\n' > "$d/scripts/qa/test-beta.sh"
chmod 755 "$d/scripts/qa/test-beta.sh"
cat >> "$d/.github/workflows/ci.yml" <<'YAML'
YAML
# The step is inserted by loading and rewriting the workflow, so the insertion
# depends on `jobs.governance.steps` still being where this reads it. A rename
# there raises inside the block; unwrapped, that took the run down before this
# case or any after it reported (FR-143).
if fixture_mutate "unrecorded-step" "$d/.github/workflows/ci.yml" ruby -ryaml -e '
doc = YAML.load_file(ARGV[0])
doc["jobs"]["governance"]["steps"] << { "name" => "Beta gate", "id" => "beta", "run" => "./scripts/qa/test-beta.sh" }
File.write(ARGV[0], doc.to_yaml)
' "$d/.github/workflows/ci.yml"; then
  jq '.scripts += [{"path": "scripts/qa/test-beta.sh", "enforcement": "ci-required", "workflow": ".github/workflows/ci.yml", "job": "governance"}]' \
    "$d/$SURFACE" > "$d/$SURFACE.tmp"
  mv "$d/$SURFACE.tmp" "$d/$SURFACE"
  if run_gate "$d"; then
    fail "a ci-required gate whose step has no cost record was accepted"
  elif grep -q "step 'Beta gate' has no cost record" "$WORK/out.log" &&
       grep -q "scripts/qa/test-beta.sh" "$WORK/out.log"; then
    pass "an added gate with no cost record fails, naming both the step and the gate"
  else
    fail "the gate failed but not because of the unrecorded step"
    cat "$WORK/out.log" >&2
  fi
fi

# 2. A record naming a step the workflow no longer defines. The other direction
#    of the same coverage rule: a renamed step leaves a number attributed to
#    nothing, and the total silently stops meaning what it says.
d="$(new_case orphan-record)"
seed_ledger "$d"
edit_ledger "$d" '.jobs.governance.steps["Removed gate"] = 40'
if run_gate "$d"; then
  fail "a cost record for a step the workflow does not define was accepted"
elif grep -q "cost record for step 'Removed gate'" "$WORK/out.log"; then
  pass "a record naming a step the job no longer defines fails"
else
  fail "the gate failed but not because of the orphan record"
  cat "$WORK/out.log" >&2
fi

# 3. A ceiling with no reason. FR-140 requires the budget to be a decision:
#    a number with no rationale and no review condition is indistinguishable
#    from whatever the cost happened to be the day someone wrote it down.
d="$(new_case ceiling-without-reason)"
seed_ledger "$d"
edit_ledger "$d" 'del(.budget.reason)'
if run_gate "$d"; then
  fail "a budget with no written reason was accepted"
elif grep -q "the budget has no reason" "$WORK/out.log"; then
  pass "a ceiling with no reason fails"
else
  fail "the gate failed but not because of the missing reason"
  cat "$WORK/out.log" >&2
fi

# 4. Provenance. A measurement from a commit that is not in this history is
#    describing somebody else's pipeline.
d="$(new_case foreign-measurement)"
seed_ledger "$d"
edit_ledger "$d" '.measurement.headSha = "0000000000000000000000000000000000000000"'
if run_gate "$d"; then
  fail "a measurement from a commit outside this history was accepted"
elif grep -q "not an ancestor of HEAD" "$WORK/out.log"; then
  pass "a measurement that is not an ancestor of HEAD fails"
else
  fail "the gate failed but not because of the measurement's provenance"
  cat "$WORK/out.log" >&2
fi

# 4b. A recorded critical path that the graph no longer produces (FR-174).
#
#     The mutation is a `needs` edge added to the workflow, not an edited
#     number. Editing the number is the case the author has in mind and it is
#     the one nobody commits by accident; the way a latency goes stale in
#     practice is that the graph moves underneath it while every per-job second
#     stays correct, so nothing else in this ledger changes and no other check
#     here has an opinion. Here `parity` gains a dependency on `governance`,
#     which makes the longest chain 300 + 100 rather than 300, and the recorded
#     300 becomes a description of a workflow that no longer exists.
d="$(new_case critical-path-drift)"
seed_ledger "$d"
# ruby, not python3: FR-144 recorded a gate that used python3 in a job which
# never provided it, and check_command_sources exists because of it.
ruby -e '
  path = ARGV[0]
  text = File.read(path)
  text.sub!("  parity:\n    name: Parity\n    runs-on: ubuntu-latest\n",
            "  parity:\n    name: Parity\n    runs-on: ubuntu-latest\n    needs:\n      - governance\n") ||
    abort("fixture premise gone: the parity job no longer has the shape this mutation edits")
  File.write(path, text)
' "$d/.github/workflows/ci.yml"
git -C "$d" add -A >/dev/null 2>&1
git -C "$d" commit -qm "add a needs edge" >/dev/null 2>&1
edit_ledger "$d" ".measurement.headSha = \"$(git -C "$d" rev-parse HEAD)\""
if run_gate "$d"; then
  fail "a critical path describing a superseded graph was accepted"
elif grep -q "criticalPath.full records 300s but the graph gives 400s" "$WORK/out.log"; then
  pass "a critical path the needs graph no longer produces fails, with both numbers"
else
  fail "the gate failed but not because the recorded critical path went stale"
  cat "$WORK/out.log" >&2
fi

# 4c. The field missing altogether. Separate from 4b because a ledger written
#     before FR-174 has no `criticalPath` at all, and "absent" must not read as
#     "nothing to check" — that is how the number FR-174 argues from would go
#     back to being each reader's problem.
d="$(new_case critical-path-absent)"
seed_ledger "$d"
edit_ledger "$d" 'del(.criticalPath)'
if run_gate "$d"; then
  fail "a ledger with no critical path at all was accepted"
elif grep -q "records no criticalPath" "$WORK/out.log"; then
  pass "a ledger that records no critical path fails"
else
  fail "the gate failed but not because the critical path was missing"
  cat "$WORK/out.log" >&2
fi

# 5. A pending-measurement annotation left on a step that has since been
#    measured. Otherwise the budget stays unenforced forever behind a note
#    nobody revisited — the same failure mode as a knownFailing that outlived
#    its failure.
d="$(new_case stale-pending)"
seed_ledger "$d"
edit_ledger "$d" '.pendingMeasurement["Alpha gate"] = "measured long ago"'
if run_gate "$d"; then
  fail "a pending-measurement annotation on a measured step was accepted"
elif grep -q "pending measurement but has a recorded cost" "$WORK/out.log"; then
  pass "an annotation outliving the measurement it excused fails"
else
  fail "the gate failed but not because of the stale annotation"
  cat "$WORK/out.log" >&2
fi

# 6. Over budget.
#
#    This is the assertion that separates a cost ledger from a cost *budget*.
#    Every case above is satisfied by a ledger that records numbers and compares
#    none of them; this one requires the sum to be computed from the recorded
#    seconds and tested. The ceiling is lowered rather than the durations raised,
#    because FR-140 asks specifically for proof that the ceiling is evaluated
#    against real recorded time and not spinning.
d="$(new_case over-budget)"
seed_ledger "$d"
edit_ledger "$d" '.budget.seconds = 399'
if run_gate "$d"; then
  fail "a recorded total over the ceiling was accepted"
elif grep -q "governance costs 400s against a 399s budget, over by 1s" "$WORK/out.log"; then
  pass "a total over the ceiling fails, with the arithmetic in the diagnostic"
else
  fail "the gate failed but not because the total exceeded the ceiling"
  cat "$WORK/out.log" >&2
fi

# 6b. The failure has to be usable. "Over budget" without saying where the time
#     went leaves the reader to go and do the attribution by hand, which is the
#     state this FR started from.
if grep -q "8s  Alpha gate" "$WORK/out.log" && grep -q "governance: 300s" "$WORK/out.log"; then
  pass "the over-budget diagnostic breaks the total down per job and per step"
else
  fail "the over-budget diagnostic did not name where the time went"
  cat "$WORK/out.log" >&2
fi

# 7. --write refused when no human is present. Same refusal as the other five
#    governance writers, asserted on the diagnostic as well as the exit code:
#    an exit 2 alone is also what a crashed interpreter produces.
d="$(new_case unattended-write)"
seed_ledger "$d"
set +e
(cd "$d" && CI=1 ruby "$GATE" --refresh --write > "$WORK/write.log" 2>&1)
STATUS=$?
set -e
if [[ "$STATUS" -eq 0 ]]; then
  fail "--write was allowed to run unattended"
elif grep -q "refusing --write under CI" "$WORK/write.log"; then
  pass "--write is refused under CI, and the diagnostic names the indicator"
else
  fail "--write failed but not because it was refused"
  cat "$WORK/write.log" >&2
fi

# 8. What RustLexer.mask_literals is supposed to produce, stated rather than
#    compared.
#
#    FR-140 rewrote it from a `String#[]` walk to a character array walk for a
#    25x speedup, on the claim that behaviour was untouched. That claim was
#    checked during governance against the previous implementation over all 415
#    tracked Rust files and 7000 random inputs (QA-191 records both), but a
#    same-as-last-commit differential is the wrong thing to leave behind: it
#    would forbid every deliberate future change to this function rather than
#    catching accidental ones.
#
#    So what stays is a known-answer table. Each expectation is written out, not
#    generated, because a baseline captured from the implementation asserts only
#    that the implementation has not changed — including from a state that was
#    already wrong. These are the cases FR-134 built the lexer for and the ones
#    a rewrite is most likely to get wrong: a brace inside a literal, nested
#    block comments, raw strings at several hash depths, a lifetime that is not
#    a char literal, and literals unterminated at end of file.
if ruby - "$REPO_ROOT" > "$WORK/known.log" 2>&1 <<'RUBY'; then
$LOAD_PATH.unshift File.join(ARGV[0], "scripts", "lib")
require "rust_lexer"

# input => expected masked output. Masking replaces every character of a
# comment or literal — delimiters included — with a space, and leaves newlines
# in place so callers can still index by line.
#
# Expectations are spelled `"code" + " " * n` rather than written as literal
# runs of spaces: the count is the assertion, and a reviewer cannot check a run
# of spaces by looking at it. Each `n` below is the length of the construct
# being masked, counted from the input on the same line.
def sp(count)
  " " * count
end

CASES = {
  # "xy" is 4 characters
  %(let a = "xy";)                => %(let a = ) + sp(4) + %(;),
  # `// c` is 4; the space before it is code and survives
  %(let a = 1; // c)              => %(let a = 1; ) + sp(4),
  # a brace inside a literal is not a brace — the reason this lexer exists
  %(fn f() { g("{"); })           => "fn f() { g(" + sp(3) + "); }",
  # nested block comment, 17 characters, closed only by the outer `*/`
  %(/* a /* b */ c */ let x = 1;) => sp(17) + %( let x = 1;),
  # r"a" is 4
  %(let s = r"a"; let t = 2;)     => %(let s = ) + sp(4) + %(; let t = 2;),
  # r#"a"# is 6
  %(let s = r#"a"#; let t = 2;)   => %(let s = ) + sp(6) + %(; let t = 2;),
  # r##"a"#"## is 10 — the inner `"#` does not close it
  %(let s = r##"a"#"##; let t=2;) => %(let s = ) + sp(10) + %(; let t=2;),
  # b"ab" is 5
  %(let s = b"ab"; let t = 2;)    => %(let s = ) + sp(5) + %(; let t = 2;),
  # lifetimes are code, not char literals: nothing is masked
  %(fn f<'a>(x: &'a str) {})      => %(fn f<'a>(x: &'a str) {}),
  # 'a' is 3
  %(let c = 'a'; let d = 2;)      => %(let c = ) + sp(3) + %(; let d = 2;),
  # '\n' is 4
  %(let c = '\\n'; let d = 2;)    => %(let c = ) + sp(4) + %(; let d = 2;),
  # a literal spanning a newline: 2 masked, the newline kept, 2 masked
  %(let s = "a\nb"; let t = 2;)   => %(let s = ) + sp(2) + "\n" + sp(2) + %(; let t = 2;),
  # unterminated string runs to end of file: 13 masked, newline, 10 masked
  %(let s = "never closed\nlet t = 2;) => %(let s = ) + sp(13) + "\n" + sp(10),
  # unterminated block comment does the same: 15 masked, newline, 10 masked
  %(let a = 1; /* never closed\nlet b = 2;) => %(let a = 1; ) + sp(15) + "\n" + sp(10),
  # a literal that starts at offset 0 and spans a line: 6 masked, newline, 5
  %("multi\nline")                => sp(6) + "\n" + sp(5)
}

bad = []
CASES.each do |input, expected|
  actual = RustLexer.mask_literals(input)
  next if actual == expected

  bad << "  input    #{input.inspect}\n  expected #{expected.inspect}\n  actual   #{actual.inspect}"
end

if bad.empty?
  puts "mask_literals matches all #{CASES.length} known-answer cases"
  exit 0
end
warn bad.join("\n\n")
exit 1
RUBY
  pass "$(tail -1 "$WORK/known.log")"
else
  fail "RustLexer.mask_literals does not produce the masking its callers rely on"
  cat "$WORK/known.log" >&2
fi

echo ""
echo "=== governance execution cost: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
