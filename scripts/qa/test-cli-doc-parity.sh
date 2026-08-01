#!/usr/bin/env bash
#
# FR-154: the three CLI documentation surfaces derive from the clap tree.
#
# config/governance/cli-surface.json is a committed CommandFactory dump of the
# CLI, kept fresh by a cargo test (cli_surface_json_is_fresh). This gate holds
# the documentation surfaces to it:
#
#   1. the surface itself is readable and plausibly sized (fail-closed anchor:
#      an empty or unparseable surface must never hand the coverage checks an
#      empty set they would happily satisfy — §4.4 shape 5);
#   2. docs/guide/07-cli-reference.md covers every visible invocable path;
#   3. docs/guide/zh/07-cli-reference.md covers the same full set (EN ≡ ZH
#      follows: both equal the surface);
#   4. every `orchestrator …` invocation either document shows prefix-matches
#      a real, visible command — a documented-but-removed, misspelled, or
#      hidden command fails;
#   5. the built-in guide's command entries (crates/cli/src/commands/guide.rs)
#      equal the surface's invocable set bidirectionally — the cargo-free twin
#      of the guide_matches_clap_leaves unit test, so the ci-required claim
#      does not depend on a cargo build in the governance job.
#
# Coverage semantics (shared by checks 2-4): candidates are inline backtick
# spans plus fenced-code-block lines, with HTML comments stripped FIRST — so a
# commented-out mention stops counting, which is exactly the mutation the
# fixtures apply. A leading `$ ` and the `orchestrator ` binary name are
# normalized away. A path is covered when a candidate equals it or begins with
# it followed by a space. Check 4 walks each invocation's leading command-like
# tokens to the longest known path (last-segment aliases accepted): no match,
# a hidden match, or an unmatched subcommand token under a non-invocable
# parent is a violation; tokens beyond an invocable command are positional
# arguments and never flagged.
#
# The set is derived from the surface JSON, never hand-typed (§4.4 shape 2);
# every abort in the derivation is a gate failure, not a skip (§4.4 shape 7).
#
# Usage:
#   test-cli-doc-parity.sh                 verify the real repository
#   test-cli-doc-parity.sh --fixture-test  also prove each check fails on an injected defect

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

SURFACE="$REPO_ROOT/config/governance/cli-surface.json"
EN_DOC="$REPO_ROOT/docs/guide/07-cli-reference.md"
ZH_DOC="$REPO_ROOT/docs/guide/zh/07-cli-reference.md"
GUIDE_RS="$REPO_ROOT/crates/cli/src/commands/guide.rs"

# ── The shared engine ─────────────────────────────────────────────────────────
#
# One ruby program, mode-dispatched, so every check reads the surface and the
# markdown the same way. Modes:
#   paths      <surface>          all visible invocable paths, sorted
#   missing    <surface> <doc>    invocable paths the doc does not cover
#   violations <surface> <doc>    doc invocations that match no real visible command
#   guide      <surface> <guide.rs>  two-sided diff of guide command entries vs surface
# Every derivation failure aborts with a diagnostic; the caller records a FAIL.
engine() {
  ruby - "$@" <<'RUBY'
require "json"

MODE = ARGV.shift

def load_surface(path)
  doc = JSON.parse(File.read(path))
  commands = doc["commands"]
  abort "surface #{path} has no commands array" unless commands.is_a?(Array)
  abort "surface #{path} holds #{commands.size} commands — implausibly small" if commands.size < 100
  commands
rescue Errno::ENOENT
  abort "surface #{path} does not exist"
rescue JSON::ParserError => e
  abort "surface #{path} is not valid JSON: #{e.message[0, 120]}"
end

def invocable(commands)
  commands.select { |c| !c["hidden"] && (c["leaf"] || c["bare_invocable"]) }
          .map { |c| c["path"] }
end

# Inline spans + fenced lines, HTML comments stripped first so a commented-out
# mention stops counting.
def candidates(doc_path)
  text = File.read(doc_path).gsub(/<!--.*?-->/m, "")
  list = text.scan(/`([^`]+)`/).flatten
  in_fence = false
  text.each_line do |line|
    if line.strip.start_with?("```")
      in_fence = !in_fence
      next
    end
    list << line.strip if in_fence
  end
  abort "extracted zero candidates from #{doc_path} — the extraction is reading nothing" if list.empty?
  list.map { |s| s.sub(/\A\$\s+/, "") }
rescue Errno::ENOENT
  abort "document #{doc_path} does not exist"
end

def covered(paths, cands)
  normalized = cands.map { |s| s.sub(/\Aorchestrator\s+/, "") }
  paths.select { |p| normalized.any? { |s| s == p || s.start_with?("#{p} ") } }
end

case MODE
when "paths"
  commands = load_surface(ARGV[0])
  puts invocable(commands).sort
when "missing"
  commands = load_surface(ARGV[0])
  paths = invocable(commands)
  missing = paths - covered(paths, candidates(ARGV[1]))
  puts missing.sort
when "violations"
  commands = load_surface(ARGV[0])
  by_path = {}
  commands.each { |c| by_path[c["path"]] = c }
  # Last-segment aliases: `gd` for `guide`, etc.
  commands.each do |c|
    (c["aliases"] || []).each do |a|
      segs = c["path"].split(" ")
      segs[-1] = a
      by_path[segs.join(" ")] ||= c
    end
  end
  invocations = candidates(ARGV[1]).select { |s| s =~ /\Aorchestrator\s+\S/ }
  abort "found zero `orchestrator …` invocations in #{ARGV[1]}" if invocations.empty?
  bad = []
  invocations.each do |inv|
    tokens = inv.sub(/\Aorchestrator\s+/, "").split(/\s+/)
    lead = tokens.take_while { |t| t =~ /\A[a-z][a-z0-9-]*\z/ }
    next if lead.empty? # placeholder like `orchestrator <command>`
    matched = nil
    lead.size.downto(1) do |j|
      key = lead[0, j].join(" ")
      if by_path.key?(key)
        matched = [key, j]
        break
      end
    end
    if matched.nil?
      bad << "#{inv.inspect}: '#{lead[0]}' is not a command"
      next
    end
    key, j = matched
    node = by_path[key]
    if node["hidden"]
      bad << "#{inv.inspect}: '#{key}' is hidden and must not be documented"
    elsif !(node["leaf"] || node["bare_invocable"]) && j < lead.size
      bad << "#{inv.inspect}: '#{key} #{lead[j]}' is not a command"
    end
  end
  puts bad
when "guide"
  commands = load_surface(ARGV[0])
  paths = invocable(commands).sort
  source = File.read(ARGV[1])
  topic_region = source[/^fn topic_entries.*?(?=^fn )/m]
  abort "cannot locate fn topic_entries in #{ARGV[1]} — the extraction anchor moved" if topic_region.nil?
  extract = lambda do |text|
    text.each_line.reject { |l| l.strip.start_with?("//") }
        .flat_map { |l| l.scan(/command: "([^"]+)"/).flatten }
  end
  topics = extract.call(topic_region)
  abort "fn topic_entries yielded zero commands — the extraction anchor is wrong" if topics.empty?
  guide = (extract.call(source) - topics).sort
  abort "extracted zero command entries from #{ARGV[1]}" if guide.empty?
  (paths - guide).each { |p| puts "missing in guide: #{p}" }
  (guide - paths).each { |p| puts "unknown in guide: #{p}" }
else
  abort "unknown engine mode #{MODE.inspect}"
end
RUBY
}

# ── Checks ────────────────────────────────────────────────────────────────────
# Every check takes explicit inputs so fixtures can substitute private copies.

check_surface_readable() {
  local surface="$1" count
  count="$(engine paths "$surface" 2> "$WORK/surface.err" | awk 'END { print NR + 0 }')" || {
    sed 's/^/    /' "$WORK/surface.err" >&2
    return 1
  }
  if [[ -s "$WORK/surface.err" ]]; then
    sed 's/^/    /' "$WORK/surface.err" >&2
    return 1
  fi
  [[ "$count" -ge 100 ]] || {
    echo "    surface yields only $count invocable paths — implausibly small" >&2
    return 1
  }
}

check_en_doc_covers_surface() {
  local surface="$1" doc="$2" missing
  missing="$(engine missing "$surface" "$doc" 2> "$WORK/en.err")" || {
    sed 's/^/    /' "$WORK/en.err" >&2
    return 1
  }
  if [[ -n "$missing" ]]; then
    echo "    $doc does not cover:" >&2
    echo "$missing" | sed 's/^/      /' >&2
    return 1
  fi
}

check_zh_doc_covers_surface() {
  local surface="$1" doc="$2" missing
  missing="$(engine missing "$surface" "$doc" 2> "$WORK/zh.err")" || {
    sed 's/^/    /' "$WORK/zh.err" >&2
    return 1
  }
  if [[ -n "$missing" ]]; then
    echo "    $doc does not cover:" >&2
    echo "$missing" | sed 's/^/      /' >&2
    return 1
  fi
}

check_docs_reference_only_real_commands() {
  local surface="$1" rc=0 doc bad
  for doc in "$2" "$3"; do
    bad="$(engine violations "$surface" "$doc" 2> "$WORK/viol.err")" || {
      sed 's/^/    /' "$WORK/viol.err" >&2
      rc=1
      continue
    }
    if [[ -n "$bad" ]]; then
      echo "    $doc references commands that do not exist (or are hidden):" >&2
      echo "$bad" | sed 's/^/      /' >&2
      rc=1
    fi
  done
  return "$rc"
}

check_guide_matches_surface() {
  local surface="$1" guide_rs="$2" diff
  diff="$(engine guide "$surface" "$guide_rs" 2> "$WORK/guide.err")" || {
    sed 's/^/    /' "$WORK/guide.err" >&2
    return 1
  }
  if [[ -n "$diff" ]]; then
    echo "    built-in guide and clap surface disagree:" >&2
    echo "$diff" | sed 's/^/      /' >&2
    return 1
  fi
}

ALL_CHECKS=(
  check_surface_readable
  check_en_doc_covers_surface
  check_zh_doc_covers_surface
  check_docs_reference_only_real_commands
  check_guide_matches_surface
)

defined_checks() {
  grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()//' | LC_ALL=C sort
}

echo "=== FR-154: CLI three-surface documentation parity ==="
echo ""

registered="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort)"
if [[ "$registered" == "$(defined_checks)" ]]; then
  pass "meta: ALL_CHECKS registers every check function defined in this script"
else
  fail "meta: ALL_CHECKS drifted from the defined check functions"
fi

if check_surface_readable "$SURFACE"; then
  pass "cli-surface.json is readable and plausibly sized"
else
  fail "cli-surface.json failed the fail-closed anchor"
fi

if check_en_doc_covers_surface "$SURFACE" "$EN_DOC"; then
  pass "EN reference covers every visible invocable path"
else
  fail "EN reference is missing commands"
fi

if check_zh_doc_covers_surface "$SURFACE" "$ZH_DOC"; then
  pass "ZH reference covers every visible invocable path"
else
  fail "ZH reference is missing commands"
fi

if check_docs_reference_only_real_commands "$SURFACE" "$EN_DOC" "$ZH_DOC"; then
  pass "every documented invocation resolves to a real visible command"
else
  fail "the references mention commands that do not exist or are hidden"
fi

if check_guide_matches_surface "$SURFACE" "$GUIDE_RS"; then
  pass "built-in guide command entries equal the surface invocable set"
else
  fail "the built-in guide drifted from the clap surface"
fi

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo ""
  echo "--- negative fixtures ---"

  # Which check each fixture trips; compared against ALL_CHECKS at the end so
  # no registered check goes unproven.
  TARGETED=(
    check_surface_readable
    check_en_doc_covers_surface
    check_zh_doc_covers_surface
    check_docs_reference_only_real_commands
    check_docs_reference_only_real_commands
    check_guide_matches_surface
  )

  # The victim is derived from the surface, never named (§4.4 shape 7 /
  # fixture-target-drift): the first invocable path both documents cover.
  victim="$(engine paths "$SURFACE" | sed -n '1p')"
  [[ -n "$victim" ]] || { fail "fixture setup: could not derive a victim path"; victim="__none__"; }

  # Fixture 0: a truncated surface copy. The anchor check must fail rather
  # than hand the coverage checks an empty set.
  printf '{"version": 1, "commands": [' > "$WORK/surface_truncated.json"
  if check_surface_readable "$WORK/surface_truncated.json" > "$WORK/f0.log" 2>&1; then
    fail "fixture 0: a truncated surface was accepted"
  else
    pass "fixture 0: truncated surface rejected by the fail-closed anchor"
  fi

  # Fixture 1: comment out — not delete — every EN line mentioning the victim,
  # in a private copy. Comment-out is the mutation the extractor is least
  # likely to handle by accident: the path text stays on the page and only the
  # comment-stripping pass may see the difference.
  cp "$EN_DOC" "$WORK/en_mutated.md"
  if fixture_mutate "fixture 1" "$WORK/en_mutated.md" \
      ruby -e 'path = ARGV[0]; victim = ARGV[1]
               out = File.read(path).each_line.map { |l|
                 l.include?(victim) ? "<!-- #{l.chomp} -->\n" : l
               }.join
               File.write(path, out)' "$WORK/en_mutated.md" "$victim"; then
    if check_en_doc_covers_surface "$SURFACE" "$WORK/en_mutated.md" > "$WORK/f1.log" 2>&1; then
      fail "fixture 1: an EN reference missing '$victim' was accepted"
    elif ! grep -F -q "$victim" "$WORK/f1.log"; then
      fail "fixture 1: rejected, but the diagnostic does not name '$victim'"
    else
      pass "fixture 1: commented-out EN coverage rejected, diagnostic names '$victim'"
    fi
  fi

  # Fixture 2: the same mutation on a ZH copy only.
  cp "$ZH_DOC" "$WORK/zh_mutated.md"
  if fixture_mutate "fixture 2" "$WORK/zh_mutated.md" \
      ruby -e 'path = ARGV[0]; victim = ARGV[1]
               out = File.read(path).each_line.map { |l|
                 l.include?(victim) ? "<!-- #{l.chomp} -->\n" : l
               }.join
               File.write(path, out)' "$WORK/zh_mutated.md" "$victim"; then
    if check_zh_doc_covers_surface "$SURFACE" "$WORK/zh_mutated.md" > "$WORK/f2.log" 2>&1; then
      fail "fixture 2: a ZH reference missing '$victim' was accepted"
    elif ! grep -F -q "$victim" "$WORK/f2.log"; then
      fail "fixture 2: rejected, but the diagnostic does not name '$victim'"
    else
      pass "fixture 2: ZH-only coverage loss rejected, diagnostic names '$victim'"
    fi
  fi

  # Fixture 3: a surface copy that no longer knows a leaf the docs invoke —
  # the documented-but-removed direction of check 4.
  if fixture_produce "fixture 3" "$WORK/surface_dropped.json" \
      ruby -rjson -e 'doc = JSON.parse(File.read(ARGV[0]))
                      before = doc["commands"].size
                      doc["commands"].reject! { |c| c["path"] == ARGV[1] }
                      abort "victim #{ARGV[1]} not present" unless doc["commands"].size == before - 1
                      File.write(ARGV[2], JSON.pretty_generate(doc))' \
      "$SURFACE" "$victim" "$WORK/surface_dropped.json"; then
    if check_docs_reference_only_real_commands "$WORK/surface_dropped.json" "$EN_DOC" "$ZH_DOC" \
        > "$WORK/f3.log" 2>&1; then
      fail "fixture 3: documentation for removed command '$victim' was accepted"
    elif ! grep -F -q "${victim##* }" "$WORK/f3.log"; then
      fail "fixture 3: rejected, but the diagnostic does not name the removed command"
    else
      pass "fixture 3: documented-but-removed command rejected, diagnostic names it"
    fi
  fi

  # Fixture 4: a surface copy that hides a leaf the docs invoke.
  if fixture_produce "fixture 4" "$WORK/surface_hidden.json" \
      ruby -rjson -e 'doc = JSON.parse(File.read(ARGV[0]))
                      node = doc["commands"].find { |c| c["path"] == ARGV[1] }
                      abort "victim #{ARGV[1]} not present" unless node
                      node["hidden"] = true
                      File.write(ARGV[2], JSON.pretty_generate(doc))' \
      "$SURFACE" "$victim" "$WORK/surface_hidden.json"; then
    if check_docs_reference_only_real_commands "$WORK/surface_hidden.json" "$EN_DOC" "$ZH_DOC" \
        > "$WORK/f4.log" 2>&1; then
      fail "fixture 4: documentation for hidden command '$victim' was accepted"
    elif ! grep -F -q "$victim" "$WORK/f4.log"; then
      fail "fixture 4: rejected, but the diagnostic does not name the hidden command"
    else
      pass "fixture 4: documented hidden command rejected, diagnostic names '$victim'"
    fi
  fi

  # Fixture 5: comment out one GuideEntry's command line in a guide.rs copy.
  cp "$GUIDE_RS" "$WORK/guide_mutated.rs"
  if fixture_mutate "fixture 5" "$WORK/guide_mutated.rs" \
      ruby -e 'path = ARGV[0]; victim = ARGV[1]
               needle = "command: \"#{victim}\""
               text = File.read(path)
               mutated = false
               out = text.each_line.map { |l|
                 if !mutated && l.include?(needle)
                   mutated = true
                   l.sub(/^(\s*)/) { "#{Regexp.last_match(1)}// " }
                 else
                   l
                 end
               }.join
               abort "victim entry not found" unless mutated
               File.write(path, out)' "$WORK/guide_mutated.rs" "$victim"; then
    if check_guide_matches_surface "$SURFACE" "$WORK/guide_mutated.rs" > "$WORK/f5.log" 2>&1; then
      fail "fixture 5: a guide missing '$victim' was accepted"
    elif ! grep -F -q "missing in guide: $victim" "$WORK/f5.log"; then
      fail "fixture 5: rejected, but the diagnostic does not name '$victim'"
    else
      pass "fixture 5: commented-out guide entry rejected, diagnostic names '$victim'"
    fi
  fi

  # meta: every registered check is proven by at least one fixture.
  targeted_sorted="$(printf '%s\n' ${TARGETED[@]+"${TARGETED[@]}"} | LC_ALL=C sort -u)"
  registered_sorted="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort -u)"
  if [[ "$targeted_sorted" == "$registered_sorted" ]]; then
    pass "meta: every registered check is proven by at least one fixture"
  else
    fail "meta: some registered check has no negative fixture"
  fi
fi

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
exit 0
