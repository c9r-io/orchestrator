#!/usr/bin/env bash
#
# FR-152: the error-code glossary matches the source-derived code set.
#
# The orchestrator prints machine-readable bracketed codes —
# [driver_config_invalid], [secret_value_placeholder_rejected] — straight to the
# user's terminal, concentrated on the first-run path. docs/guide/error-codes.md
# is the glossary; this gate keeps it honest in both directions: a code the
# product can emit but the glossary does not explain fails, and so does a
# glossary entry whose code no longer exists in source. The set is derived,
# never hand-typed (§4.4 shape 2), by three anchored rules:
#
#   A. a bracketed snake_case literal opening a string: "\[[a-z][a-z0-9_]+\]
#      (the quote anchor excludes [{ts}]-style format placeholders);
#   B. the first string argument of driver_error(...) — those codes reach the
#      terminal through the "[{code}]" formatter in workflow_steps.rs, which a
#      literal-bracket grep cannot see (§4.4 shape 4, found while verifying
#      this FR: the FR's own count missed all seven);
#   C. an interpolated SCREAMING_CASE const opening a string ("[{NAME}]"),
#      resolved to the const's value in the same file — unresolvable is a
#      failure, not a skip.
#
# Scope is every tracked .rs under core/src and crates/*/src, minus tests/
# directories, tests.rs / *_tests.rs basenames, and inline #[cfg(test)] modules
# (stripped with scripts/lib/rust_source.rb's brace-matching helper).
#
# This basename rule used to be narrower than rust_source.rb's on purpose: that
# function excluded by the spelling /test.*\.rs\z/, which swallowed
# scheduler/safety/self_test.rs — a production module emitting
# [empty_change_check] — and DD-163 recorded the near-miss. **That reason has
# expired.** rust_source.rb now confirms a basename match against the file's mod
# declaration instead of trusting it, so it scans self_test.rs too and the two
# predicates agree about the file this comment was written for.
#
# One difference survives, and it runs the other way: rust_source.rb also
# excludes cfg-gated helpers like test_utils.rs and test_support.rs, which this
# gate still scans because their basenames are neither tests.rs nor *_tests.rs.
# That is wider, not narrower, and it has never produced a finding here. Adopting
# RustSource.test_only_file? wholesale would be the unification, and it is a
# behaviour change to this gate's scan that wants its own before/after rather
# than riding along with the rust_source.rb repair.
#
# Exclusions carry a reason and are checked for staleness: an excluded token
# the scan no longer produces fails the gate, so the list cannot outlive the
# code it excuses.
#
# Usage:
#   test-error-code-glossary.sh                 verify the real repository
#   test-error-code-glossary.sh --fixture-test  also prove the checks fail on injected defects

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

for command in ruby git; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

EN_DOC="$REPO_ROOT/docs/guide/error-codes.md"
ZH_DOC="$REPO_ROOT/docs/guide/zh/error-codes.md"

# ── Derivation ────────────────────────────────────────────────────────────────
#
# Prints the derived code set, one per line, sorted. Fails closed: empty file
# list, zero raw hits, an unresolvable rule-C const, or a stale exclusion all
# exit non-zero with a diagnostic — a derivation that scanned nothing must
# never hand the comparison an empty set it would happily match against an
# empty document (§4.4 shape 5).
derive_codes() {
  ruby -r "$REPO_ROOT/scripts/lib/rust_source" - "$REPO_ROOT" <<'RUBY'
require "pathname"

root = Pathname.new(ARGV[0])

# Excluded tokens: things the extraction rules hit that are not user-facing
# error codes. Every entry must still be produced by the raw scan; an entry
# the scan no longer yields is stale and fails the derivation.
EXCLUDED = {
  "fs_watcher" => "daemon stderr log label (eprintln in crates/daemon/src/fs_watcher.rs), " \
                  "an operator log prefix with no remediation semantics",
}.freeze

files = `git -C #{root} ls-files -z`.split("\0")
                                    .grep(%r{\A(core/src|crates/[^/]+/src)/.*\.rs\z})
abort "derivation scanned nothing: git ls-files returned no Rust sources" if files.empty?

raw = Hash.new { |hash, key| hash[key] = [] }
scanned = 0
files.each do |rel|
  next if rel.split("/").include?("tests")
  base = File.basename(rel)
  next if base == "tests.rs" || base.end_with?("_tests.rs")

  source = RustSource.strip_test_modules(File.read(root.join(rel)))
  scanned += 1
  source.scan(/"\[([a-z][a-z0-9_]+)\]/) { |(code)| raw[code] << rel }
  source.scan(/driver_error\(\s*"([a-z][a-z0-9_]+)"/m) { |(code)| raw[code] << rel }
  source.scan(/"\[\{([A-Z][A-Z0-9_]*)\}\]/) do |(konst)|
    if source =~ /const #{konst}: &str = "([^"]+)"/
      raw[Regexp.last_match(1)] << rel
    else
      abort "rule C cannot resolve const #{konst} in #{rel}: " \
            "the interpolated code is invisible to the glossary"
    end
  end
end

abort "derivation scanned zero files after exclusions" if scanned.zero?
abort "derivation produced zero raw tokens across #{scanned} files" if raw.empty?

EXCLUDED.each_key do |token|
  next if raw.key?(token)

  abort "stale exclusion: '#{token}' is excluded but the scan no longer produces it — " \
        "delete the exclusion"
end

derived = raw.keys - EXCLUDED.keys
abort "every raw token is excluded; the glossary would assert nothing" if derived.empty?

puts derived.sort
RUBY
}

# Prints the codes documented in a glossary file: one `## `code`` heading each.
doc_codes() {
  sed -n 's/^## `\([A-Za-z0-9_]*\)`$/\1/p' "$1" | LC_ALL=C sort
}

# ── Checks ────────────────────────────────────────────────────────────────────

# The derivation itself: it must run and produce a plausible set.
check_derivation_produces_codes() {
  local derived="$1"
  [[ -s "$derived" ]] || {
    echo "    the derivation produced an empty set" >&2
    return 1
  }
  # A sanity anchor: some code the derivation must find, or its scope is wrong
  # whatever else it turned up. FR-173 retired the previous anchor
  # (legacy_agent_command_deprecated) along with the surface it named, so the
  # anchor moved to a code emitted from a different file and a different layer —
  # picking one from the same file the derivation already reads would make this
  # check agree with itself.
  grep -qx "driver_config_invalid" "$derived" || {
    echo "    derivation sanity: driver_config_invalid is missing from the set" >&2
    return 1
  }
}

# Both directions of EN-vs-source, with named diffs.
check_en_doc_matches_derived() {
  local derived="$1" en_doc="$2" rc=0
  [[ -f "$en_doc" ]] || {
    echo "    $en_doc does not exist" >&2
    return 1
  }
  doc_codes "$en_doc" > "$WORK/en_codes"
  [[ -s "$WORK/en_codes" ]] || {
    echo "    no '## \`code\`' headings found in $en_doc — the extraction is reading nothing" >&2
    return 1
  }
  local undocumented spurious
  undocumented="$(comm -23 "$derived" "$WORK/en_codes")"
  if [[ -n "$undocumented" ]]; then
    echo "    source emits code(s) the glossary does not document:" >&2
    printf '      %s\n' $undocumented >&2
    rc=1
  fi
  spurious="$(comm -13 "$derived" "$WORK/en_codes")"
  if [[ -n "$spurious" ]]; then
    echo "    the glossary documents code(s) the source no longer emits:" >&2
    printf '      %s\n' $spurious >&2
    rc=1
  fi
  return "$rc"
}

# The ZH mirror documents exactly the same set as EN.
check_zh_doc_matches_en() {
  local en_doc="$1" zh_doc="$2"
  [[ -f "$zh_doc" ]] || {
    echo "    $zh_doc does not exist" >&2
    return 1
  }
  doc_codes "$en_doc" > "$WORK/en_codes_for_zh"
  doc_codes "$zh_doc" > "$WORK/zh_codes"
  if ! diff "$WORK/en_codes_for_zh" "$WORK/zh_codes" > "$WORK/zh_diff" 2>&1; then
    echo "    EN and ZH glossaries document different code sets:" >&2
    sed 's/^/      /' "$WORK/zh_diff" >&2
    return 1
  fi
  [[ -s "$WORK/zh_codes" ]] || {
    echo "    the ZH glossary has zero code headings" >&2
    return 1
  }
}

ALL_CHECKS=(check_derivation_produces_codes check_en_doc_matches_derived check_zh_doc_matches_en)

# meta: every check_* function defined in this file is registered.
defined_checks() {
  grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()//' | LC_ALL=C sort
}

echo "=== FR-152: error-code glossary parity ==="
echo ""

DERIVED="$WORK/derived"
if ! derive_codes > "$DERIVED" 2> "$WORK/derive.err"; then
  echo "  FAIL: the derivation itself failed:" >&2
  sed 's/^/    /' "$WORK/derive.err" >&2
  echo ""
  echo "0 passed, 1 failed"
  exit 1
fi

registered="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort)"
if [[ "$registered" == "$(defined_checks)" ]]; then
  pass "meta: ALL_CHECKS registers every check function defined in this script"
else
  fail "meta: ALL_CHECKS drifted from the defined check functions"
fi

if check_derivation_produces_codes "$DERIVED"; then
  pass "the derived set is non-empty and contains its cross-layer sanity anchor"
else
  fail "the derivation lost its scope"
fi

if check_en_doc_matches_derived "$DERIVED" "$EN_DOC"; then
  pass "docs/guide/error-codes.md documents exactly the source-derived set ($(wc -l < "$DERIVED" | tr -d ' ') codes)"
else
  fail "the EN glossary and the source disagree"
fi

if check_zh_doc_matches_en "$EN_DOC" "$ZH_DOC"; then
  pass "the ZH glossary documents the same set as the EN glossary"
else
  fail "the ZH glossary drifted from EN"
fi

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo ""
  echo "--- negative fixtures ---"

  # Positive control is the real-mode result above: if it failed, the fixture
  # verdicts below are void, and the gate exits non-zero either way.

  # Fixture 1: comment out — not delete — one glossary heading in a private
  # copy. Commenting out is the mutation the heading regex is least likely to
  # be hand-tuned for: the code's name is still on the page, in prose and in
  # the commented line, and only the anchored heading extraction may see the
  # difference. The victim is derived (first code in the set), never named.
  victim="$(sed -n '1p' "$DERIVED")"
  cp "$EN_DOC" "$WORK/en_mutated.md"
  if fixture_mutate "fixture 1" "$WORK/en_mutated.md" \
      sh -c 'sed "s|^## \`'"$victim"'\`$|<!-- ## \`'"$victim"'\` -->|" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$WORK/en_mutated.md"; then
    f1_out="$WORK/f1.log"
    if check_en_doc_matches_derived "$DERIVED" "$WORK/en_mutated.md" > "$f1_out" 2>&1; then
      fail "fixture 1: a glossary missing '$victim' was accepted"
    elif ! grep -q "$victim" "$f1_out"; then
      fail "fixture 1: rejected, but the diagnostic does not name the missing code"
    else
      pass "fixture 1: commented-out entry rejected, diagnostic names '$victim'"
    fi
  fi

  # Fixture 2: the other direction — a heading for a code the source does not
  # emit, appended to a private copy.
  cp "$EN_DOC" "$WORK/en_spurious.md"
  if fixture_mutate "fixture 2" "$WORK/en_spurious.md" \
      sh -c 'printf "\n## \`code_that_never_existed\`\n\n- ghost entry\n" >> "$1"' _ "$WORK/en_spurious.md"; then
    f2_out="$WORK/f2.log"
    if check_en_doc_matches_derived "$DERIVED" "$WORK/en_spurious.md" > "$f2_out" 2>&1; then
      fail "fixture 2: a glossary entry for a nonexistent code was accepted"
    elif ! grep -q "code_that_never_existed" "$f2_out"; then
      fail "fixture 2: rejected, but the diagnostic does not name the spurious code"
    else
      pass "fixture 2: spurious entry rejected, diagnostic names it"
    fi
  fi

  # Fixture 3: a ZH mirror that lost one entry, in a private copy.
  cp "$ZH_DOC" "$WORK/zh_mutated.md"
  if fixture_mutate "fixture 3" "$WORK/zh_mutated.md" \
      sh -c 'sed "s|^## \`'"$victim"'\`$|<!-- ## \`'"$victim"'\` -->|" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$WORK/zh_mutated.md"; then
    f3_out="$WORK/f3.log"
    if check_zh_doc_matches_en "$EN_DOC" "$WORK/zh_mutated.md" > "$f3_out" 2>&1; then
      fail "fixture 3: a ZH mirror missing '$victim' was accepted"
    elif ! grep -q "$victim" "$f3_out"; then
      fail "fixture 3: rejected, but the diagnostic does not name the divergent code"
    else
      pass "fixture 3: ZH divergence rejected, diagnostic names '$victim'"
    fi
  fi
fi

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
exit 0
