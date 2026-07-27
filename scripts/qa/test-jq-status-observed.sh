#!/usr/bin/env bash
#
# FR-144: fixtures for the JSON reader and the scanner that keeps it in place.
#
# The defect these guard against is a gate that stops checking while reporting
# PASS. It is not hypothetical and it was not found by an audit: FR-140 wrote
# `"providerIsolation": "no-provider"` where the schema requires
# `{"mode": "no-provider"}`, jq exited 5, the loop being fed by
# `done < <(jq …)` read zero rows, and test-qa-gate-surface.sh printed
# "13 passed, 0 failed" over a manifest it could not parse.
#
# Two things follow, and they shape every case below.
#
# First, the judge has to be the real gate. The evidence for this defect is
# precisely that the gate and its own negative fixtures disagreed: the fixture
# suite failed six cases while the gate reported success. A fixture that only
# exercised the fixture harness would reproduce the original mistake, so case 2
# runs test-qa-gate-surface.sh itself against a malformed manifest and requires
# it to fail.
#
# Second, the mutation is a *type error*, not a deleted entry. Deletion is the
# case the author has in mind, and it does not make jq exit non-zero — it yields
# fewer rows, which every check here already handles. The type error is the
# mutation the implementation is least likely to catch, and it is the one that
# actually happened.
#
# Case 4 exists because case 3 alone would be satisfied by a reader that
# rejects everything. Cases 7 and 8 exist because a scanner that flags its own
# documentation is a grep wearing a lexer's name — and this repository's design
# records quote the forbidden pattern by necessity.
#
# Safety: every case builds a throwaway tree under $TMPDIR. The working tree is
# read, never written. No daemon starts, no database is touched, no provider is
# invoked, and nothing contacts the network.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCANNER="scripts/qa/jq-status-observed.rb"
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

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr144-jq-status.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# shellcheck source=../lib/gate_jq.sh
. "$REPO_ROOT/scripts/lib/gate_jq.sh"

echo "=== FR-144: JSON reads observe their exit status ==="
echo ""

# ── The reader ────────────────────────────────────────────────────────────────

FIXTURE_JSON="$WORK/policy.json"
cat > "$FIXTURE_JSON" <<'JSON'
{
  "typed": "a string where an object belongs",
  "population": [1, 2, 3],
  "exemptions": []
}
JSON

# 0. Positive control. Without it every case below could be passing because the
#    reader is broken rather than because the defect is detected.
if rows="$(gate_jq_rows require-rows "$FIXTURE_JSON" '.population[]')" \
   && [[ "$(printf '%s' "$rows" | grep -c .)" -eq 3 ]]; then
  pass "control: a well-formed read returns its rows and succeeds"
else
  fail "control: a well-formed read did not return its rows"
fi

# 1. The recorded defect, at the reader. jq exits 5 on `.typed.mode`; the read
#    must fail and must say both which file and what jq said. Exit code alone
#    would be a proxy — a crashed interpreter produces one too.
out="$(gate_jq_rows require-rows "$FIXTURE_JSON" '.typed.mode' 2>&1)" && status=0 || status=$?
if [[ "$status" -ne 0 ]] \
   && grep -qF "$FIXTURE_JSON" <<< "$out" \
   && grep -qF "Cannot index string with string" <<< "$out"; then
  pass "a type error makes the read fail, naming the file and quoting jq's diagnostic"
else
  fail "a type error did not produce a named, diagnosed failure (status=$status)"
  printf '    %s\n' "$out" >&2
fi

# 2. require-rows on an empty result fails.
if gate_jq_rows require-rows "$FIXTURE_JSON" '.exemptions[]' >/dev/null 2>&1; then
  fail "require-rows accepted an empty result"
else
  pass "a query declared to require rows fails when it reads none"
fi

# 3. allow-empty on an empty result succeeds, and yields zero lines rather than
#    one blank one. Without this case, case 2 is satisfied by a reader that
#    rejects everything, and the legitimately-empty exemption lists in this
#    repository would have to be kept non-empty to keep the gates quiet.
if rows="$(gate_jq_rows allow-empty "$FIXTURE_JSON" '.exemptions[]')" \
   && [[ "$(printf '%s' "$rows" | grep -c .)" -eq 0 ]]; then
  pass "a query declared to allow an empty result passes on one, and iterates zero times"
else
  fail "allow-empty rejected an empty result, or emitted a blank row"
fi

# 4. Emptiness has no default. A default is a way to forget, and forgetting is
#    the defect: zero rows is legitimate for staleClaimExemptions and impossible
#    for ci-required, and only the caller knows which it meant.
if gate_jq_rows "$FIXTURE_JSON" '.population[]' >/dev/null 2>&1; then
  fail "an undeclared emptiness was accepted"
else
  pass "a read that does not declare what an empty result means is an error"
fi

# 5. The failure record survives a subshell. This is the mechanism that catches
#    reads the call sites cannot: test-docs-publishing-integrity.sh reads its
#    policy four loops deep inside nested process substitutions, where a
#    non-zero return has nowhere to go.
gate_jq_begin
# Deliberately the old shape, in a subshell, exactly as the unconverted gates
# have it — the return value is discarded and the parent sees nothing.
while read -r _; do :; done < <(gate_jq_rows require-rows "$FIXTURE_JSON" '.typed.mode' 2>/dev/null)
if [[ "$(gate_jq_failure_count)" -ge 1 ]]; then
  pass "a read that fails inside a process substitution still leaves a record the parent can find"
else
  fail "a failed read inside a process substitution left no record"
fi
gate_jq_end

# ── The real gate, end to end ─────────────────────────────────────────────────

# 6. The assertion that would have caught the original. A working copy of the
#    repository with one manifest entry retyped from object to string: the gate
#    must fail and must name the manifest and jq's complaint.
#
#    Not run through the FR-127 expect_fail harness, which requires a fixture to
#    isolate to exactly one check. It cannot here, and pretending otherwise
#    would be a false claim: two checks read providerIsolation, so the type
#    error trips check_provider_isolation and check_provider_stub_coverage
#    both. The honest assertion is that the gate as a whole rejects the tree.
CASE6="$WORK/case6"
git clone -q "$REPO_ROOT" "$CASE6" 2>/dev/null
ruby -rjson -e '
  path = ARGV[0]
  doc = JSON.parse(File.read(path))
  entry = doc["scripts"].find { |s| s["providerIsolation"].is_a?(Hash) }
  entry["providerIsolation"] = "no-provider"
  File.write(path, JSON.pretty_generate(doc) + "\n")
' "$CASE6/$SURFACE"
# The clone is of HEAD; the gate and its library must be the working-tree ones,
# or this case certifies the last commit rather than the change under test.
cp "$REPO_ROOT/scripts/qa/test-qa-gate-surface.sh" "$CASE6/scripts/qa/"
cp "$REPO_ROOT/scripts/lib/gate_jq.sh" "$CASE6/scripts/lib/"
(cd "$CASE6" && bash scripts/qa/test-qa-gate-surface.sh) > "$WORK/case6.log" 2>&1 && gate_status=0 || gate_status=$?
if [[ "$gate_status" -ne 0 ]] \
   && grep -q "jq exited" "$WORK/case6.log" \
   && grep -q "Cannot index string with string" "$WORK/case6.log"; then
  pass "the real gate-surface gate rejects a manifest jq cannot parse, and says why"
else
  fail "the real gate accepted a manifest jq cannot parse (exit $gate_status)"
  tail -5 "$WORK/case6.log" >&2
fi

# ── The scanner ───────────────────────────────────────────────────────────────

# A scratch checkout the scanner can be pointed at. Cases below append to one
# in-scope gate and re-run; each starts from the pristine copy.
CASE7="$WORK/case7"
git clone -q "$REPO_ROOT" "$CASE7" 2>/dev/null
cp "$REPO_ROOT/$SCANNER" "$CASE7/scripts/qa/"
cp "$REPO_ROOT/$SURFACE" "$CASE7/$SURFACE"
TARGET="$CASE7/scripts/qa/test-doc-lifecycle.sh"
cp "$TARGET" "$WORK/target.pristine"

# Captured rather than piped into grep. `set -o pipefail` is on, and the scanner
# exits non-zero exactly when it finds something, so `scan | grep -q` returns
# failure on a successful match — the pipeline's status is the scanner's. That
# is the same class of confusion this whole FR is about, met here in the test
# for it.
scan() { (cd "$CASE7" && ruby "scripts/qa/jq-status-observed.rb" 2>&1) || true; }
restore() { cp "$WORK/target.pristine" "$TARGET"; }

# 7. Control: the tree as committed is clean. A scanner that flags nothing
#    because it scans nothing would pass every case below.
if scan | grep -q "^jq status observed: PASS"; then
  pass "control: the repository as it stands has no unobserved read"
else
  fail "control: the repository does not pass its own scanner"
  scan | head -5 >&2
fi

# 8. The forbidden shape, reintroduced.
restore
printf '\nwhile read -r fixture_row; do :; done < <(jq -r ".a" fixture.json)\n' >> "$TARGET"
if scan | grep -q "\[unobserved-feed\] a loop is fed by jq"; then
  pass "a reintroduced process-substitution jq feed is rejected, naming the file and line"
else
  fail "a reintroduced process-substitution jq feed was not detected"
fi

# 9. The same text, commented out. This is the mutation the implementation is
#    least likely to survive: a grep passes case 8 and fails this one, and the
#    design records for this very FR quote the pattern in prose.
restore
printf '\n# while read -r x; do :; done < <(jq -r ".a" fixture.json)\n' >> "$TARGET"
if scan | grep -q "^jq status observed: PASS"; then
  pass "the same line inside a comment is not a finding"
else
  fail "the scanner flagged a commented-out occurrence, so it is a grep, not a parse"
fi

# 10. The same text inside a here-document, which is data to the enclosing
#     script. FR-138 exists because a here-document lookalike silently ended a
#     scan; this asserts the lexer is actually being used here.
restore
cat >> "$TARGET" <<'OUTER'

cat > /dev/null <<'INNER'
while read -r x; do :; done < <(jq -r ".a" fixture.json)
INNER
OUTER
if scan | grep -q "^jq status observed: PASS"; then
  pass "the same line inside a here-document body is not a finding"
else
  fail "the scanner read a here-document body as code"
fi

# 11. jq piped inside a command substitution. The substitution reports the last
#     stage's status, so this is unobservable however carefully the caller tests
#     it — which is exactly what check_surface_complete did.
restore
printf '\ndeclared_paths="$(jq -r ".a[]" fixture.json | LC_ALL=C sort)"\n' >> "$TARGET"
if scan | grep -q "\[status-dropped-by-pipe\]"; then
  pass "jq piped inside a command substitution is rejected"
else
  fail "jq piped inside a command substitution was not detected"
fi

# 12. Coverage is derived, not listed. A gate registered as ci-required today is
#     scanned today; the scanned set must follow the manifest rather than a
#     roster somebody has to remember to grow.
restore
scanned_before="$( (cd "$CASE7" && ruby scripts/qa/jq-status-observed.rb --list-files) | wc -l | tr -d ' ')"
printf '#!/usr/bin/env bash\nexit 0\n' > "$CASE7/scripts/qa/test-freshly-registered.sh"
chmod +x "$CASE7/scripts/qa/test-freshly-registered.sh"
ruby -rjson -e '
  path = ARGV[0]
  doc = JSON.parse(File.read(path))
  doc["scripts"] << {
    "path" => "scripts/qa/test-freshly-registered.sh",
    "enforcement" => "ci-required",
    "workflow" => ".github/workflows/ci.yml",
    "job" => "governance",
    "providerIsolation" => { "mode" => "no-provider" },
    "note" => "fixture: registered after the scanner was written",
  }
  File.write(path, JSON.pretty_generate(doc) + "\n")
' "$CASE7/$SURFACE"
scanned_after="$( (cd "$CASE7" && ruby scripts/qa/jq-status-observed.rb --list-files) | wc -l | tr -d ' ')"
if [[ "$scanned_after" -eq $((scanned_before + 1)) ]] \
   && (cd "$CASE7" && ruby scripts/qa/jq-status-observed.rb --list-files) | grep -qxF "scripts/qa/test-freshly-registered.sh"; then
  pass "a gate registered after this scanner was written is in scope without editing it"
else
  fail "the scanned set did not follow the manifest ($scanned_before -> $scanned_after)"
fi

echo ""
echo "=== JSON read status: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
