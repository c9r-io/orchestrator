#!/usr/bin/env bash
#
# FR-133 dependency policy — QA gate for scripts/qa/dependency-policy.rb and for
# deny.toml itself.
#
# Every rule dependency-policy.rb defines has zero violations in this repository
# today. That is the point of a guard, and it is also the problem: a rule nobody
# has tried to trip is a rule nobody knows can fire, and a rule that fires on
# correct input gets switched off long before it catches anything. So every
# "must fire" case below is paired with a "must not fire" one on the same probe.
#
# Two halves, and they need different things:
#
#   (default)         the gate, over scratch copies of deny.toml, Cargo.lock,
#                     security.yml and .cargo/audit.toml. Needs ruby only, and
#                     runs in ci.yml's governance job.
#   --tool-fixtures   the policy, against the real cargo-deny. Needs the binary,
#                     so it runs in security.yml's cargo-deny job where the
#                     binary is installed. Asserting that a flag is present in a
#                     YAML file is not the same claim as asserting the ratchet
#                     ratchets, and only the second one is worth trusting.
#
# The mutations go through scripts/lib/gate_fixture.sh, so a fixture whose
# target has moved fails loudly instead of proving nothing (FR-143). That is not
# ceremony here: several of these substitutions name a line of deny.toml, and
# deny.toml is a file this gate exists to let people edit.
#
# Safety: read-only against the working tree. Every case builds a scratch tree
# under $TMPDIR. No daemon starts, no database is touched, no provider is
# invoked, and nothing reaches the network — cargo-deny's bans and licenses
# checks read the lock and the registry cache, never the advisory database,
# which is the half this policy deliberately leaves to cargo audit.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/dependency-policy.rb"
POLICY="deny.toml"
WORKFLOW=".github/workflows/security.yml"
AUDIT=".cargo/audit.toml"
LOCK="Cargo.lock"
DEPENDABOT=".github/dependabot.yml"
SURFACE="config/governance/qa-gate-surface.json"

TOOL_MODE=0
[[ "${1:-}" == "--tool-fixtures" ]] && TOOL_MODE=1

for command in ruby shasum mktemp awk; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ "$TOOL_MODE" -eq 1 ]]; then
  # `>/dev/null` without `2>&1`: `command -v` writes the path it found to
  # stdout and nothing to stderr, so there is no diagnostic here to hide. The
  # spelling is deliberate — check 8 of test-qa-gate-surface.sh forbids
  # `cargo ... >/dev/null 2>&1` because a gate that reports "FAIL: cargo test"
  # and nothing else costs a local reproduction to diagnose, and that rule is
  # right. The cargo-deny runs below capture their output and print it.
  command -v cargo >/dev/null || {
    echo "missing required command: cargo" >&2
    exit 1
  }
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr133-dependency-policy.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

CASE="$WORK/case"

# A scratch checkout holding only the artefacts the gate reads. Restored
# before every case, so no case can be satisfied by a previous one's mutation.
#
# The npm stubs are a hand-kept list where the rule they serve derives its set,
# and that is deliberate: if a fourth tree appears in the repository, the real
# dependabot.yml grows an entry, the scratch copy inherits it, case 1 fails
# loudly on the missing stub, and this list gets the fourth line. Deriving the
# stubs here would need a find-into-while feed, whose unobserved exit status is
# §4.4 shape 5 — the fail-loud list is the safer of the two.
reset_case() {
  rm -rf "$CASE"
  mkdir -p "$CASE/.github/workflows" "$CASE/.cargo"
  cp "$REPO_ROOT/$POLICY" "$CASE/$POLICY"
  cp "$REPO_ROOT/$LOCK" "$CASE/$LOCK"
  cp "$REPO_ROOT/$WORKFLOW" "$CASE/$WORKFLOW"
  cp "$REPO_ROOT/$AUDIT" "$CASE/$AUDIT"
  cp "$REPO_ROOT/$DEPENDABOT" "$CASE/$DEPENDABOT"
  local tree
  for tree in gui site .claude/skills/project-bootstrap/assets/template/portal; do
    mkdir -p "$CASE/$tree"
    printf '{}\n' > "$CASE/$tree/package.json"
  done
}

# The exit status is read through `if`, never from `$?` after an assignment:
# every caller of this reaches it from condition position, where ERR is live and
# the assignment would have consumed the status first (FR-144).
GATE_OUT=""
GATE_STATUS=0
run_gate() {
  if GATE_OUT="$(ruby "$REPO_ROOT/$GATE" --repo-root "$CASE" 2>&1)"; then
    GATE_STATUS=0
  else
    GATE_STATUS=$?
  fi
}

# A rule must fire *and* the gate must exit non-zero. The rule tag alone is a
# proxy: a gate that prints findings and returns 0 blocks nothing.
fires() {
  local label="$1" rule="$2"
  run_gate
  if [[ "$GATE_STATUS" -eq 0 ]]; then
    fail "$label: the gate exited 0"
    return
  fi
  if grep -q "\[$rule\]" <<<"$GATE_OUT"; then
    pass "$label"
  else
    fail "$label: expected [$rule], got: $(grep -oE '\[[a-z-]+\]' <<<"$GATE_OUT" | sort -u | tr '\n' ' ')"
  fi
}

# Silence is asserted on the summary line rather than on the exit code, because
# a gate that crashed before reading anything also exits 0 in some shells.
silent() {
  local label="$1"
  run_gate
  if [[ "$GATE_STATUS" -eq 0 ]] && grep -q '^Dependency policy: PASS' <<<"$GATE_OUT"; then
    pass "$label"
  else
    fail "$label: $(head -2 <<<"$GATE_OUT" | tr '\n' ' ')"
  fi
}

# from -> to, in place, routed through fixture_mutate so a pattern that no longer
# matches is a failed assertion rather than a silent no-op.
sub() {
  local label="$1" file="$2" from="$3" to="$4"
  fixture_mutate "$label" "$CASE/$file" \
    ruby -e 'path, from, to = ARGV; File.write(path, File.read(path).sub(from, to))' \
    "$CASE/$file" "$from" "$to"
}

# The same, anchored to a whole line.
#
# This exists because the unanchored form silently hit the wrong place: deny.toml
# documents its own severities in its header, so `sub` on
# `unused-allowed-license = "deny"` rewrote a *comment*, the digest changed,
# fixture_mutate was satisfied, and the setting the case meant to break stayed
# exactly as it was. A landed mutation is not a mutation of the intended target
# — that is the residue DD-155 names, and here the case's own assertion is what
# caught it.
sub_line() {
  local label="$1" file="$2" from="$3" to="$4"
  fixture_mutate "$label" "$CASE/$file" \
    ruby -e 'path, from, to = ARGV
             text = File.read(path)
             File.write(path, text.sub(/^#{Regexp.escape(from)}$/, to))' \
    "$CASE/$file" "$from" "$to"
}

echo "=== FR-133: dependency policy ==="
echo ""

if [[ "$TOOL_MODE" -eq 0 ]]; then

echo "-- 1. the repository's own policy is clean --"
reset_case
silent "1. the four artefacts as committed produce no findings"
echo ""

echo "-- 2. the ratchet must stay armed --"
reset_case
if sub "2" "$WORKFLOW" "--deny unmatched-skip " ""; then
  fires "2. dropping --deny unmatched-skip is a finding" "ratchet-armed"
fi
reset_case
# The drop that actually happens is someone reformatting a long command, not
# someone deleting a step — so the control moves the flag rather than removing
# it, and the gate must not care where it sits.
if sub "2b" "$WORKFLOW" \
    "check --deny unmatched-skip bans licenses sources" \
    "check bans licenses sources --deny unmatched-skip"; then
  silent "2b. the same flag after the check names is not a finding"
fi
echo ""

echo "-- 3. the two tools stay partitioned --"
reset_case
if sub "3" "$WORKFLOW" "bans licenses sources" "bans licenses sources advisories"; then
  fires "3. adding advisories to cargo-deny's check list is a finding" "checks-partitioned"
fi
reset_case
if sub "3b" "$WORKFLOW" "bans licenses sources" "all"; then
  fires "3b. collapsing the list to 'all' is a finding" "checks-partitioned"
fi
reset_case
if sub "3c" "$WORKFLOW" "bans licenses sources" "sources bans licenses"; then
  silent "3c. the same three in another order is not a finding"
fi
echo ""

echo "-- 4. the policy must actually execute --"
reset_case
# A grep for `cargo deny` is satisfied by this. The gate reads the parsed step,
# where a commented-out command is not a command (FR-134).
if sub "4" "$WORKFLOW" "run: cargo deny" "run: \"true\" # cargo deny"; then
  fires "4. a commented-out invocation is not an invocation" "deny-job-present"
fi
reset_case
if sub "4b" "$WORKFLOW" \
    "      - name: Check dependency policy" \
    "      - name: Check dependency policy
        continue-on-error: true"; then
  fires "4b. continue-on-error on the deny step is a finding" "deny-job-present"
fi
echo ""

echo "-- 5. the severities must bind --"
reset_case
if sub_line "5" "$POLICY" 'multiple-versions = "deny"' 'multiple-versions = "warn"'; then
  fires "5. multiple-versions as a warning is a finding" "severity-binding"
fi
reset_case
if sub_line "5b" "$POLICY" 'unused-allowed-license = "deny"' 'unused-allowed-license = "warn"'; then
  fires "5b. an unused allow entry as a warning is a finding" "severity-binding"
fi
reset_case
if sub_line "5c" "$POLICY" 'unknown-git = "deny"' 'unknown-git = "allow"'; then
  fires "5c. allowing unknown git sources is a finding" "severity-binding"
fi
echo ""

echo "-- 6. no blanket --"
reset_case
if sub_line "6" "$POLICY" 'skip = [' 'skip-tree = [{ crate = "tauri" }]
skip = ['; then
  fires "6. a skip-tree entry is a finding" "no-blanket"
fi
reset_case
if sub_line "6b" "$POLICY" 'skip = [' 'skip-tree = []
skip = ['; then
  silent "6b. an empty skip-tree accepts nothing and is not a finding"
fi
reset_case
# The words appear in this file's own comments and in the reason strings the
# gate reads. A grep-based check flags its own documentation; a parse does not.
if sub "6c" "$POLICY" \
    '{ crate = "base64@0.21.7", reason = "' \
    '{ crate = "base64@0.21.7", reason = "not a skip-tree = [{ crate = \"tauri\" }] entry. '; then
  silent "6c. skip-tree inside a comment or a reason string is not a skip-tree"
fi
echo ""

echo "-- 7. every acceptance carries a reason --"
reset_case
if sub "7" "$POLICY" \
    '{ crate = "base64@0.21.7", reason = "' \
    '{ crate = "base64@0.21.7", reason = "" }, { crate = "unused@0.0.0", reason = "'; then
  fires "7. an empty reason is a finding" "every-acceptance-reasoned"
fi
reset_case
if sub_line "7b" "$POLICY" 'skip = [' 'skip = [
  { crate = "base64@0.21.7" },'; then
  fires "7b. a skip with no reason key at all is a finding" "every-acceptance-reasoned"
fi
reset_case
# cargo-deny rejects a `reason` key on a licence exception, so the comment above
# it is the only place a justification can live. Removing it must be visible.
if sub "8" "$POLICY" '  # The only dependency in the tree' '  ## DELETED
'; then
  if sub "8-cont" "$POLICY" '  ## DELETED' ''; then
    fires "8. a licence exception with no comment above it is a finding" "every-acceptance-reasoned"
  fi
fi
echo ""

echo "-- 9. every accepted duplicate is still a duplicate --"
reset_case
if sub_line "9" "$POLICY" 'skip = [' 'skip = [
  { crate = "serde@1.0.229", reason = "serde resolves to exactly one version" },'; then
  fires "9. skipping a crate with one version is a finding" "skip-is-live"
fi
reset_case
if sub "9b" "$POLICY" '{ crate = "base64@0.21.7"' '{ crate = "base64@0.21.99"'; then
  fires "9b. skipping a version the lock does not contain is a finding" "skip-is-live"
fi
reset_case
if sub "9c" "$POLICY" '{ crate = "base64@0.21.7"' '{ crate = "no-such-crate@1.0.0"'; then
  fires "9c. skipping a crate absent from the lock is a finding" "skip-is-live"
fi
echo ""

echo "-- 10. the advisory half --"
reset_case
if sub "10" "$WORKFLOW" "cargo audit --deny unsound" "cargo audit"; then
  fires "10. cargo audit without --deny unsound is a finding" "audit-unsound-denied"
fi
reset_case
if sub "10b" "$AUDIT" '  # RUSTSEC-2024-0429' '  ## MARK'; then
  # Erase every comment between the array opener and the id, so the id is the
  # first thing inside it. One substitution cannot do that; two can.
  if fixture_mutate "10b-strip" "$CASE/$AUDIT" \
      ruby -e 'path = ARGV[0]
               text = File.read(path)
               head, tail = text.split("ignore = [", 2)
               tail = tail.sub(/\A.*?(?=  "RUSTSEC)/m, "\n")
               File.write(path, head + "ignore = [" + tail)' \
      "$CASE/$AUDIT"; then
    fires "10b. an ignored advisory with no reason above it is a finding" "audit-unsound-denied"
  fi
fi
reset_case
rm -f "$CASE/$AUDIT"
fires "10c. a missing acceptance file is a finding, not an empty pass" "audit-unsound-denied"
echo ""

echo "-- 11. a scan that read nothing is not a clean scan --"
reset_case
if fixture_mutate "11" "$CASE/$WORKFLOW" \
    ruby -e 'File.write(ARGV[0], "name: Security\non: push\njobs: {}\n")' "$CASE/$WORKFLOW"; then
  fires "11. a workflow with no jobs fails rather than passing vacuously" "empty-scan"
fi
reset_case
if fixture_mutate "11b" "$CASE/$LOCK" \
    ruby -e 'File.write(ARGV[0], "version = 4\n")' "$CASE/$LOCK"; then
  fires "11b. a lock with no packages fails rather than passing vacuously" "empty-scan"
fi
echo ""

echo "-- 12. coverage follows the manifest --"
reset_case
for script in "$GATE" "scripts/qa/test-dependency-policy.sh"; do
  if ruby -rjson -e '
        manifest, path = ARGV
        entry = JSON.parse(File.read(manifest))["scripts"].find { |s| s["path"] == path }
        exit 1 unless entry && entry["enforcement"] == "ci-required"
      ' "$REPO_ROOT/$SURFACE" "$script"; then
    pass "12. $script is registered ci-required"
  else
    fail "12. $script is not registered ci-required in $SURFACE"
  fi
done
echo ""

# Cases 19–21 continue past the tool-mode block's 14–18 so every case id in
# this file names exactly one case.

echo "-- 19. the prose counts are derived, not trusted --"
reset_case
if sub "19" "$POLICY" \
    "# 48 crates resolve to more than one version; 71 extra copies are accepted here," \
    "# 47 crates resolve to more than one version; 71 extra copies are accepted here,"; then
  fires "19. a stale crate count in the header is a finding" "prose-counts-derived"
fi
reset_case
# Rewording, not renumbering: the mutation the rule is least likely to catch is
# the sentence disappearing, which a number comparison never sees.
if sub "19b" "$POLICY" \
    "resolve to more than one version; 71 extra copies" \
    "resolve to more than one version; a number of extra copies"; then
  fires "19b. rewording the counts away is a finding, not a skip" "prose-counts-derived"
fi
reset_case
# The victim number is derived from the live prose, never hardcoded: a
# dependency change moves the real count (clap_complete's removal took it
# 654 → 653 during FR-154) and a literal from-string then mutates nothing —
# fixture target drift, caught by fixture_mutate's digest check.
current_pkgs="$(sed -n 's/.*one of the \([0-9][0-9]*\) external packages.*/\1/p' "$CASE/$POLICY")"
current_pkgs="${current_pkgs%%$'\n'*}"
if [ -z "$current_pkgs" ]; then
  fail "19c: cannot derive the external-package count from the policy prose"
elif sub "19c" "$POLICY" \
    "$current_pkgs external packages" \
    "$((current_pkgs + 1)) external packages"; then
  fires "19c. a stale external-package count is a finding" "prose-counts-derived"
fi
reset_case
if sub "19d" "$POLICY" \
    "extra copies are accepted here," \
    "extra copies are accepted here (each with a reason),"; then
  silent "19d. prose beyond the anchored phrase may change freely"
fi
echo ""

echo "-- 20. npm coverage is derived from the tree --"
reset_case
# The removal that happened (3446b652) was wholesale deletion; the removal a
# reviewer misses is a commented-out entry. This is the latter.
if fixture_mutate "20" "$CASE/$DEPENDABOT" \
    ruby -e 'path = ARGV[0]
             lines = File.readlines(path)
             i = lines.index("    directory: /site\n") or abort "no /site npm entry to comment out"
             start = i
             start -= 1 until lines[start].start_with?("  - ")
             stop = start + 1
             stop += 1 until stop >= lines.length || lines[stop].start_with?("  - ")
             (start...stop).each { |j| lines[j] = "# " + lines[j] }
             File.write(path, lines.join)' \
    "$CASE/$DEPENDABOT"; then
  fires "20. a commented-out npm entry is a missing entry" "dependabot-npm-coverage"
fi
reset_case
mkdir -p "$CASE/newtree"
printf '{}\n' > "$CASE/newtree/package.json"
fires "20b. a package.json outside the covered set is a finding" "dependabot-npm-coverage"
reset_case
rm -f "$CASE/gui/package.json"
fires "20c. an npm entry whose tree is gone is a finding" "dependabot-npm-coverage"
reset_case
rm -f "$CASE/$DEPENDABOT"
fires "20d. a missing dependabot.yml is a finding, not an empty pass" "dependabot-npm-coverage"
reset_case
if fixture_mutate "20e" "$CASE/$DEPENDABOT" \
    ruby -e 'path = ARGV[0]
             text = File.read(path)
             File.write(path, text.sub("      interval: weekly\n    open-pull-requests-limit: 5", "      interval: daily\n    open-pull-requests-limit: 5"))' \
    "$CASE/$DEPENDABOT"; then
  silent "20e. the assertion is coverage, not cadence"
fi
echo ""

echo "-- 21. the unmaintained ledger binds --"
reset_case
if sub "21" "$WORKFLOW" \
    "cargo audit --deny unsound --deny unmaintained" \
    "cargo audit --deny unsound"; then
  fires "21. cargo audit without --deny unmaintained is a finding" "audit-unsound-denied"
fi
reset_case
if sub "21b" "$WORKFLOW" \
    "--deny unsound --deny unmaintained" \
    "--deny unmaintained --deny unsound"; then
  silent "21b. the same flags in another order are not a finding"
fi
echo ""

# FR-165 requirement 4. cargo-audit has no `--deny unmatched-ignore`, so until
# now an acceptance whose crate had left the tree stayed forever: it accepted
# nothing and held the advisory ID reserved. `check_audit` asked only whether an
# ignore had *a* comment above it, which is the §4.4 shape 1 proxy — the
# retirement conditions were prose, for a human, and nobody read them.
echo "-- 22. every acceptance is still accepting something --"
reset_case
if fixture_mutate "22" "$CASE/$AUDIT" \
    ruby -e 'path = ARGV[0]
             text = File.read(path)
             File.write(path, text.sub("  # retire-when: crate=atk absent\n", ""))' \
    "$CASE/$AUDIT"; then
  fires "22. an acceptance with no retirement declaration is a finding" "audit-ignore-is-live"
fi

# The crate is renamed rather than deleted from the lock: an upstream rename is
# how this actually happens, and it leaves the advisory ID pointing at nothing
# while the file still looks complete.
reset_case
if sub "22b" "$AUDIT" \
    "# retire-when: crate=paste absent" \
    "# retire-when: crate=paste-renamed-upstream absent"; then
  run_gate
  if [[ "$GATE_STATUS" -ne 0 ]] && grep -q 'not in Cargo.lock at all' <<<"$GATE_OUT"; then
    pass "22b. an acceptance whose crate left the lock is named"
  else
    fail "22b. expected the not-in-lock diagnostic: $(grep -oE '\[[a-z-]+\]' <<<"$GATE_OUT" | sort -u | tr '\n' ' ')"
  fi
fi

# The reverse instance, and the reason this check is not just a port of
# skip-is-live. deny.toml's `--deny unmatched-skip` is satisfied by a crate that
# is merely present; the audit analogue of "present but no longer duplicated" is
# "present but already fixed", which a presence check cannot see. FR-133 recorded
# that gap on the deny side as case 15b and it should not repeat here.
reset_case
if sub "22c" "$AUDIT" \
    "# retire-when: crate=glib patched>=0.20.0" \
    "# retire-when: crate=glib patched>=0.18.0"; then
  run_gate
  if [[ "$GATE_STATUS" -ne 0 ]] && grep -q 'the advisory is fixed' <<<"$GATE_OUT"; then
    pass "22c. a crate still in the tree at a patched version is named"
  else
    fail "22c. expected the advisory-is-fixed diagnostic: $(grep -oE '\[[a-z-]+\]' <<<"$GATE_OUT" | sort -u | tr '\n' ' ')"
  fi
fi

# Version comparison is numeric, and this is the direction a string compare gets
# wrong: paste is locked at 1.0.15, and `"1.0.15" >= "0.9.0"` is false as text
# because "1" sorts before "9". A lexical implementation would go on accepting a
# fixed advisory here and say nothing. The companion direction is the committed
# state itself — glib 0.18.5 against a 0.20.0 bound, where a text compare of the
# minor component ("18" < "20") happens to agree, which is why this case is the
# one that has to exist.
reset_case
if sub "22d" "$AUDIT" \
    "# retire-when: crate=paste absent" \
    "# retire-when: crate=paste patched>=0.9.0"; then
  run_gate
  if [[ "$GATE_STATUS" -ne 0 ]] && grep -q 'the advisory is fixed' <<<"$GATE_OUT"; then
    pass "22d. 1.0.15 counts as past 0.9.0; the comparison is numeric, not lexical"
  else
    fail "22d. a lexical version compare would have missed this: $(grep -oE '\[[a-z-]+\]' <<<"$GATE_OUT" | sort -u | tr '\n' ' ')"
  fi
fi

# The mirror of 22c: a bound one patch above the locked version must stay
# accepted. Without this the check could satisfy every case above by reporting
# "fixed" unconditionally.
reset_case
if sub "22e" "$AUDIT" \
    "# retire-when: crate=gtk absent" \
    "# retire-when: crate=gtk patched>=0.18.3"; then
  silent "22e. gtk 0.18.2 against a 0.18.3 bound is still accepted"
fi

# An empty lock makes every acceptance vacuously live, so it must fail closed.
# skip-is-live emits `empty-scan` for the same lock, so the rule tag alone cannot
# say whether *this* check observed anything — the detail is asserted instead.
reset_case
if fixture_mutate "22f" "$CASE/$LOCK" \
    ruby -e 'File.write(ARGV[0], "")' "$CASE/$LOCK"; then
  run_gate
  if [[ "$GATE_STATUS" -ne 0 ]] && grep -q 'audit-ignore-is-live examined nothing' <<<"$GATE_OUT"; then
    pass "22f. an empty lock fails closed for the acceptances too, not only the skips"
  else
    fail "22f. expected the audit-ignore-is-live empty-scan detail: $(head -2 <<<"$GATE_OUT" | tr '\n' ' ')"
  fi
fi
echo ""

echo "-- 13. the gate passes on this repository --"
if ruby "$REPO_ROOT/$GATE" >/dev/null 2>&1; then
  pass "13. dependency-policy.rb passes against the working tree"
else
  fail "13. dependency-policy.rb fails against the working tree"
fi
echo ""

else # ── --tool-fixtures ──────────────────────────────────────────────────────

# These run cargo-deny against a mutated *config* and the repository's real
# manifest, so no workspace copy is needed and nothing in the tree is touched.
DENY_OUT=""
DENY_STATUS=0
run_deny() {
  if DENY_OUT="$(cd "$REPO_ROOT" && cargo deny --workspace --all-features \
      --config "$CASE/$POLICY" check --deny unmatched-skip "$@" 2>&1)"; then
    DENY_STATUS=0
  else
    DENY_STATUS=$?
  fi
}

reset_tool_case() {
  # The full four-artefact tree, not just the policy: case 15b runs both
  # observers over the same mutation to show they cover different halves.
  reset_case
}

echo "-- 14. the committed policy passes --"
reset_tool_case
run_deny bans licenses sources
if [[ "$DENY_STATUS" -eq 0 ]]; then
  pass "14. cargo deny check bans licenses sources exits 0 on the committed policy"
else
  fail "14. the committed policy does not pass: $(grep -E '^(error|bans|licenses|sources)' <<<"$DENY_OUT" | sed -n '1,3p' | tr '\n' ' ')"
fi
echo ""

echo "-- 15. the ratchet ratchets --"
reset_tool_case
if sub_line "15" "$POLICY" 'skip = [' 'skip = [
  { crate = "serde@1.0.999", reason = "a version that has left the graph" },'; then
  run_deny bans
  if [[ "$DENY_STATUS" -ne 0 ]] && grep -q "unmatched-skip" <<<"$DENY_OUT"; then
    pass "15. an entry naming a version the graph no longer has fails the build"
  else
    fail "15. exit $DENY_STATUS, and the word unmatched-skip did not appear"
  fi
fi
echo ""

echo "-- 15b. and where it does not, the other observer does --"
reset_tool_case
# `unmatched-skip` means the skip's crate@version is absent from the graph. It
# says nothing about a version that is still present but no longer duplicated —
# which is what happens when the graph converges onto the *older* copy. Measured
# here rather than assumed: cargo deny passes this mutation, and
# dependency-policy.rb's skip-is-live rule is the only thing that catches it.
if sub_line "15b" "$POLICY" 'skip = [' 'skip = [
  { crate = "serde@1.0.229", reason = "present in the lock, but not duplicated" },'; then
  run_deny bans
  run_gate
  if [[ "$DENY_STATUS" -eq 0 ]] && [[ "$GATE_STATUS" -ne 0 ]] && grep -q '\[skip-is-live\]' <<<"$GATE_OUT"; then
    pass "15b. cargo deny accepts a skip for a crate that is no longer duplicated; skip-is-live does not"
  else
    fail "15b. cargo deny exit $DENY_STATUS, gate exit $GATE_STATUS — the two are meant to cover different halves"
  fi
fi
echo ""

echo "-- 16. a duplicate that loses its acceptance fails --"
reset_tool_case
if fixture_mutate "16" "$CASE/$POLICY" \
    ruby -e 'path = ARGV[0]
             text = File.read(path)
             File.write(path, text.gsub(/^  \{ crate = "base64@0\.21\.7".*\n/, ""))' \
    "$CASE/$POLICY"; then
  run_deny bans
  if [[ "$DENY_STATUS" -ne 0 ]] && grep -q "duplicate entries for crate 'base64'" <<<"$DENY_OUT"; then
    pass "16. removing an acceptance makes cargo deny name that crate"
  else
    fail "16. exit $DENY_STATUS, and base64 was not named"
  fi
fi
echo ""

echo "-- 18. the sources check reads the real graph --"
reset_tool_case
# `sources` has zero findings today and would have zero findings if it were
# evaluating nothing, which is the whole reason a guard needs a fixture. Emptying
# the allowed-registry list makes crates.io itself unknown: if the check is live,
# every package in the graph is rejected.
if sub_line "18" "$POLICY" 'unknown-registry = "deny"' 'unknown-registry = "deny"
allow-registry = []'; then
  run_deny sources
  if [[ "$DENY_STATUS" -ne 0 ]] && grep -q "source-not-allowed" <<<"$DENY_OUT"; then
    pass "18. with no registry allowed, every package is a finding"
  else
    fail "18. exit $DENY_STATUS, and source-not-allowed did not appear"
  fi
fi
echo ""

echo "-- 17. the licence check reads the real graph --"
reset_tool_case
if sub "17" "$POLICY" '  "MPL-2.0",' ''; then
  run_deny licenses
  if [[ "$DENY_STATUS" -ne 0 ]] && grep -q "MPL-2.0" <<<"$DENY_OUT"; then
    pass "17. removing MPL-2.0 from allow fails and names the licence"
  else
    fail "17. exit $DENY_STATUS, and MPL-2.0 was not named"
  fi
fi
echo ""

fi

echo "=== $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]]
