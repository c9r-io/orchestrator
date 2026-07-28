#!/usr/bin/env ruby
# frozen_string_literal: true
#
# FR-143: a negative fixture must prove it applied a mutation.
#
# FR-129 and FR-134 each built two meta assertions over a gate's check registry:
# every check is registered, and every registered check has a negative fixture.
# They answer "does this check exist" and "has anyone tried to make it fail".
# Neither asks whether that attempt actually applied a mutation.
#
# Nine recorded times a fixture's target moved out from under it. Eight of the
# nine stayed green. The targets were enumerated — a named file, a named
# statement — and the governed code moving is precisely what these gates exist
# to permit. This is the enumerated-coverage defect the whole FR-127..FR-142
# round spent itself eliminating from the objects under test, still living in
# the fixtures that check them.
#
# Scope is derived from config/governance/qa-gate-surface.json — the ci-required
# shell gates — and never listed here. A hand-written roster would guard exactly
# what was known the day it was written, which is the defect.
#
# It parses with scripts/lib/shell_lexer.rb rather than grepping, and joins
# statements from per-line lexer state. That is load-bearing rather than
# fastidious: a fixture's `ruby -e '...'` body is a multi-line single-quoted
# region, so a joiner that follows only backslash continuations reads the opener
# and the closing `' "$DIR/..."` as two statements and misses every mutation
# target in the repository. Measured during FR-143: that mistake finds 1
# in-place site where there are 27.
#
# Ruby 2.6 compatible: macOS system ruby is 2.6 and the macOS CI leg runs it.

require "json"
require "pathname"

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "shell_lexer"

ROOT = Pathname.new(Dir.pwd)
MANIFEST = "config/governance/qa-gate-surface.json"

# A statement, with the line it began on. Continues while the lexer says a quote
# or a command substitution is still open, and across the shell's own
# continuations.
def statements(text)
  state = ShellLexer::State.new
  out = []
  current = nil
  text.lines.each_with_index do |raw, index|
    line = raw.chomp
    number = index + 1
    if state.in_heredoc?
      state.drop_heredoc_line
      state.close_heredoc if line.strip == state.heredoc
      next
    end
    code = ShellLexer.scan_line(line, state, number)
    if current
      current[1] = "#{current[1]} #{code.strip}"
      current[2] = number
    else
      next if code.strip.empty?

      current = [number, code.strip, number]
    end
    next if state.quote || state.nested? || code.rstrip.end_with?("\\", "&&", "||")

    out << current
    current = nil
  end
  out << current if current
  [out, state]
end

# The scratch trees a gate builds, discovered rather than listed. A variable is
# a scratch root when it is assigned from mktemp -d, from new_case, or from a
# string that already names one. Listing the names — DIR, d, BASE, PROBE — is
# the same enumeration this gate exists to reject, one level in.
def scratch_variables(stmts)
  roots = %w[WORK FIXTURE_ROOT]
  loop do
    before = roots.length
    stmts.each do |_, text|
      name, value = text.match(/\A(?:local\s+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)\z/)&.captures
      next unless name
      next if roots.include?(name)

      value = value.strip.sub(/\A"/, "").sub(/"\z/, "")
      # A root is a *path*, not a value read out of one. Without this,
      # `BEFORE="$(digest "$DIR/$LEDGER")"` makes BEFORE a scratch root and
      # every later mention of it looks like a mutation target.
      seeded = value =~ /\A\$\(\s*(?:new_case\b|mktemp\s+-d\b)/
      derived = roots.any? { |root| value =~ /\A\$\{?#{Regexp.escape(root)}\}?\// }
      roots << name if seeded || derived
    end
    break if roots.length == before
  end
  roots
end

def names_scratch?(text, roots)
  roots.any? { |root| text =~ /\$\{?#{Regexp.escape(root)}\b/ }
end

WRAPPERS = /\A(?:if\s+!?\s*)?(?:fixture_mutate|fixture_premise|fixture_produce)\b/.freeze
# The in-place editors this repository uses.
#
# `ruby` alone is not one of them: `(cd "$DIR" && ruby "$GATE")` runs the gate
# under test against the fixture tree, which is the opposite of mutating it.
# Requiring `-e` is what separates an inline rewrite from an invocation, and
# without it the rule reports 96 findings where there are 48 — a scanner
# reporting defects that are not there is worse than the silence it replaces.
#
# An append or a create cannot silently fail to apply, which is the only thing
# the landing proof observes, so neither is in scope. Recorded as a decision in
# DD-155 rather than left to be inferred from a regex.
IN_PLACE = /(?:\A|[;&|(]\s*|\s)(?:ruby(?:\s+-\S+)*\s+-e\b|sed\s+-i\b|perl\s+-i)/.freeze

def findings_for(path, text)
  stmts, state = statements(text)
  roots = scratch_variables(stmts)
  found = []

  stmts.each do |number, statement, _last|
    next if statement =~ WRAPPERS

    # 1. A mutation whose landing nobody proves.
    if statement =~ IN_PLACE && names_scratch?(statement, roots) &&
       statement !~ /\A[A-Za-z_][A-Za-z0-9_]*=/ && statement !~ /\A(?:local|export|declare)\b/
      found << [number, "unproven-mutation",
                "rewrites a fixture tree in place without proving the mutation landed"]
    end

    # 2. An expected diagnostic that restates a ledger value as a literal.
    if statement =~ /grep/ && statement =~ /\b\d+\s*->\s*\d+/
      found << [number, "restated-expectation",
                "an expected diagnostic restates a ledger value as a literal N -> M"]
    end
  end

  # 3. A premise check nobody catches.
  found.concat(aborting_premises(stmts, text))

  # 4. A pass whose only condition is an exit code.
  found.concat(exit_code_only(stmts))

  # 5. The backstop. A clean result over a file the scan only half-read is an
  #    artefact of how much was read, not a property of the file. FR-138 is
  #    exactly that failure in the bash 3.2 scanner, so it is asserted here
  #    rather than inherited on trust.
  if state.in_heredoc?
    found << [state.heredoc_line, "unclosed-heredoc",
              "the file ends inside a here-document opened here, so the scan never reached the rest of it"]
  end

  found.sort_by { |number, rule, _| [number, rule] }
end

# A `ruby -e` body that can abort, whose invocation nobody wraps. The abort
# itself is right — it is the diagnosis, and after FR-143 it reaches the reader
# through fixture_premise's stderr. What was wrong is that nothing caught it:
# `set -e` took the run down, the summary line never printed, and a truncated
# run reads exactly like a complete one.
#
# Two readers, and each is the right one for its half.
#
# Whether the block is wrapped is a question about the *statement*, so it is
# asked of the joined statement: `if fixture_produce "..." "$AGGREGATE" \` and
# the `ruby -e '` on the next line are one statement, and a reader that looked at
# the ruby line alone would report the two correctly wrapped blocks in this
# repository as findings. That was the first thing this rule got wrong.
#
# Whether the block *can* abort is a question about the body, which the lexer
# blanks — it is a single-quoted region, by design and correctly. So the body is
# read from the raw lines inside the statement's own extent. The parse decides
# where to look; the raw text decides what is there.
def aborting_premises(stmts, text)
  lines = text.lines
  found = []
  stmts.each do |first, statement, last|
    next unless statement =~ /\bruby\b[^|;&]*-e\s+'/
    next if statement =~ WRAPPERS

    lines[(first - 1)...last].each_with_index do |line, offset|
      next unless line =~ /\A\s*(abort|raise)\b/

      found << [first, "aborting-premise",
                "a fixture premise at line #{first + offset} aborts the run instead of failing the case"]
      break # one finding per block: the wrapper goes in one place
    end
  end
  found
end

# §4.4, stated mechanically: a proxy may be an additional condition, never the
# only one. An exit code cannot distinguish the branch a gate failed through
# from any other, and cases 12-14 of test-persistence-dependency.sh spent an
# entire FR reporting through `+ file is not in the ledger` while claiming to
# test `~ file sql N -> N+1`.
#
# Both escapes are allowed, because both are real: a diagnostic match, which no
# unrelated pre-existing failure can produce, or a recorded before-run, which is
# what core-boundary case 9 and persistence-dependency case 10 already carry.
def exit_code_only(stmts)
  found = []
  stmts.each_with_index do |(number, statement), index|
    condition = statement[/\A(?:if|elif)\s+\[\[(.+?)\]\]\s*;?\s*then\z/, 1]
    next unless condition
    next unless condition =~ /-ne\s+0/
    next if condition.include?("&&") || condition.include?("||")
    next if statement =~ /grep/

    branch = []
    stmts[(index + 1)..-1].to_a.each do |_, following|
      break if following =~ /\A(?:else|elif|fi)\b/

      branch << following
    end
    next unless branch.any? { |line| line =~ /\Apass\b/ }
    next if branch.any? { |line| line =~ /BEFORE_STATUS|_before\b/ }

    found << [number, "exit-code-only",
              "reports a pass on a non-zero exit code alone, with no diagnostic match and no before-run"]
  end
  found
end

def gates
  manifest = JSON.parse((ROOT + MANIFEST).read)
  manifest.fetch("scripts")
          .select { |entry| entry["enforcement"] == "ci-required" && entry["path"].to_s.end_with?(".sh") }
          .map { |entry| entry["path"] }
          .select { |path| (ROOT + path).file? }
          .sort
end

if ARGV.include?("--list-files")
  puts gates
  exit 0
end

total = 0
gates.each do |path|
  findings = findings_for(path, (ROOT + path).read)
  next if findings.empty?

  findings.each do |number, rule, why|
    warn "#{path}:#{number}: [#{rule}] #{why}"
    total += 1
  end
end

if total.zero?
  puts "Fixture target drift: PASS (#{gates.length} ci-required shell gates scanned)"
  exit 0
end

warn ""
warn "#{total} fixture(s) can report without proving they applied a mutation."
warn "See docs/design_doc/orchestrator/155-fixture-target-drift.md."
exit 1
