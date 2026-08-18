#!/usr/bin/env bash
#
# FR-174: fixtures for the meta-verification tier and the aggregator that
# enforces it.
#
# The defect this guards against is the one FR-174 creates. Before tiering, no
# gate in the governance job carried an `if:`, so `skipped` could only mean a
# cancelled job and the aggregator accepted it for free. Nineteen conditional
# gates turn that tolerance into §4.4 shape 5: a predicate that wrongly returns
# `deferred` skips all nineteen, the aggregator prints nineteen untroubled lines,
# the job is green, and no meta-verification has run anywhere. The saving would
# be real and the checking would be gone, and nothing in the log would say so.
#
# Three things follow, and they shape every case below.
#
# First, **the judge is the real script**. Cases drive `ci-tier.sh` and
# `governance-result.sh` themselves against throwaway repositories and synthetic
# outcome tables. A fixture that re-implemented the predicate would prove the
# fixture works. This is why both were lifted out of ci.yml at all: a `run:`
# block can only be checked by reading it, and reading is what §4.4 calls a
# proxy.
#
# Second, **the mutation that matters is `deferred` + a gate that ran**, not a
# gate that failed. A failure is the case the author has in mind and every
# arrangement catches it. A meta gate reporting `success` under `deferred` means
# the condition did not do what the run says it did — the tier is a claim about
# what executed, and an unexpected success falsifies it exactly as an unexpected
# failure does. Case 12 is that direction, and it is the one a tolerant
# aggregator waves through.
#
# Third, **the rosters are derived from the workflows, never restated here**.
# Four sets have to agree — the steps ci.yml gates, ci.yml's META list, the
# nightly's steps, and the nightly's META list — and a fixture holding a fifth
# copy would go stale the day a gate is added, which is §4.4 shape 2 aimed at
# the fixture's own target (shape 7). Case 16 reads all four out of the YAML and
# compares them.
#
# Safety: every case builds a throwaway git repository under $TMPDIR. The
# working tree is read, never written. No daemon starts, no database is touched,
# no provider is invoked, and nothing contacts the network.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TIER_SH="$REPO_ROOT/scripts/qa/ci-tier.sh"
RESULT_SH="$REPO_ROOT/scripts/qa/governance-result.sh"
CI_YML="$REPO_ROOT/.github/workflows/ci.yml"
NIGHTLY_YML="$REPO_ROOT/.github/workflows/nightly-governance.yml"

for required in git ruby; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "missing required command: $required" >&2
    exit 1
  }
done

PASS=0
FAIL=0
pass() {
  echo "  PASS: $1"
  PASS=$((PASS + 1))
}
fail() {
  echo "  FAIL: $1" >&2
  FAIL=$((FAIL + 1))
}

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr174-ci-tier.XXXXXX")"
# A truncated run reads exactly like a complete one to anything checking the
# exit code, so announce truncation rather than relying on the summary appearing
# (§4.4 shape 7, and the FR-170 helper that took `set -e` with it).
SUMMARY_REACHED=0
cleanup() {
  status=$?
  if [ "$SUMMARY_REACHED" -eq 0 ]; then
    echo "" >&2
    echo "FR-174 tier fixtures terminated before the summary line (exit $status)" >&2
    echo "the cases above are an incomplete run, not a passing one" >&2
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "=== FR-174: meta-verification tier and its aggregator ==="
echo ""

# ── The predicate ─────────────────────────────────────────────────────────────
#
# Each case gets a repository whose base is a real ref, because the thing under
# test is `git diff base...HEAD` and a mock would only prove the mock.

make_repo() { # $1 = dir, $2... = files to change on the branch
  dir="$1"
  shift
  mkdir -p "$dir"
  git -C "$dir" init --quiet
  git -C "$dir" config user.email fixture@example.invalid
  git -C "$dir" config user.name fixture
  mkdir -p "$dir/src"
  echo base > "$dir/src/base.rs"
  git -C "$dir" add -A
  git -C "$dir" commit --quiet -m base
  git -C "$dir" update-ref refs/remotes/origin/main HEAD
  for f in "$@"; do
    mkdir -p "$dir/$(dirname "$f")"
    echo changed >> "$dir/$f"
  done
  if [ "$#" -gt 0 ]; then
    git -C "$dir" add -A
    git -C "$dir" commit --quiet -m change
  fi
}

tier_of() { # $1 = repo dir; remaining env supplied by caller
  # `${BASE-main}` and not `${BASE:-main}`. With the colon an explicitly empty
  # BASE expands to `main`, so the "no base ref" case would have handed the
  # script a perfectly good ref and asserted nothing — the harness lying about
  # which state it built. Caught by that case failing; it would have passed
  # silently had the control repo's changeset touched a tiered root.
  (
    cd "$1" || exit 1
    GITHUB_EVENT_NAME="${EVENT-pull_request}" \
      GITHUB_BASE_REF="${BASE-main}" \
      GITHUB_OUTPUT="" \
      "$TIER_SH" 2>/dev/null
  )
}

# 0. Positive control. Without it every `full` below could be a script that
#    returns `full` unconditionally, and the whole suite would look healthy
#    while the tier never engaged.
make_repo "$WORK/control" "src/lib.rs"
if [ "$(tier_of "$WORK/control")" = "deferred" ]; then
  pass "control: a changeset touching no tiered root defers"
else
  fail "control: a product-only changeset did not defer — nothing below is meaningful"
fi

# 1-4. Each tiered root, separately. One case per root because a single case
#      over all four would pass while three of the patterns were wrong.
i=1
for root in "scripts/qa/test-thing.sh" "scripts/lib/thing.sh" \
  "config/governance/thing.json" ".github/workflows/thing.yml"; do
  make_repo "$WORK/root$i" "$root"
  if [ "$(tier_of "$WORK/root$i")" = "full" ]; then
    pass "a changeset touching $(dirname "$root")/ returns meta-verification to the PR path"
  else
    fail "$(dirname "$root")/ did not trigger the full tier"
  fi
  i=$((i + 1))
done

# 5. A product changeset that also touches a gate root. The tier is a claim
#    about the whole changeset, not about its largest part.
make_repo "$WORK/mixed" "src/lib.rs" "scripts/qa/test-thing.sh"
if [ "$(tier_of "$WORK/mixed")" = "full" ]; then
  pass "one tiered file among untiered ones is enough to return the full tier"
else
  fail "a mixed changeset deferred; the predicate is looking at the wrong subset"
fi

# 6-8. Fail closed. Each is a state where the predicate cannot answer, and the
#      answer it must give is the expensive one.
make_repo "$WORK/closed" "src/lib.rs"

if [ "$(EVENT=push tier_of "$WORK/closed")" = "full" ]; then
  pass "a push is full: there is no base ref to reason about"
else
  fail "a push did not fail closed"
fi

if [ "$(BASE="" tier_of "$WORK/closed")" = "full" ]; then
  pass "an empty base ref is full"
else
  fail "an empty base ref did not fail closed"
fi

if [ "$(BASE=does-not-exist tier_of "$WORK/closed")" = "full" ]; then
  pass "an unresolvable base ref is full"
else
  fail "an unresolvable base ref did not fail closed"
fi

# 9. An empty diff. This is the case a reasonable author gets wrong: "no files
#    changed" reads like "no gates changed" and is not — it is the question
#    going unanswered. A predicate that deferred here would defer every run
#    whose diff computation silently returned nothing.
make_repo "$WORK/empty"
if [ "$(tier_of "$WORK/empty")" = "full" ]; then
  pass "an empty diff is full, not 'no gate changed'"
else
  fail "an empty diff deferred; absence of data was read as evidence"
fi

# 10. The output contract the workflow depends on. `steps.tier.outputs.tier` is
#     what nineteen `if:` conditions read; if the script stops writing it every
#     one of them evaluates against an empty string and skips.
GITHUB_OUT="$WORK/gh-output"
: > "$GITHUB_OUT"
(
  cd "$WORK/control" || exit 1
  GITHUB_EVENT_NAME=pull_request GITHUB_BASE_REF=main GITHUB_OUTPUT="$GITHUB_OUT" \
    "$TIER_SH" >/dev/null 2>&1
)
if grep -q '^tier=deferred$' "$GITHUB_OUT"; then
  pass "the verdict is written to GITHUB_OUTPUT as tier=<t>"
else
  fail "GITHUB_OUTPUT did not receive tier=; every if: would evaluate empty"
fi

echo ""

# ── The aggregator ────────────────────────────────────────────────────────────

META_FIXTURE=$'a-fixtures\nb-fixtures'

judge() { # $1 = tier, $2 = outcomes, $3 = expected exit
  out="$(TIER="$1" META="$META_FIXTURE" OUTCOMES="$2" "$RESULT_SH" 2>&1)"
  rc=$?
  if [ "$rc" -eq "$3" ]; then
    return 0
  fi
  printf '%s\n' "$out" | sed 's/^/      /' >&2
  return 1
}

# 11. Control, both tiers. A rejecting-everything aggregator would satisfy every
#     negative case below.
if judge full $'a-fixtures=success\nb-fixtures=success\nreal=success' 0 \
  && judge deferred $'a-fixtures=skipped\nb-fixtures=skipped\nreal=success' 0; then
  pass "control: a consistent full run and a consistent deferred run both pass"
else
  fail "control: a consistent run was rejected"
fi

# 12. **The mutation the implementation is least likely to catch.** Not a
#     failure — a meta gate that succeeded when the tier said it would not run.
#     The old aggregator accepted this without noticing, because it only ever
#     asked whether an outcome was bad.
if judge deferred $'a-fixtures=success\nb-fixtures=skipped\nreal=success' 1; then
  pass "a meta gate that RAN under the deferred tier fails the job"
else
  fail "a meta gate ran under deferred and the job passed — the tier claim is unenforced"
fi

# 13. The inverse, and the one that would make this FR a coverage cut: the tier
#     says full, so every meta gate must have run.
if judge full $'a-fixtures=skipped\nb-fixtures=success\nreal=success' 1; then
  pass "a meta gate skipped under the full tier fails the job"
else
  fail "a skipped meta gate passed under the full tier"
fi

# 14. The whole-fleet case. This is the shape of the actual disaster: a broken
#     predicate defers on a PR that touches gates, and every meta gate skips.
if judge full $'a-fixtures=skipped\nb-fixtures=skipped\nreal=success' 1; then
  pass "every meta gate skipping under the full tier fails the job"
else
  fail "a run with no meta-verification at all reported success"
fi

# 15. The pre-existing contract, unchanged: a real gate failing still fails, and
#     a real gate skipping is not tolerated now that skips carry meaning.
if judge deferred $'a-fixtures=skipped\nb-fixtures=skipped\nreal=failure' 1 \
  && judge full $'a-fixtures=success\nb-fixtures=success\nreal=skipped' 1; then
  pass "a non-meta gate that failed or was skipped still fails the job"
else
  fail "a non-meta gate failure or skip was tolerated"
fi

# 16. Reading nothing is not passing (§4.4 shape 5, stated directly), and a tier
#     the script does not recognise is a failure rather than a default.
if judge full '' 1 \
  && judge '' $'a-fixtures=success\nb-fixtures=success\nreal=success' 1 \
  && judge nonsense $'a-fixtures=success\nb-fixtures=success\nreal=success' 1; then
  pass "empty outcomes, an unset tier and an unrecognised tier each fail"
else
  fail "the aggregator passed on absent or unusable input"
fi

# 17. Roster drift, both directions. A META entry naming no gate drops that gate
#     out of the tier assertion; the reverse makes a conditional gate be judged
#     as mandatory. Neither produces a wrong-looking line on its own.
if judge full $'a-fixtures=success\nreal=success' 1; then
  pass "a META entry naming no gate in OUTCOMES fails"
else
  fail "META named a gate that does not exist and the job passed"
fi

# 18. Membership is whole-line. `cost-fixtures` must not match
#     `cost-fixtures-extra`; a substring test would silently reclassify.
out="$(TIER=deferred META='cost-fixtures' \
  OUTCOMES=$'cost-fixtures=skipped\ncost-fixtures-extra=success' "$RESULT_SH" 2>&1)"
rc=$?
if [ "$rc" -eq 0 ] && grep -q 'cost-fixtures-extra *success$' <<< "$out"; then
  pass "roster membership matches whole ids, not prefixes"
else
  fail "a prefix of a META id was treated as a member"
fi

echo ""

# ── The wiring the two files cannot see about each other ──────────────────────

# 19. Four sets, derived from the YAML, compared rather than restated. A gate
#     deferred by ci.yml and absent from the nightly runs nowhere at all, and
#     each file is individually consistent while that is true.
ROSTER_REPORT="$(ruby -ryaml -e '
  ci = YAML.load_file(ARGV[0])["jobs"]["governance"]["steps"]
  ng = YAML.load_file(ARGV[1])["jobs"]["meta-verification"]["steps"]

  gated = ci.select { |s| s["if"].to_s.include?("tier.outputs.tier") }
  ci_ids = gated.map { |s| s["id"] }.compact.sort

  agg = ci.find { |s| s["name"] == "Governance result" }
  ci_meta = agg["env"]["META"].to_s.split("\n").map(&:strip).reject(&:empty?).sort

  ng_steps = ng.select { |s| s["id"] }
  ng_ids = ng_steps.map { |s| s["id"] }.sort
  ng_agg = ng.find { |s| s["name"] == "Nightly governance result" }
  ng_meta = ng_agg["env"]["META"].to_s.split("\n").map(&:strip).reject(&:empty?).sort

  problems = []
  problems << "ci gated steps != ci META: #{(ci_ids - ci_meta) | (ci_meta - ci_ids)}" if ci_ids != ci_meta
  problems << "ci gated steps != nightly steps: #{(ci_ids - ng_ids) | (ng_ids - ci_ids)}" if ci_ids != ng_ids
  problems << "nightly steps != nightly META: #{(ng_ids - ng_meta) | (ng_meta - ng_ids)}" if ng_ids != ng_meta

  cmd_ci = gated.to_h { |s| [s["id"], s["run"].to_s.strip] }
  cmd_ng = ng_steps.to_h { |s| [s["id"], s["run"].to_s.strip] }
  cmd_ci.each do |id, cmd|
    problems << "#{id} runs a different command in the nightly" if cmd_ng[id] != cmd
  end

  problems << "no gate carries the tier condition" if ci_ids.empty?

  if problems.empty?
    puts "OK #{ci_ids.size}"
  else
    problems.each { |p| puts "PROBLEM #{p}" }
  end
' "$CI_YML" "$NIGHTLY_YML" 2>&1)"

case "$ROSTER_REPORT" in
  OK\ *)
    pass "the gated set, both META rosters and the nightly's steps agree (${ROSTER_REPORT#OK } gates, commands identical)"
    ;;
  *)
    fail "roster drift between ci.yml and nightly-governance.yml"
    printf '%s\n' "$ROSTER_REPORT" | sed 's/^/      /' >&2
    ;;
esac

# 20. The mechanism may not defer itself. A tier step, an aggregator or this
#     gate carrying the tier condition would be a gate that can switch off its
#     own verification — the deadlock this FR must not build.
SELF_REPORT="$(ruby -ryaml -e '
  ci = YAML.load_file(ARGV[0])["jobs"]["governance"]["steps"]
  offenders = ci.select do |s|
    next false unless s["if"].to_s.include?("tier.outputs.tier")
    run = s["run"].to_s
    s["id"] == "tier" || s["name"] == "Governance result" ||
      run.include?("ci-tier.sh") || run.include?("governance-result.sh") ||
      run.include?("test-ci-tier.sh")
  end
  if offenders.empty?
    puts "OK"
  else
    offenders.each { |s| puts "PROBLEM #{s["name"]} is tier-conditional" }
  end
' "$CI_YML" 2>&1)"

if [ "$SELF_REPORT" = "OK" ]; then
  pass "the tier step, the aggregator and this gate are never themselves deferred"
else
  fail "part of the tiering mechanism can defer itself"
  printf '%s\n' "$SELF_REPORT" | sed 's/^/      /' >&2
fi

# 21. The manifest's claim about itself. `ci-required` means "runs on every
#     push/PR", and for these nineteen that is no longer true — the entries
#     carry `tieredBy` to say so. An entry that lost the marker would read as
#     unconditional, and the enforcement-surface report would overstate what a
#     given PR actually ran. Derived from ci.yml on both sides, so adding a
#     twentieth gate cannot leave the manifest describing nineteen.
MANIFEST_REPORT="$(ruby -ryaml -rjson -e '
  ci = YAML.load_file(ARGV[0])["jobs"]["governance"]["steps"]
  gated = ci.select { |s| s["if"].to_s.include?("tier.outputs.tier") }
             .map { |s| s["run"].to_s.strip[%r{scripts/\S+?\.(?:sh|rb)}] }
             .compact.sort.uniq
  surface = JSON.parse(File.read(ARGV[1]))["scripts"]
  marked = surface.select { |s| s["tieredBy"] }.map { |s| s["path"] }.sort

  problems = []
  problems << "gated in ci.yml but not marked tieredBy: #{(gated - marked).inspect}" unless (gated - marked).empty?
  problems << "marked tieredBy but not gated in ci.yml: #{(marked - gated).inspect}" unless (marked - gated).empty?
  surface.select { |s| s["tieredBy"] }.each do |s|
    problems << "#{s["path"]} tieredBy names #{s["tieredBy"]}, which is not the tier script" unless s["tieredBy"] == "scripts/qa/ci-tier.sh"
  end
  problems << "no entry carries tieredBy" if marked.empty?
  puts(problems.empty? ? "OK #{marked.size}" : problems.map { |p| "PROBLEM #{p}" }.join("\n"))
' "$CI_YML" "$REPO_ROOT/config/governance/qa-gate-surface.json" 2>&1)"

case "$MANIFEST_REPORT" in
  OK\ *)
    pass "the gate manifest marks exactly the ${MANIFEST_REPORT#OK } gates ci.yml tiers"
    ;;
  *)
    fail "the gate manifest and ci.yml disagree about which gates are tiered"
    printf '%s\n' "$MANIFEST_REPORT" | sed 's/^/      /' >&2
    ;;
esac

# 22. The three names that have to be the same name. The `if:` conditions read
#     `steps.<step-id>.outputs.<key>`; the step that produces it has an `id:`;
#     and ci-tier.sh writes a `<key>=` line into GITHUB_OUTPUT. Nothing else
#     compares them, and each pair can drift silently: rename the step's id and
#     every condition resolves against a step that does not exist, rename the
#     output key and they resolve against a key nobody writes. Both yield an
#     empty string, both skip all nineteen gates, and the aggregator's
#     unset-tier check is the only thing between that and a green job — a
#     backstop, not a diagnosis. Derived from all three files.
NAMES_REPORT="$(ruby -ryaml -e '
  ci = YAML.load_file(ARGV[0])["jobs"]["governance"]["steps"]
  producer = ci.find { |s| s["run"].to_s.include?("ci-tier.sh") }
  problems = []
  if producer.nil?
    problems << "no step in the governance job runs ci-tier.sh"
  else
    step_id = producer["id"].to_s
    problems << "the step running ci-tier.sh has no id" if step_id.empty?

    refs = ci.map { |s| s["if"].to_s }.grep(/tier\.outputs\./).uniq
    problems << "no step reads the tier output" if refs.empty?
    refs.each do |expr|
      m = expr.match(/steps\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)/)
      if m.nil?
        problems << "cannot read a steps.<id>.outputs.<key> reference out of #{expr.inspect}"
        next
      end
      problems << "#{expr.inspect} names step #{m[1].inspect}, but ci-tier.sh runs in #{step_id.inspect}" if m[1] != step_id
      key = m[2]
      written = File.read(ARGV[1]).include?("printf %s\x27#{key}=" % "")
      problems << "#{expr.inspect} reads output #{key.inspect}, which ci-tier.sh never writes" unless written
    end
  end
  puts(problems.empty? ? "OK" : problems.map { |p| "PROBLEM #{p}" }.join("\n"))
' "$CI_YML" "$REPO_ROOT/scripts/qa/ci-tier.sh" 2>&1)"

if [ "$NAMES_REPORT" = "OK" ]; then
  pass "the tier step's id, the if: references and the output ci-tier.sh writes are one name"
else
  fail "the tier output is read under a name nothing produces"
  printf '%s\n' "$NAMES_REPORT" | sed 's/^/      /' >&2
fi

echo ""
SUMMARY_REACHED=1
echo "FR-174 meta-verification tier: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
