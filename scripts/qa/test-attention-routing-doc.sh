#!/usr/bin/env bash
#
# FR-162: the documented attention routing table matches the source-derived one.
#
# docs/guide/03-workflow-configuration.md carries three generated blocks — the
# event→inbox routing table, the completion/resolution rules, and the list of
# source-side (task-less) item kinds. Hand-writing those lists is §4.4 shape 2:
# the page would document exactly the arms someone remembered the day it was
# written, and the next arm would land outside it silently. This gate derives
# all three from the source and holds the doc to the derived set in both
# directions, with the ZH mirror equal to EN:
#
#   A. routing arms — the `"event" | "event" => Some(("kind", Severity, ...))`
#      arms of policy_operations in the scheduler's attention service, including
#      the success==false guard arm, plus the low-confidence upsert with its
#      threshold read from is_low_confidence;
#   B. resolution arms — the task-completion sweep (event list, preserved-kind
#      const, resolution reason), the resume full sweep, and the successful-step
#      resolve;
#   C. external kinds — every literal `kind:` of an AttentionCandidate built
#      outside the projector (webhook, managed source, source router, connection
#      lifecycle, the projection-gap item), across all non-test production Rust.
#
# Fails closed: a derivation that finds zero arms, cannot resolve the preserved
# const, or reads a doc block with zero rows aborts rather than handing the
# comparison an empty set it would happily match (§4.4 shape 5).
#
# Usage:
#   test-attention-routing-doc.sh                 verify the real repository
#   test-attention-routing-doc.sh --emit          print the canonical blocks
#   test-attention-routing-doc.sh --fixture-test  also prove the checks fail on injected defects

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

MODE="${1:-}"
if [[ "$MODE" != "" && "$MODE" != "--fixture-test" && "$MODE" != "--emit" ]]; then
  echo "usage: $0 [--emit|--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

EN_DOC="$REPO_ROOT/docs/guide/03-workflow-configuration.md"
ZH_DOC="$REPO_ROOT/docs/guide/zh/03-workflow-configuration.md"
POLICY_DEFAULT="crates/orchestrator-scheduler/src/service/attention.rs"

# ── Derivation ────────────────────────────────────────────────────────────────
#
# Prints canonical lines: `route|events|condition|kind|severity`,
# `resolve|events|scope|preserved|reason`, `external|kind`. Fails closed on
# every missing anchor. ATTENTION_POLICY_SRC overrides the policy file (used by
# the fixtures to derive against a mutated private copy).
derive_rows() {
  ruby -r "$REPO_ROOT/scripts/lib/rust_source" - "$REPO_ROOT" <<'RUBY'
require "pathname"

root = Pathname.new(ARGV[0])
policy_path = ENV["ATTENTION_POLICY_SRC"] ||
              root.join("crates/orchestrator-scheduler/src/service/attention.rs").to_s
abort "policy source #{policy_path} does not exist" unless File.exist?(policy_path)
policy_src = RustSource.strip_test_modules(File.read(policy_path))

# Slices one top-level `fn name` body out of the source.
def region(src, name)
  start = src.index(/^(?:pub )?(?:async )?fn #{name}\b/)
  abort "fn #{name} not found in the policy source" unless start
  tail = src[start..]
  stop = tail.index(/\n(?:pub )?(?:async )?fn /, 1) || tail.length
  tail[0...stop]
end

quoted = /"([a-z0-9_]+)"/
list = /(?:"[a-z0-9_]+"\s*\|\s*)*"[a-z0-9_]+"/

policy = region(policy_src, "policy_operations")

# Resolution arms.
completion = policy[/matches!\(\s*event\.event_type\.as_str\(\),\s*(#{list})\s*\)/m, 1] or
  abort "task-completion matches! arm not found in policy_operations"
completion_events = completion.scan(quoted).flatten
abort "policy_operations lost the resume_executed arm" unless
  policy.include?('event.event_type == "resume_executed"')
abort "the completion sweep no longer stamps reason task_completed" unless
  policy.include?('reason: "task_completed"')
abort "the resume sweep no longer stamps reason condition_cleared" unless
  policy.include?('reason: "condition_cleared"')

preserved = policy_src[/const TASK_SWEEP_PRESERVED_KINDS: &\[&str\] = &\[([^\]]*)\]/m, 1] or
  abort "TASK_SWEEP_PRESERVED_KINDS const not found"
preserved_kinds = preserved.scan(quoted).flatten.sort
abort "TASK_SWEEP_PRESERVED_KINDS is empty" if preserved_kinds.empty?

success_region = region(policy_src, "is_successful_step")
success_events = success_region[/matches!\(\s*event\.event_type\.as_str\(\),\s*(#{list})/m, 1]
  &.scan(quoted)&.flatten or abort "is_successful_step event list not found"
abort "is_successful_step no longer checks success != false" unless
  success_region.include?("!= Some(false)")

low_region = region(policy_src, "is_low_confidence")
threshold = low_region[/confidence < ([0-9.]+)/, 1] or
  abort "is_low_confidence threshold not found"
low_events = low_region[/matches!\(\s*event\.event_type\.as_str\(\),\s*(#{list})/m, 1]
  &.scan(quoted)&.flatten or abort "is_low_confidence event list not found"
abort "the low-confidence upsert call disappeared" unless
  policy_src =~ /candidate\(\s*event,\s*"low_confidence",\s*AttentionSeverity::Attention/m

# Routing arms, in source order.
routes = policy.scan(
  /(#{list})\s*(if\b[^=]*?==\s*Some\(false\)\s*)?=>\s*\{?\s*Some\(\(\s*"([a-z0-9_]+)",\s*AttentionSeverity::(Intervention|Attention)/m
).map do |events, guard, kind, severity|
  condition = guard ? "payload.success == false" : "-"
  [events.scan(quoted).flatten, condition, kind, severity.downcase]
end
abort "derivation found zero routing arms in policy_operations" if routes.empty?
routes << [low_events, "confidence < #{threshold}", "low_confidence", "attention"]

# External kinds across all non-test production Rust.
files = `git -C #{root} ls-files -z`.split("\0")
                                    .grep(%r{\A(core/src|crates/[^/]+/src)/.*\.rs\z})
abort "external-kind scan found no Rust sources" if files.empty?
external = []
files.each do |rel|
  next if rel.split("/").include?("tests")
  base = File.basename(rel)
  next if base == "tests.rs" || base.end_with?("_tests.rs")

  path = rel == "crates/orchestrator-scheduler/src/service/attention.rs" ? policy_path : root.join(rel).to_s
  source = RustSource.strip_test_modules(File.read(path))
  external.concat(source.scan(/AttentionCandidate\s*\{[^;]*?kind:\s*"([a-z0-9_]+)"/m).flatten)
  external.concat(source.scan(/(?<!Some\()\(\s*"([a-z0-9_]+)",\s*AttentionSeverity::/m).flatten)
end
external = (external - ["low_confidence"]).sort.uniq
abort "external-kind scan produced zero kinds" if external.empty?

routes.each do |events, condition, kind, severity|
  puts "route|#{events.join(', ')}|#{condition}|#{kind}|#{severity}"
end
puts "resolve|#{completion_events.join(', ')}|whole task|#{preserved_kinds.join(', ')}|task_completed"
puts "resolve|resume_executed|whole task|(none)|condition_cleared"
puts "resolve|#{success_events.join(', ')} (success != false)|matching step|n/a|condition_cleared"
external.each { |kind| puts "external|#{kind}" }
RUBY
}

# Extracts the canonical lines a guide file declares, from its three generated
# blocks. Zero rows in any block is a failure, not an empty set.
doc_rows() {
  local doc="$1"
  awk '
    /<!-- attention-routing:begin -->/ { section = "route"; next }
    /<!-- attention-resolution:begin -->/ { section = "resolve"; next }
    /<!-- attention-external-kinds:begin -->/ { section = "external"; next }
    /<!-- attention-(routing|resolution|external-kinds):end -->/ { section = ""; next }
    section == "" { next }
    section == "external" {
      if (match($0, /^- `[a-z0-9_]+`/)) {
        kind = substr($0, 4, RLENGTH - 4)
        print "external|" kind
      }
      next
    }
    /^\|/ {
      if ($0 ~ /^\| *:?-+/) next
      if ($0 ~ /Source event|Trigger event/) next
      line = $0
      gsub(/^\| */, "", line); gsub(/ *\| *$/, "", line)
      gsub(/ *\| */, "|", line); gsub(/`/, "", line)
      print section "|" line
    }
  ' "$doc"
}

# ── Checks ────────────────────────────────────────────────────────────────────

check_derivation_produces_rows() {
  local derived="$1"
  [[ -s "$derived" ]] || {
    echo "    the derivation produced an empty set" >&2
    return 1
  }
  # step_failed is the arm the FR exists for; a derivation that lost it has the
  # wrong scope, whatever else it found.
  grep -q "|step_failed|" "$derived" || {
    echo "    derivation sanity: no step_failed routing row in the derived set" >&2
    return 1
  }
  grep -qx "external|source_auth_failed" "$derived" || {
    echo "    derivation sanity: source_auth_failed missing from the external kinds" >&2
    return 1
  }
}

check_en_doc_matches_derived() {
  local derived="$1" en_doc="$2" rc=0
  [[ -f "$en_doc" ]] || {
    echo "    $en_doc does not exist" >&2
    return 1
  }
  doc_rows "$en_doc" | LC_ALL=C sort > "$WORK/en_rows"
  [[ -s "$WORK/en_rows" ]] || {
    echo "    no generated-block rows found in $en_doc — the extraction is reading nothing" >&2
    return 1
  }
  for section in route resolve external; do
    grep -q "^$section|" "$WORK/en_rows" || {
      echo "    the $section block of $en_doc has zero rows" >&2
      rc=1
    }
  done
  [[ "$rc" -eq 0 ]] || return "$rc"
  LC_ALL=C sort "$derived" > "$WORK/derived_sorted"
  local undocumented spurious
  undocumented="$(comm -23 "$WORK/derived_sorted" "$WORK/en_rows")"
  if [[ -n "$undocumented" ]]; then
    echo "    source declares row(s) the guide does not document:" >&2
    printf '      %s\n' "$undocumented" >&2
    rc=1
  fi
  spurious="$(comm -13 "$WORK/derived_sorted" "$WORK/en_rows")"
  if [[ -n "$spurious" ]]; then
    echo "    the guide documents row(s) the source does not declare:" >&2
    printf '      %s\n' "$spurious" >&2
    rc=1
  fi
  return "$rc"
}

check_zh_doc_matches_en() {
  local en_doc="$1" zh_doc="$2"
  [[ -f "$zh_doc" ]] || {
    echo "    $zh_doc does not exist" >&2
    return 1
  }
  doc_rows "$en_doc" > "$WORK/en_rows_for_zh"
  doc_rows "$zh_doc" > "$WORK/zh_rows"
  if ! diff "$WORK/en_rows_for_zh" "$WORK/zh_rows" > "$WORK/zh_diff" 2>&1; then
    echo "    EN and ZH generated blocks declare different rows:" >&2
    sed 's/^/      /' "$WORK/zh_diff" >&2
    return 1
  fi
  [[ -s "$WORK/zh_rows" ]] || {
    echo "    the ZH generated blocks have zero rows" >&2
    return 1
  }
}

ALL_CHECKS=(check_derivation_produces_rows check_en_doc_matches_derived check_zh_doc_matches_en)

# meta: every check_* function defined in this file is registered.
defined_checks() {
  grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()//' | LC_ALL=C sort
}

DERIVED="$WORK/derived"
if ! derive_rows > "$DERIVED" 2> "$WORK/derive.err"; then
  echo "  FAIL: the derivation itself failed:" >&2
  sed 's/^/    /' "$WORK/derive.err" >&2
  echo ""
  echo "0 passed, 1 failed"
  exit 1
fi

if [[ "$MODE" == "--emit" ]]; then
  echo "<!-- attention-routing:begin -->"
  echo "| Source event(s) | Condition | Inbox kind | Severity |"
  echo "|---|---|---|---|"
  grep '^route|' "$DERIVED" | awk -F'|' '{printf "| %s | %s | `%s` | %s |\n", $2, $3, $4, $5}'
  echo "<!-- attention-routing:end -->"
  echo ""
  echo "<!-- attention-resolution:begin -->"
  echo "| Trigger event(s) | Scope | Preserved kinds | Resolution reason |"
  echo "|---|---|---|---|"
  grep '^resolve|' "$DERIVED" | awk -F'|' '{printf "| %s | %s | %s | %s |\n", $2, $3, $4, $5}'
  echo "<!-- attention-resolution:end -->"
  echo ""
  echo "<!-- attention-external-kinds:begin -->"
  grep '^external|' "$DERIVED" | awk -F'|' '{printf "- `%s`\n", $2}'
  echo "<!-- attention-external-kinds:end -->"
  exit 0
fi

echo "=== FR-162: attention routing documentation parity ==="
echo ""

registered="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort)"
if [[ "$registered" == "$(defined_checks)" ]]; then
  pass "meta: ALL_CHECKS registers every check function defined in this script"
else
  fail "meta: ALL_CHECKS drifted from the defined check functions"
fi

if check_derivation_produces_rows "$DERIVED"; then
  pass "the derived set is non-empty and contains the FR-162 anchor rows"
else
  fail "the derivation lost its scope"
fi

if check_en_doc_matches_derived "$DERIVED" "$EN_DOC"; then
  pass "the guide documents exactly the source-derived rows ($(wc -l < "$DERIVED" | tr -d ' ') rows)"
else
  fail "the EN guide and the source disagree"
fi

if check_zh_doc_matches_en "$EN_DOC" "$ZH_DOC"; then
  pass "the ZH guide declares the same rows as the EN guide"
else
  fail "the ZH guide drifted from EN"
fi

if [[ "$MODE" == "--fixture-test" ]]; then
  echo ""
  echo "--- negative fixtures ---"

  # Positive control is the real-mode result above: if it failed, the fixture
  # verdicts below are void, and the gate exits non-zero either way.

  # Fixture 1: a new routing arm in a private copy of the policy source. Adding
  # — not deleting — is the mutation the comparison is least tuned for: the doc
  # still contains only true rows, and only the derived side moves.
  cp "$REPO_ROOT/$POLICY_DEFAULT" "$WORK/policy_mutated.rs"
  if fixture_mutate "fixture 1" "$WORK/policy_mutated.rs" \
      sh -c 'sed "s|\"task_failed\" => Some((|\"phantom_event_fr162\" => Some((\n            \"phantom_kind_fr162\",\n            AttentionSeverity::Intervention,\n            \"Phantom\",\n        )),\n        \"task_failed\" => Some((|" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$WORK/policy_mutated.rs"; then
    if ! ATTENTION_POLICY_SRC="$WORK/policy_mutated.rs" derive_rows > "$WORK/derived_mutated" 2> "$WORK/f1.err"; then
      fail "fixture 1: the derivation rejected the mutated source instead of deriving it"
      sed 's/^/    /' "$WORK/f1.err" >&2
    else
      f1_out="$WORK/f1.log"
      if check_en_doc_matches_derived "$WORK/derived_mutated" "$EN_DOC" > "$f1_out" 2>&1; then
        fail "fixture 1: a routing arm missing from the guide was accepted"
      elif ! grep -q "phantom_kind_fr162" "$f1_out"; then
        fail "fixture 1: rejected, but the diagnostic does not name the new arm's kind"
      else
        pass "fixture 1: undocumented new arm rejected, diagnostic names phantom_kind_fr162"
      fi
    fi
  fi

  # Fixture 2: one routing row commented out in a private copy of the guide.
  # Commenting out keeps the kind's name on the page; only the anchored row
  # extraction may see the difference.
  victim_kind="$(grep '^route|' "$DERIVED" | head -1 | cut -d'|' -f4)"
  cp "$EN_DOC" "$WORK/en_mutated.md"
  if fixture_mutate "fixture 2" "$WORK/en_mutated.md" \
      ruby -e 'path, kind = ARGV; text = File.read(path); row = text.each_line.find { |l| l.start_with?("|") && l.include?("`#{kind}`") } or abort "row for #{kind} not found"; File.write(path, text.sub(row, "<!-- #{row.chomp} -->\n"))' "$WORK/en_mutated.md" "$victim_kind"; then
    f2_out="$WORK/f2.log"
    if check_en_doc_matches_derived "$DERIVED" "$WORK/en_mutated.md" > "$f2_out" 2>&1; then
      fail "fixture 2: a guide missing the '$victim_kind' row was accepted"
    elif ! grep -q "$victim_kind" "$f2_out"; then
      fail "fixture 2: rejected, but the diagnostic does not name the missing row"
    else
      pass "fixture 2: commented-out row rejected, diagnostic names '$victim_kind'"
    fi
  fi

  # Fixture 3: a ZH mirror that lost the same row, in a private copy.
  cp "$ZH_DOC" "$WORK/zh_mutated.md"
  if fixture_mutate "fixture 3" "$WORK/zh_mutated.md" \
      ruby -e 'path, kind = ARGV; text = File.read(path); row = text.each_line.find { |l| l.start_with?("|") && l.include?("`#{kind}`") } or abort "row for #{kind} not found"; File.write(path, text.sub(row, "<!-- #{row.chomp} -->\n"))' "$WORK/zh_mutated.md" "$victim_kind"; then
    f3_out="$WORK/f3.log"
    if check_zh_doc_matches_en "$EN_DOC" "$WORK/zh_mutated.md" > "$f3_out" 2>&1; then
      fail "fixture 3: a ZH mirror missing the '$victim_kind' row was accepted"
    elif ! grep -q "$victim_kind" "$f3_out"; then
      fail "fixture 3: rejected, but the diagnostic does not name the divergent row"
    else
      pass "fixture 3: ZH divergence rejected, diagnostic names '$victim_kind'"
    fi
  fi

  # Fixture 4: derivation pointed at a source with no arms must abort, not
  # hand the comparison an empty set.
  printf 'fn unrelated() {}\n' > "$WORK/armless.rs"
  if ATTENTION_POLICY_SRC="$WORK/armless.rs" derive_rows > "$WORK/derived_empty" 2> "$WORK/f4.err"; then
    fail "fixture 4: an armless source derived successfully"
  elif [[ -s "$WORK/derived_empty" ]]; then
    fail "fixture 4: the failed derivation still emitted rows"
  else
    pass "fixture 4: armless source fails closed ($(head -1 "$WORK/f4.err" | cut -c1-60)...)"
  fi
fi

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
exit 0
