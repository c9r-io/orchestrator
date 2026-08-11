#!/usr/bin/env bash
# FR-165: negative fixtures for the manual-gate freshness criterion and for the
# release edge that enforces it.
#
# Two things are under test and they fail in different ways:
#
#   1. The criterion. Before FR-165 a gate was fresh if its record was recent,
#      and `exitStatus`/`worktreeDirty` were printed but never read — so a gate
#      recorded as having failed reported `ok`. Every fixture below therefore
#      mutates a *record* rather than deleting one: deletion is the case the
#      original author had in mind, and it was already handled. The mutations
#      that matter are the ones that leave a plausible-looking record in place.
#
#   2. The release edge. A job that runs `--strict`, goes red, and is depended
#      on by nothing lets the release publish anyway. Asserting the job exists
#      is §4.4 shape 1; this parses release.yml and asserts `build` and
#      `gui-build` actually name it in `needs:`, and that the step does not
#      carry `continue-on-error`.
#
# Structure follows the FR-143 fixture-drift rule: a before-run and an after-run
# bracket the mutations, so a truncated run cannot read as a complete one, and
# every expectation is derived from the fixture ledger rather than restated.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/manual-gate-freshness.rb"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr165-freshness.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# Added while closing FR-165 requirement 2, which found this file failing the
# ci-required scripts/qa/fixture-target-drift.rb — three ledger rewrites that
# proved nothing about whether they landed, and one block whose premise aborted.
# Requirement 1's certification ran a hand-listed sweep and did not include the
# drift scanner, so a gate this file breaks was red from the moment it shipped:
# §4.6 condition 6 exactly, inside the FR that wrote condition 6's own fixtures.
# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

# A scratch tree the real script can run against unmodified: it derives repo_root
# as ../.. from its own location, so scripts/qa/ plus config/governance/ is the
# whole world it needs.
build_tree() {
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/scripts/qa" "$root/config/governance"
  cp "$REPO_ROOT/$GATE" "$root/$GATE"
}

# Two gates, one fresh and one whose state each case sets. Dates are computed
# relative to today so the fixture does not rot the way a literal date would.
write_fixture() {
  local root="$1" subject_json="$2"
  ruby -rjson -rdate -e '
    root, subject = ARGV
    fresh = {
      "date" => Date.today.strftime("%Y-%m-%d"),
      "revision" => "a" * 40, "exitStatus" => 0, "worktreeDirty" => false
    }
    gates = {
      "scripts/qa/gate-control.sh" => { "owner" => "docs/qa/control.md", "lastRun" => fresh },
      "scripts/qa/gate-subject.sh" => JSON.parse(subject)
    }
    File.write(File.join(root, "config/governance/manual-gate-freshness.json"),
               JSON.pretty_generate({ "version" => 1, "staleAfterDays" => 90, "gates" => gates }) + "\n")
    File.write(File.join(root, "config/governance/qa-gate-surface.json"),
               JSON.pretty_generate({ "scripts" => gates.keys.map { |p|
                 { "path" => p, "enforcement" => "manual-runbook" } } }) + "\n")
  ' "$root" "$subject_json"
}

# `set -e` is disabled inside a condition, so the status is captured explicitly.
run_gate() {
  local root="$1"; shift
  local out status
  out="$(cd "$root" && ruby "$GATE" "$@" 2>&1)" && status=0 || status=$?
  LAST_OUTPUT="$out"
  return "$status"
}

# A helper for "this mutation must make --strict fail", which also insists the
# diagnostic names the gate that was mutated. An exit code alone cannot tell the
# branch a gate failed through from any other branch (§4.4 shape 7), and these
# four cases all exit 1.
expect_strict_red() {
  local root="$1" label="$2" needle="$3"
  if run_gate "$root" --strict; then
    fail "$label: --strict passed"
    return
  fi
  if grep -q "$needle" <<< "$LAST_OUTPUT"; then
    pass "$label"
  else
    fail "$label: --strict failed but the diagnostic never mentions '$needle'"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
}

TREE="$WORK/tree"
TODAY="$(date -u +%Y-%m-%d)"
FORTY_A="$(printf 'a%.0s' {1..40})"

fresh_record() {
  printf '{"owner":"docs/qa/subject.md","lastRun":{"date":"%s","revision":"%s","exitStatus":0,"worktreeDirty":false}}' \
    "$TODAY" "$FORTY_A"
}

# ── before-run ────────────────────────────────────────────────────────────────
# The premise every mutation below depends on: with both gates fresh, --strict is
# green. If this is red the run is meaningless, and a fixture that aborts here
# would print no summary line and read exactly like a complete pass.
build_tree "$TREE"
write_fixture "$TREE" "$(fresh_record)"
if run_gate "$TREE" --strict; then
  pass "before-run: two fresh gates, --strict is green"
else
  fail "before-run: --strict is already red before any mutation — every case below is void"
  echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
fi

# ── 1. a recorded failure is not a run ────────────────────────────────────────
# The mutation is exitStatus 0 -> 1 with the date left at today. This is the
# live defect FR-165 was filed on: test-attention-inbox.sh carried exit 1, dated
# the day before, and printed `ok`.
build_tree "$TREE"
write_fixture "$TREE" "$(printf '{"owner":"docs/qa/subject.md","lastRun":{"date":"%s","revision":"%s","exitStatus":1,"worktreeDirty":false}}' "$TODAY" "$FORTY_A")"
expect_strict_red "$TREE" "a same-day record with exitStatus 1 is not fresh" "gate-subject.sh"

# ── 2. a dirty-worktree run is not a run ──────────────────────────────────────
# exitStatus stays 0 and only worktreeDirty flips, so nothing but the field under
# test distinguishes this from the before-run.
build_tree "$TREE"
write_fixture "$TREE" "$(printf '{"owner":"docs/qa/subject.md","lastRun":{"date":"%s","revision":"%s","exitStatus":0,"worktreeDirty":true}}' "$TODAY" "$FORTY_A")"
expect_strict_red "$TREE" "a same-day record made on a dirty worktree is not fresh" "gate-subject.sh"

# ── 3. recency still counts ───────────────────────────────────────────────────
# The pre-existing rule must survive the new ones.
build_tree "$TREE"
AGED="$(ruby -rdate -e 'puts (Date.today - 120).strftime("%Y-%m-%d")')"
write_fixture "$TREE" "$(printf '{"owner":"docs/qa/subject.md","lastRun":{"date":"%s","revision":"%s","exitStatus":0,"worktreeDirty":false}}' "$AGED" "$FORTY_A")"
expect_strict_red "$TREE" "a successful clean run older than staleAfterDays is not fresh" "gate-subject.sh"

# ── 4. never recorded ─────────────────────────────────────────────────────────
build_tree "$TREE"
write_fixture "$TREE" '{"owner":"docs/qa/subject.md","lastRun":null}'
expect_strict_red "$TREE" "a gate that has never been recorded is not fresh" "gate-subject.sh"

# ── 5. the exemption works, and only for the gate that carries it ─────────────
build_tree "$TREE"
write_fixture "$TREE" '{"owner":"docs/qa/subject.md","lastRun":null,"releaseBlocking":false,"releaseBlockingReason":"unbounded loop; cannot precede a release"}'
if run_gate "$TREE" --strict; then
  if grep -q "do not block a release" <<< "$LAST_OUTPUT" &&
     grep -q "unbounded loop" <<< "$LAST_OUTPUT"; then
    pass "an exempt gate does not block a release, and the exemption is printed with its reason"
  else
    pass "an exempt gate does not block a release"
    fail "the exemption is not visible in the report — an unprinted exemption is an invisible one"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
else
  fail "an exempt never-run gate still blocked the release"
  echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
fi

# ── 6. the exemption does not leak to its neighbour ───────────────────────────
# The control gate is the one made stale here while the *subject* holds the
# exemption. If exemption were applied set-wide rather than per gate this passes
# and nothing else would have caught it.
build_tree "$TREE"
write_fixture "$TREE" '{"owner":"docs/qa/subject.md","lastRun":null,"releaseBlocking":false,"releaseBlockingReason":"exempt"}'
fixture_mutate "control gate made never-run" "$TREE/config/governance/manual-gate-freshness.json" \
  ruby -rjson -e '
    path = File.join(ARGV[0], "config/governance/manual-gate-freshness.json")
    data = JSON.parse(File.read(path))
    gate = data["gates"]["scripts/qa/gate-control.sh"]
    raise "the control gate is not in the fixture ledger" if gate.nil?
    gate["lastRun"] = nil
    File.write(path, JSON.pretty_generate(data) + "\n")
  ' "$TREE"
expect_strict_red "$TREE" "one gate's exemption does not excuse another gate" "gate-control.sh"

# ── 7. an exemption without a reason is an error ──────────────────────────────
build_tree "$TREE"
write_fixture "$TREE" '{"owner":"docs/qa/subject.md","lastRun":null,"releaseBlocking":false}'
if run_gate "$TREE"; then
  fail "releaseBlocking: false with no reason was accepted"
else
  if grep -q "no releaseBlockingReason" <<< "$LAST_OUTPUT"; then
    pass "an exemption without a reason fails even without --strict"
  else
    fail "rejected, but not for the missing reason"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
fi

# ── 8. a reason left behind after the exemption is removed is an error ────────
# The inverted form of case 7. §4.4 shape 9: before concluding a shape is safe in
# one direction, negate it and ask whether the same defect still produces the
# same colour. A stale reason reads to the next person as an exemption that is
# no longer in force.
build_tree "$TREE"
write_fixture "$TREE" "$(printf '{"owner":"docs/qa/subject.md","releaseBlockingReason":"stale, the exemption was removed","lastRun":{"date":"%s","revision":"%s","exitStatus":0,"worktreeDirty":false}}' "$TODAY" "$FORTY_A")"
if run_gate "$TREE"; then
  fail "an orphaned releaseBlockingReason was accepted"
else
  if grep -q "carries releaseBlockingReason but is release-blocking" <<< "$LAST_OUTPUT"; then
    pass "a reason left behind without its exemption is an error"
  else
    fail "rejected, but not for the orphaned reason"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
fi

# ── 9. the empty-read diagnostic derives its number ───────────────────────────
# The regression this replaces: the diagnostic said "35 are expected" while the
# manifest declared 38. The fixture ledger has two gates, so a derived message
# says two and a restated one says something else. This is the only case whose
# expectation is a number, and it is read out of the fixture rather than typed.
build_tree "$TREE"
write_fixture "$TREE" "$(fresh_record)"
fixture_mutate "manual-runbook set emptied" "$TREE/config/governance/qa-gate-surface.json" \
  ruby -rjson -e '
    path = File.join(ARGV[0], "config/governance/qa-gate-surface.json")
    File.write(path, JSON.pretty_generate({ "scripts" => [] }) + "\n")
  ' "$TREE"
EXPECTED_N="$(ruby -rjson -e 'puts JSON.parse(File.read(File.join(ARGV[0], "config/governance/manual-gate-freshness.json")))["gates"].length' "$TREE")"
if run_gate "$TREE"; then
  fail "an empty manual-runbook set was accepted as a clean result"
else
  if grep -q "the freshness ledger records $EXPECTED_N" <<< "$LAST_OUTPUT"; then
    pass "the empty-read diagnostic derives its count from the ledger ($EXPECTED_N)"
  else
    fail "the empty-read diagnostic does not name the derived count $EXPECTED_N"
    echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
  fi
fi

# ── 10. the release edge is real ──────────────────────────────────────────────
# Parsed, not grepped. A `needs:` inside a comment, or the job's name appearing
# in a `name:` field, satisfies a grep and blocks nothing.
if [[ ! -f "$WORKFLOW" ]]; then
  fail "release.yml not found at $WORKFLOW"
else
  # Every objection is collected and printed rather than aborted on. An abort
  # here would have been caught by the shell below, but fixture-target-drift.rb
  # is right to refuse the shape on sight: the reader cannot know its caller
  # checks, and the same block copied into a context without the `|| status=$?`
  # would take the run down before the summary line printed. Reporting all
  # objections at once is also strictly more useful than reporting the first.
  edge_report="$(ruby -ryaml -e '
    y = YAML.load_file(ARGV[0])
    jobs = y["jobs"] || {}
    problems = []
    dependents = []

    job = jobs["manual-gate-freshness"]
    if job.nil?
      problems << "no manual-gate-freshness job"
    else
      steps = job["steps"] || []
      strict = steps.find { |s| s["run"].to_s.include?("manual-gate-freshness.rb") }
      if strict.nil?
        problems << "no step runs manual-gate-freshness.rb"
      else
        run = strict["run"].to_s
        problems << "the strict step is missing --strict" unless run.include?("--strict")
        problems << "the strict step carries continue-on-error" if strict["continue-on-error"]
        problems << "the strict step pipes its output, which reports the pager status" if run.include?("|")
      end

      dependents = jobs.select { |_, spec| Array(spec["needs"]).include?("manual-gate-freshness") }.keys
      if dependents.empty?
        problems << "no job needs manual-gate-freshness"
      else
        %w[build gui-build].each do |required|
          problems << "#{required} does not need manual-gate-freshness" unless dependents.include?(required)
        end
      end
    end

    if problems.empty?
      puts "OK " + dependents.sort.join(",")
    else
      puts "BROKEN " + problems.join("; ")
    end
  ' "$WORKFLOW" 2>&1)"

  # The marker is asserted, not the exit code: the reader now exits 0 whatever it
  # finds, so an exit code would say only that ruby ran.
  case "$edge_report" in
  "OK "*)
    pass "release.yml: the strict job exists, does not swallow its status, and gates ${edge_report#OK }"
    ;;
  "BROKEN "*)
    fail "release.yml enforcement edge: ${edge_report#BROKEN }"
    ;;
  *)
    fail "release.yml enforcement edge: the reader produced neither verdict: $edge_report"
    ;;
  esac
fi

# ── after-run ─────────────────────────────────────────────────────────────────
# The same premise as the before-run, rebuilt from scratch. Together they bracket
# the mutations: if this is red while the before-run was green, a case leaked
# state and the greens above cannot be trusted.
build_tree "$TREE"
write_fixture "$TREE" "$(fresh_record)"
if run_gate "$TREE" --strict; then
  pass "after-run: the fixture is green again, so no case leaked state"
else
  fail "after-run: --strict is red on a clean fixture"
  echo "$LAST_OUTPUT" | sed 's/^/      /' >&2
fi

echo ""
echo "FR-165 manual-gate freshness fixtures: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
