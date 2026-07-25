#!/usr/bin/env bash
#
# FR-134 requirement 8: fixtures for the CI liveness ledger.
#
# ci-liveness.rb exists because the enforcement surface ledger classifies
# scripts/qa/* and nothing else, so a liveness rule scoped to it would never
# have looked at boundary-coverage — which was red for six consecutive runs
# before anyone noticed. Every case below is a way that ledger could go on
# saying something true-looking about a pipeline that had moved underneath it.
#
# Safety: every case runs against a throwaway git repository under $TMPDIR. The
# working tree is never written, no daemon starts, no provider is invoked, and
# nothing here contacts GitHub — verification is offline by construction and
# only --refresh, which these cases never call, talks to the API.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/ci-liveness.rb"
LEDGER="config/governance/ci-job-liveness.json"

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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr134-ci-liveness.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# A case is a small self-contained repository: one workflow, one ledger, the
# gate and the libraries it requires. Small on purpose — the freshness rule is
# about git history, so each case needs its own history to manipulate, and
# copying this repository's would make that slow and shared.
new_case() {
  local dir="$WORK/$1"
  mkdir -p "$dir/.github/workflows" "$dir/config/governance" "$dir/scripts/qa" "$dir/scripts/lib"
  cp "$REPO_ROOT/$GATE" "$dir/$GATE"
  cp "$REPO_ROOT/scripts/lib/workflow_model.rb" "$dir/scripts/lib/workflow_model.rb"
  cp "$REPO_ROOT/scripts/lib/ci_env.rb" "$dir/scripts/lib/ci_env.rb"
  cat > "$dir/.github/workflows/ci.yml" <<'YAML'
name: CI
on:
  push:
    branches: [main]
jobs:
  alpha:
    name: Alpha
    runs-on: ubuntu-latest
    steps:
      - run: echo alpha
  beta:
    name: Beta (${{ matrix.os }})
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - run: echo beta
YAML
  git -C "$dir" init -q
  git -C "$dir" config user.email qa@local
  git -C "$dir" config user.name qa
  git -C "$dir" add -A
  git -C "$dir" commit -qm "workflow"
  echo "$dir"
}

# Writes a ledger recording both jobs green at the current HEAD, then whatever
# the caller mutates is the defect under test.
seed_ledger() {
  local dir="$1" sha
  sha="$(git -C "$dir" rev-parse HEAD)"
  cat > "$dir/$LEDGER" <<JSON
{
  "version": 1,
  "description": "fixture",
  "workflows": [
    {
      "path": ".github/workflows/ci.yml",
      "inScope": true,
      "jobs": {
        "alpha": { "conclusion": "success", "runId": "1", "headSha": "$sha" },
        "beta": { "conclusion": "success", "runId": "1", "headSha": "$sha" }
      }
    }
  ]
}
JSON
}

run_gate() { (cd "$1" && ruby "$GATE" > "$WORK/out.log" 2>&1); }

echo "=== FR-134: CI job liveness (negative fixtures) ==="
echo ""

# Positive control. Without this every case below could be passing because the
# gate is broken rather than because the defect is detected.
d="$(new_case control)"
seed_ledger "$d"
if run_gate "$d"; then
  pass "positive control: a complete, fresh, all-green ledger passes"
else
  fail "positive control: an intact ledger did not pass"
  cat "$WORK/out.log" >&2
fi

# 1. A job added to the workflow with no record. This is the case that decides
#    whether the object list is discovered or enumerated: an enumerated list
#    would simply never mention the new job.
d="$(new_case new-job)"
seed_ledger "$d"
cat >> "$d/.github/workflows/ci.yml" <<'YAML'
  gamma:
    name: Gamma
    runs-on: ubuntu-latest
    steps:
      - run: echo gamma
YAML
if run_gate "$d"; then
  fail "a workflow job with no liveness record was accepted"
elif grep -q "job 'gamma' has no liveness record" "$WORK/out.log"; then
  pass "a job added to the workflow with no record fails, and is named"
else
  fail "the gate failed but not because of the unrecorded job"
  cat "$WORK/out.log" >&2
fi

# 2. A red job with no annotation. The whole point: boundary-coverage was red
#    six runs running and nothing was obliged to say so.
d="$(new_case unannotated-red)"
seed_ledger "$d"
jq '(.workflows[0].jobs.alpha.conclusion) = "failure"' "$d/$LEDGER" > "$d/$LEDGER.tmp"
mv "$d/$LEDGER.tmp" "$d/$LEDGER"
if run_gate "$d"; then
  fail "a job recorded as failing with no known-failing annotation was accepted"
elif grep -q "is not marked known-failing" "$WORK/out.log"; then
  pass "a red job with no known-failing reference and reason fails"
else
  fail "the gate failed but not because of the unannotated red job"
  cat "$WORK/out.log" >&2
fi

# 3. An annotation without a reference. "Known failing" with nobody named is
#    how a permanent exception gets written in one line.
d="$(new_case annotation-without-owner)"
seed_ledger "$d"
jq '(.workflows[0].jobs.alpha) = {conclusion: "failure", runId: "1", headSha: .workflows[0].jobs.alpha.headSha, knownFailing: {reason: "it is broken"}}' \
  "$d/$LEDGER" > "$d/$LEDGER.tmp"
mv "$d/$LEDGER.tmp" "$d/$LEDGER"
if run_gate "$d"; then
  fail "a known-failing annotation with no reference was accepted"
elif grep -q "known-failing without a reference" "$WORK/out.log"; then
  pass "a known-failing annotation with no owner fails"
else
  fail "the gate failed but not because of the missing reference"
  cat "$WORK/out.log" >&2
fi

# 4. The freshness rule. A record taken before the workflow last changed
#    describes a pipeline that no longer exists, and this is the only thing
#    stopping the ledger from becoming the stale declaration this FR is about.
d="$(new_case stale-record)"
seed_ledger "$d"
git -C "$d" add -A
git -C "$d" commit -qm "ledger"
printf '\n      - run: echo added\n' >> "$d/.github/workflows/ci.yml"
git -C "$d" add -A
git -C "$d" commit -qm "change the workflow after the record was taken"
if run_gate "$d"; then
  fail "a record taken before the workflow changed was accepted as current"
elif grep -q "before .github/workflows/ci.yml last changed" "$WORK/out.log"; then
  pass "a record predating the workflow's last change is stale and fails"
else
  fail "the gate failed but not because the record was stale"
  cat "$WORK/out.log" >&2
fi

# 5. An annotation left on a job that has recovered. Otherwise the first real
#    failure after a fix is pre-excused by a note nobody revisited.
d="$(new_case stale-annotation)"
seed_ledger "$d"
jq '(.workflows[0].jobs.alpha.knownFailing) = {reference: "FR-000", reason: "fixed long ago"}' \
  "$d/$LEDGER" > "$d/$LEDGER.tmp"
mv "$d/$LEDGER.tmp" "$d/$LEDGER"
if run_gate "$d"; then
  fail "a known-failing annotation on a green job was accepted"
elif grep -q "marked known-failing but last concluded success" "$WORK/out.log"; then
  pass "an annotation outliving the failure it excused fails"
else
  fail "the gate failed but not because of the stale annotation"
  cat "$WORK/out.log" >&2
fi

# 6. A workflow excluded from scope with no reason. Exclusion has to cost a
#    sentence, or it is the cheapest way to make any job disappear.
d="$(new_case unexplained-exclusion)"
seed_ledger "$d"
jq '(.workflows[0]) = {path: ".github/workflows/ci.yml", inScope: false}' \
  "$d/$LEDGER" > "$d/$LEDGER.tmp"
mv "$d/$LEDGER.tmp" "$d/$LEDGER"
if run_gate "$d"; then
  fail "a workflow excluded with no reason was accepted"
elif grep -q "excluded from liveness with no reason" "$WORK/out.log"; then
  pass "excluding a workflow without a written reason fails"
else
  fail "the gate failed but not because of the unexplained exclusion"
  cat "$WORK/out.log" >&2
fi

# 7. A whole workflow file with no entry at all. Adding a workflow is how a
#    pipeline grows, and the ledger has to notice the file, not just its jobs.
d="$(new_case new-workflow)"
seed_ledger "$d"
cat > "$d/.github/workflows/extra.yml" <<'YAML'
name: Extra
on:
  push:
    branches: [main]
jobs:
  delta:
    runs-on: ubuntu-latest
    steps:
      - run: echo delta
YAML
if run_gate "$d"; then
  fail "a workflow file absent from the ledger was accepted"
elif grep -q "extra.yml is a workflow on disk with no entry" "$WORK/out.log"; then
  pass "a new workflow file with no ledger entry fails"
else
  fail "the gate failed but not because of the unrecorded workflow"
  cat "$WORK/out.log" >&2
fi

# 8. A record whose sha is not in this history at all — a run from a branch that
#    was never merged, or a rewritten one. `git log A..HEAD` on an unrelated sha
#    reports nothing, so freshness alone would read it as current.
d="$(new_case foreign-sha)"
seed_ledger "$d"
jq '(.workflows[0].jobs.alpha.headSha) = "0000000000000000000000000000000000000000"' \
  "$d/$LEDGER" > "$d/$LEDGER.tmp"
mv "$d/$LEDGER.tmp" "$d/$LEDGER"
if run_gate "$d"; then
  fail "a record naming a commit outside this history was accepted"
elif grep -q "is not an ancestor of HEAD" "$WORK/out.log"; then
  pass "a record naming a commit outside this history fails"
else
  fail "the gate failed but not because the sha was foreign"
  cat "$WORK/out.log" >&2
fi

echo ""
echo "=== fixtures: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
